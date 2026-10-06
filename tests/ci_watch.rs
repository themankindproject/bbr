//! `bbr ci watch` termination, exit codes, and log streaming (mock HTTP only).

use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn pipeline(state: Value) -> Value {
    json!({
        "uuid": "{p1}",
        "build_number": 7,
        "state": state,
        "target": {"ref_name": "main", "commit": {"hash": "abc"}},
        "duration_in_seconds": 12
    })
}

fn completed(result: &str) -> Value {
    json!({"name": "COMPLETED", "result": {"name": result}})
}

fn step(uuid: &str, name: &str, state: Value) -> Value {
    json!({"uuid": uuid, "name": name, "state": state, "duration_in_seconds": 3})
}

/// Mount the branch lookup plus `GET /pipelines/{p1}` answered by `state_at(poll)`.
async fn mount_pipeline<F>(server: &MockServer, state_at: F)
where
    F: Fn(usize) -> Value + Send + Sync + 'static,
{
    let polls = Arc::new(AtomicUsize::new(0));
    let state_at = Arc::new(state_at);
    let first = state_at(0);
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pipelines/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"values": [pipeline(first)]})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repositories/ws/repo/pipelines/%7Bp1%7D$"))
        .respond_with(move |_: &Request| {
            let n = polls.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200).set_body_json(pipeline(state_at(n)))
        })
        .mount(server)
        .await;
}

async fn mount_steps(server: &MockServer, steps: Value) {
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines/%7Bp1%7D/steps/?$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "values": steps })))
        .mount(server)
        .await;
}

fn watch(server: &MockServer, args: &[&str]) -> std::process::Output {
    let home = TempDir::new().unwrap();
    assert_cmd::Command::cargo_bin("bbr")
        .unwrap()
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_USERNAME", "test")
        .env("BITBUCKET_TOKEN", "fake-token")
        .env("BITBUCKET_API_BASE", server.uri())
        .env("BB_WORKSPACE", "ws")
        .env("BB_SLUG", "repo")
        .env("BBR_NO_INTERACTIVE", "1")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .args(["ci", "watch", "--branch", "main", "--interval", "1"])
        .args(args)
        .timeout(Duration::from_secs(20))
        .output()
        .unwrap()
}

fn receipt(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

#[tokio::test]
async fn expired_pipeline_finishes_with_failure_exit() {
    let server = MockServer::start().await;
    mount_pipeline(&server, |_| completed("EXPIRED")).await;
    mount_steps(&server, json!([])).await;
    let started = Instant::now();
    let out = watch(&server, &["--json"]);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "watch must not poll forever"
    );
    assert_eq!(out.status.code(), Some(5));
    let r = receipt(&out);
    assert_eq!(r["final_state"], "EXPIRED");
    assert_eq!(r["outcome"], "completed");
    assert_eq!(r["success"], false);
}

