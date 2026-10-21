//! `ci list` must associate each step list with its own pipeline even when
//! the fetches complete out of order.

use serde_json::{json, Value};
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pip(uuid: &str, build: u64) -> Value {
    json!({
        "uuid": uuid,
        "build_number": build,
        "state": {"name": "IN_PROGRESS"},
        "target": {"ref_name": "main", "commit": {"hash": "abc"}},
        "duration_in_seconds": 1
    })
}

fn step(uuid: &str, name: &str) -> Value {
    json!({
        "uuid": uuid,
        "name": name,
        "state": {"name": "IN_PROGRESS"},
        "duration_in_seconds": 1
    })
}

#[tokio::test]
async fn steps_stay_attached_to_their_pipeline_when_fetches_complete_out_of_order() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pipelines/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [pip("{pa}", 1), pip("{pb}", 2)]
        })))
        .mount(&server)
        .await;

    // The first pipeline's step fetch is deliberately slow, so an unordered
    // buffer would return pipeline B's steps first.
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines/%7Bpa%7D/steps/?$",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(300))
                .set_body_json(json!({"values": [step("{sa}", "A-step")]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines/%7Bpb%7D/steps/?$",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"values": [step("{sb}", "B-step")]})),
        )
        .mount(&server)
        .await;

    let home = TempDir::new().unwrap();
    let out = assert_cmd::Command::cargo_bin("bbr")
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
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .args(["ci", "list", "--branch", "main", "--json"])
        .timeout(Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    let pipelines = receipt["pipelines"].as_array().unwrap();
    for pipeline in pipelines {
        let name = pipeline["steps"][0]["name"].as_str().unwrap();
        match pipeline["build_number"].as_u64().unwrap() {
            1 => assert_eq!(name, "A-step"),
            2 => assert_eq!(name, "B-step"),
            other => panic!("unexpected pipeline {other}"),
        }
    }
}