#[tokio::test]
async fn paused_pipeline_stops_watching_with_explicit_outcome() {
    let server = MockServer::start().await;
    mount_pipeline(
        &server,
        |_| json!({"name": "IN_PROGRESS", "stage": {"name": "PAUSED"}}),
    )
    .await;
    mount_steps(
        &server,
        json!([
            step("{s1}", "build", completed("SUCCESSFUL")),
            step("{s2}", "deploy (manual)", json!({"name": "PENDING"}))
        ]),
    )
    .await;
    let out = watch(&server, &["--json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "paused is not success and not a failure"
    );
    let r = receipt(&out);
    assert_eq!(r["final_state"], "PAUSED");
    assert_eq!(r["outcome"], "paused");
    assert_eq!(r["success"], false);
    let err: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(err["error"]["message"]
        .as_str()
        .unwrap()
        .contains("manual step"));
}

#[tokio::test]
async fn wait_timeout_bounds_a_running_pipeline() {
    let server = MockServer::start().await;
    mount_pipeline(
        &server,
        |_| json!({"name": "IN_PROGRESS", "stage": {"name": "RUNNING"}}),
    )
    .await;
    mount_steps(
        &server,
        json!([step("{s1}", "build", json!({"name": "IN_PROGRESS"}))]),
    )
    .await;
    let started = Instant::now();
    let out = watch(&server, &["--json", "--wait-timeout", "2"]);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
    assert_eq!(out.status.code(), Some(1));
    let r = receipt(&out);
    assert_eq!(r["outcome"], "timed_out");
    assert_eq!(r["final_state"], "IN_PROGRESS");
}

#[tokio::test]
async fn successful_pipeline_still_exits_zero() {
    let server = MockServer::start().await;
    mount_pipeline(&server, |n| {
        if n < 1 {
            json!({"name": "IN_PROGRESS"})
        } else {
            completed("SUCCESSFUL")
        }
    })
    .await;
    mount_steps(
        &server,
        json!([step("{s1}", "build", completed("SUCCESSFUL"))]),
    )
    .await;
    let out = watch(&server, &["--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r = receipt(&out);
    assert_eq!(r["outcome"], "completed");
    assert_eq!(r["success"], true);
}

#[tokio::test]
async fn logs_reach_a_piped_stderr_sanitized_and_finished_steps_are_not_refetched() {
    let server = MockServer::start().await;
    // Running for three polls, then successful.
    mount_pipeline(&server, |n| {
        if n < 3 {
            json!({"name": "IN_PROGRESS"})
        } else {
            completed("SUCCESSFUL")
        }
    })
    .await;
    mount_steps(
        &server,
        json!([
            step("{s1}", "setup", completed("SUCCESSFUL")),
            step("{s2}", "build", json!({"name": "IN_PROGRESS"}))
        ]),
    )
    .await;
    let setup_fetches = Arc::new(AtomicUsize::new(0));
    let counter = setup_fetches.clone();
    Mock::given(method("GET"))
        .and(path_regex(r"/steps/%7Bs1%7D/log$"))
        .respond_with(move |req: &Request| {
            counter.fetch_add(1, Ordering::SeqCst);
            let ranged = req
                .headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v != "bytes=0-");
            if ranged {
                ResponseTemplate::new(416)
            } else {
                ResponseTemplate::new(200).set_body_string(
                    "setup ok \u{1b}]52;c;aGFjaw==\u{7}done\nprogress 1%\rprogress 100%\n",
                )
            }
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/steps/%7Bs2%7D/log$"))
        .respond_with(ResponseTemplate::new(416))
        .mount(&server)
        .await;

    let out = watch(&server, &["--logs"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("setup ok done"),
        "logs must reach a non-TTY stderr: {stderr}"
    );
    assert!(
        stderr.contains("progress 100%") && !stderr.contains("progress 1%"),
        "{stderr}"
    );
    assert!(
        !stderr.contains('\u{1b}') && !stderr.contains('\u{7}'),
        "{stderr:?}"
    );
    let fetches = setup_fetches.load(Ordering::SeqCst);
    assert!(
        fetches <= 2,
        "a finished, drained step must not be re-requested every tick (got {fetches})"
    );
}

#[tokio::test]
async fn pending_step_logs_are_skipped_and_missing_logs_do_not_warn() {
    let server = MockServer::start().await;
    mount_pipeline(&server, |n| {
        if n < 2 {
            json!({"name": "IN_PROGRESS"})
        } else {
            completed("SUCCESSFUL")
        }
    })
    .await;
    mount_steps(
        &server,
        json!([
            step("{s1}", "build", json!({"name": "IN_PROGRESS"})),
            step("{s2}", "test", json!({"name": "PENDING"}))
        ]),
    )
    .await;
    // The running step's log has not been created yet: Bitbucket answers
    // 404 (documented) until the step flushes its first bytes.
    let running_404s = Arc::new(AtomicUsize::new(0));
    let counter = running_404s.clone();
    Mock::given(method("GET"))
        .and(path_regex(r"/steps/%7Bs1%7D/log$"))
        .respond_with(move |_: &Request| {
            counter.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(404).set_body_string("no log yet")
        })
        .mount(&server)
        .await;
    // A pending step has no log file at all; requesting it must not happen.
    let pending_fetches = Arc::new(AtomicUsize::new(0));
    let counter = pending_fetches.clone();
    Mock::given(method("GET"))
        .and(path_regex(r"/steps/%7Bs2%7D/log$"))
        .respond_with(move |_: &Request| {
            counter.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(404).set_body_string("pending")
        })
        .mount(&server)
        .await;

    let out = watch(&server, &["--logs"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("failed to stream logs"),
        "an expected 404 must not warn on every tick: {stderr}"
    );
    assert!(!stderr.contains("404"), "{stderr}");
    assert!(
        running_404s.load(Ordering::SeqCst) >= 1,
        "a started step must still be polled for its log"
    );
    assert_eq!(
        pending_fetches.load(Ordering::SeqCst),
        0,
        "a pending step has no log: the endpoint must not be requested"
    );
}

#[tokio::test]
async fn finished_step_with_missing_log_stops_being_polled() {
    let server = MockServer::start().await;
    mount_pipeline(&server, |n| {
        if n < 3 {
            json!({"name": "IN_PROGRESS"})
        } else {
            completed("SUCCESSFUL")
        }
    })
    .await;
    mount_steps(
        &server,
        json!([step("{s1}", "build", completed("SUCCESSFUL"))]),
    )
    .await;
    // The step is finished but its log 404s (moved to long-term storage
    // and unavailable, or never written). It must be polled at most twice,
    // not on every remaining tick of the watch.
    let log_fetches = Arc::new(AtomicUsize::new(0));
    let counter = log_fetches.clone();
    Mock::given(method("GET"))
        .and(path_regex(r"/steps/%7Bs1%7D/log$"))
        .respond_with(move |_: &Request| {
            counter.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(404).set_body_string("log expired")
        })
        .mount(&server)
        .await;

    let out = watch(&server, &["--logs"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("failed to stream logs"), "{stderr}");
    assert!(!stderr.contains("404"), "{stderr}");
    let fetches = log_fetches.load(Ordering::SeqCst);
    assert!(
        fetches <= 2,
        "a finished step whose log404s must drain, not be re-requested every tick (got {fetches})"
    );
}

#[tokio::test]
async fn failed_pipeline_exits_5_not_3_when_the_failure_log_is_missing() {
    let server = MockServer::start().await;
    mount_pipeline(&server, |_| completed("FAILED")).await;
    mount_steps(&server, json!([step("{s1}", "build", completed("FAILED"))])).await;
    // Every log request 404s: the excerpt fetch must not override the
    // documented PipelineFailed exit code (5) with NotFound (3).
    Mock::given(method("GET"))
        .and(path_regex(r"/steps/%7Bs1%7D/log$"))
        .respond_with(ResponseTemplate::new(404).set_body_string("gone"))
        .mount(&server)
        .await;

    let out = watch(&server, &[]);
    assert_eq!(
        out.status.code(),
        Some(5),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("404"),
        "a missing excerpt must not surface as an HTTP error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn failed_step_listing_is_reported_not_silently_empty() {
    let server = MockServer::start().await;
    mount_pipeline(&server, |n| {
        if n < 1 {
            json!({"name": "IN_PROGRESS"})
        } else {
            completed("SUCCESSFUL")
        }
    })
    .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines/%7Bp1%7D/steps/?$",
        ))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({"error": {"message": "gone"}})),
        )
        .mount(&server)
        .await;
    let out = watch(&server, &["--logs"]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("failed to list pipeline steps"), "{stderr}");
}
