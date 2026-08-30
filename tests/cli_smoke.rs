//! End-to-end CLI smoke tests against a mock Bitbucket server.
//!
//! These exercise the binary's `--help`, version, completion, and a couple of
//! `--json` data paths. They build the `bbr` binary via `assert_cmd`.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_lists_subcommands() {
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull request operations"))
        .stdout(predicate::str::contains("Credential management"))
        .stdout(predicate::str::contains("completion"))
        .stdout(predicate::str::contains(
            "Deployment and environment operations",
        ))
        .stdout(predicate::str::contains("Manage repository issues"))
        .stdout(predicate::str::contains("Repository webhook management"))
        .stdout(predicate::str::contains("Browse remote source files"));
}

#[test]
fn version_is_printed() {
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bbr "));
}

#[test]
fn emits_bash_completion() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_bbr()").or(predicate::str::contains("bbr")));
}

#[test]
fn pr_help_lists_review_commands() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("comments"))
        .stdout(predicate::str::contains("tasks"))
        .stdout(predicate::str::contains("conflicts"))
        .stdout(predicate::str::contains("request-changes"));
}

#[test]
fn pr_merge_help_lists_yes_flag() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "merge", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--yes"))
        .stdout(predicate::str::contains("confirmation"));
}

#[test]
fn pr_list_rejects_unknown_sort_field() {
    // Invalid values must fail as usage errors (exit 64) at the CLI layer,
    // before any network or git access.
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "list", "--sort", "not_a_field"])
        .assert()
        .code(64);
}

#[test]
fn pr_list_rejects_unknown_order() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "list", "--order", "sideways"])
        .assert()
        .code(64);
}

#[test]
fn doctor_help_mentions_checks() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("self-checks"))
        .stdout(predicate::str::contains("--strict"));
}

#[test]
fn doctor_json_outputs_check_array() {
    // Doctor must never fail hard without --strict, even with no creds.
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");
    Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-doctor-empty-config-that-does-not-exist",
        )
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"fail\""));
}

#[test]
fn doctor_strict_exits_nonzero_on_failures() {
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");
    // No credentials + unreachable API => failures exist => strict exits 1.
    Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-doctor-empty-config-that-does-not-exist",
        )
        .env("PWD", "/tmp")
        .args(["doctor", "--strict"])
        .assert()
        .code(1);
}

#[test]
fn root_help_lists_common_workflows() {
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Common workflows:"))
        .stdout(predicate::str::contains("bbr ci watch --logs"));
}

#[test]
fn ci_help_lists_examples() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["ci", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("ci watch --logs"));
}

#[test]
fn batch_help_mentions_min_approvals() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["batch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--min-approvals"));
}

#[test]
fn commit_status_help_lists_set() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["commit", "status", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set"));
}

#[test]
fn repo_help_lists_tags() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["repo", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tags"));
}

#[test]
fn missing_creds_exits_with_auth_code() {
    // Ensure no env creds leak into the test.
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");

    // Pre-flight auth check runs before git detection, so even in a
    // non-git directory the failure is a clean auth error (exit 2).
    Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-empty-config-dir-that-does-not-exist",
        )
        .env("PWD", "/tmp")
        .args(["repo", "info", "--json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("credentials"));
}

#[test]
fn schema_lists_models() {
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("schema")
        .assert()
        .success()
        .stdout(predicate::str::contains("Available JSON Schema Models"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("auth"));
}

#[test]
fn schema_prints_specific_model() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["schema", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"StatusOut\""))
        .stdout(predicate::str::contains("\"required\":"));
}

#[test]
fn pr_stack_help_lists_use() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "stack", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("use"))
        .stdout(predicate::str::contains("Select which stack"));
}

#[test]
fn pr_help_lists_create_merge_approve() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("merge"))
        .stdout(predicate::str::contains("approve"))
        .stdout(predicate::str::contains("merge-check"))
        .stdout(predicate::str::contains("add-reviewer"));
}

#[test]
fn pr_create_help_lists_push_and_force() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--push"))
        .stdout(predicate::str::contains("--force"));
}

#[test]
fn pr_create_force_requires_push() {
    // --force only has meaning together with --push; clap enforces that via
    // `requires`, so passing --force alone is a usage error (exit 64).
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "create", "--title", "x", "--force"])
        .assert()
        .code(64);
}

#[test]
fn pr_help_lists_comment_delete() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("comment-delete"));
}

#[test]
fn pr_comment_delete_help_lists_yes_flag() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "comment-delete", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--yes"))
        .stdout(predicate::str::contains("Comment ID"));
}

#[test]
fn repo_help_lists_default_reviewers() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["repo", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("default-reviewers"));
}

#[test]
fn json_mode_emits_structured_error_on_stderr() {
    // No creds + no repo → the command fails; with --json the error must be
    // a machine-readable object on stderr (stable shape for scripting).
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");

    let cmd = Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-empty-config-dir-that-does-not-exist-json",
        )
        .args(["repo", "info", "--json"])
        .assert();
    // Exit is non-zero either way (git fail or auth fail).
    cmd.code(predicate::ne(0_i32))
        .stderr(predicate::str::contains("\"error\""))
        .stderr(predicate::str::contains("\"exit_code\""))
        .stderr(predicate::str::contains("\"message\""));
}

#[test]
fn human_error_includes_hint() {
    // In a non-repo directory, git errors surface with an actionable hint.
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");

    let dir = std::env::temp_dir().join("bbr-no-repo-hint");
    let _ = std::fs::create_dir_all(&dir);
    Command::cargo_bin("bbr")
        .unwrap()
        .env("XDG_CONFIG_HOME", "/tmp/bbr-empty-config-xyz")
        .current_dir(&dir)
        .arg("repo")
        .arg("info")
        .assert()
        .code(predicate::ne(0_i32))
        .stderr(predicate::str::contains("hint:"));
}

// ---------------------------------------------------------------------------
// Exit-code contract
// ---------------------------------------------------------------------------

#[test]
fn usage_errors_exit_64_not_2() {
    // Unknown flags are usage errors: they must NOT collide with the
    // documented exit code 2 (auth failure). Exit 64 distinguishes them.
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("--definitely-not-a-flag")
        .assert()
        .code(64);

    // Invalid enum values are usage errors too.
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["status", "--color", "bogus"])
        .assert()
        .code(64);
}

#[test]
fn help_and_version_exit_zero() {
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("-h")
        .assert()
        .success();
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
    Command::cargo_bin("bbr")
        .unwrap()
        .arg("--version")
        .assert()
        .success();
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["pr", "--help"])
        .assert()
        .success();
}

#[test]
fn auth_status_no_credentials_exits_zero_with_truthful_message() {
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");

    Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-empty-config-dir-that-does-not-exist-auth-status",
        )
        .args(["auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No Bitbucket credentials found"));
}

#[test]
fn auth_status_json_no_credentials_is_valid_json() {
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");

    let output = Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-empty-config-dir-that-does-not-exist-auth-status-json",
        )
        .args(["auth", "status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("auth status --json must be valid JSON: {e}\n{text}"));
    assert_eq!(parsed["authenticated"], false);
    assert_eq!(parsed["source"], "none");
}

#[test]
fn auth_status_with_bad_credentials_fails_nonzero_with_real_error() {
    // Credentials exist (env) but the API base is unreachable: the failure
    // must be surfaced as an error (non-zero), not reported as "not
    // authenticated" with exit 0.
    let output = Command::cargo_bin("bbr")
        .unwrap()
        .env("BITBUCKET_API_BASE", "http://127.0.0.1:1")
        .env("BITBUCKET_USERNAME", "u")
        .env("BITBUCKET_TOKEN", "t")
        .args(["auth", "status", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "API failure must exit non-zero");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("\"error\"") && stderr.contains("exit_code"),
        "json error shape expected on stderr, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// pr diff --json flag interactions
// ---------------------------------------------------------------------------

#[test]
fn pr_diff_raw_json_is_single_valid_json_document() {
    // `--raw --json` must emit exactly one JSON document (not the raw diff
    // text appended after it). Uses the unreachable-API path: the error is a
    // single JSON error object — the important part is stdout stays clean.
    let output = Command::cargo_bin("bbr")
        .unwrap()
        .env("BITBUCKET_API_BASE", "http://127.0.0.1:1")
        .env("BITBUCKET_USERNAME", "u")
        .env("BITBUCKET_TOKEN", "t")
        .args(["pr", "diff", "1", "--raw", "--json"])
        .output()
        .unwrap();
    // stdout must be empty or a single parseable JSON doc — never a raw diff.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(stdout.trim())
            .unwrap_or_else(|e| panic!("stdout must be a single JSON doc: {e}\n{stdout}"));
    }
    // And --json errors are emitted on stderr as structured objects.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("\"error\""),
        "json mode must emit structured error, got: {stderr}"
    );
}

#[test]
fn status_export_json_emits_json_not_slack() {
    // `status --export slack --json`: --json wins and stdout must be a
    // single JSON document.
    let output = Command::cargo_bin("bbr")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-empty-config-dir-that-does-not-exist-export",
        )
        .args(["status", "--export", "slack", "--json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(stdout.trim())
            .unwrap_or_else(|e| panic!("export+json must be a single JSON doc: {e}\n{stdout}"));
    }
    assert!(
        !stdout.contains("*Status for"),
        "slack text must not leak into json mode, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Workspace role validation
// ---------------------------------------------------------------------------

#[test]
fn workspace_list_rejects_unknown_role() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["workspace", "list", "--role", "superuser"])
        .assert()
        .code(64);
}

// ---------------------------------------------------------------------------
// Completion EPIPE resilience
// ---------------------------------------------------------------------------

#[test]
fn completion_piped_to_closed_stdout_does_not_panic() {
    // `bbr completion fish | head -c1` used to panic inside clap_complete on
    // EPIPE. Spawn with stdout piped and drop the read end after a byte.
    use std::io::Read;
    use std::process::{Command as StdCommand, Stdio};

    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("bbr"))
        .args(["completion", "fish"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // Read one byte then drop the pipe — the writer gets EPIPE.
    if let Some(mut pipe) = child.stdout.take() {
        let mut buf = [0u8; 1];
        let _ = pipe.read(&mut buf);
        drop(pipe); // close the read end
    }
    let status = child.wait().unwrap();
    assert!(
        status.code() != Some(101),
        "completion must not panic on broken pipe (got {status:?})"
    );
}

// ---------------------------------------------------------------------------
// stderr EPIPE resilience (regression: SIGABRT / core dump)
// ---------------------------------------------------------------------------

#[test]
#[cfg(unix)]
fn watch_stderr_piped_to_closed_consumer_does_not_abort() {
    // `bbr status --watch 2>&1 | head -1` used to panic on the first stderr
    // write after the consumer closed the pipe; with `panic = "abort"` that
    // became SIGABRT + core dump (exit 134). main() now resets SIGPIPE to
    // SIG_DFL, so the kernel terminates the process cleanly instead.
    use std::io::Read;
    use std::process::{Command as StdCommand, Stdio};

    let mut child = StdCommand::new(assert_cmd::cargo::cargo_bin("bbr"))
        .args([
            "status",
            "--watch",
            "--interval",
            "1",
            "--api-base",
            "http://127.0.0.1:9", // unreachable: each tick errors -> writes stderr
        ])
        .env("BITBUCKET_USERNAME", "fake@example.com")
        .env("BITBUCKET_TOKEN", "fake-token")
        .env("HOME", "/tmp") // avoid picking up real credentials/config
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Read one byte of stderr then drop the pipe — the next write gets EPIPE.
    if let Some(mut pipe) = child.stderr.take() {
        let mut buf = [0u8; 1];
        let _ = pipe.read(&mut buf);
        drop(pipe);
    }

    let status = child.wait().unwrap();
    // A clean SIGPIPE death reports no exit code (signal 13). What we must
    // never see again is a panic (101) or an abort/core dump (134).
    assert!(
        status.code() != Some(101) && status.code() != Some(134),
        "stderr EPIPE must not panic/abort (got {status:?})"
    );
}

// ---------------------------------------------------------------------------
// ci --notify backends + ci tests step filters (PR #50 sibling feature set)
// ---------------------------------------------------------------------------

#[test]
fn ci_watch_help_lists_notify_backends() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["ci", "watch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--notify"))
        .stdout(predicate::str::contains("BACKEND"))
        .stdout(predicate::str::contains("desktop"))
        .stdout(predicate::str::contains("command=<cmd>"));
}

#[test]
fn ci_tail_help_lists_notify_backends() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["ci", "tail", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--notify"))
        .stdout(predicate::str::contains("BACKEND"))
        .stdout(predicate::str::contains("desktop"));
}

#[test]
fn ci_tests_help_lists_step_filters() {
    Command::cargo_bin("bbr")
        .unwrap()
        .args(["ci", "tests", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--failed"))
        .stdout(predicate::str::contains("--latest"));
}

#[test]
fn ci_watch_rejects_bad_notify_value() {
    // `--notify` takes an open-ended value (`command=<cmd>`), so clap accepts
    // it and our runtime parser rejects unknown backends. That maps to the
    // "generic" error (exit 1). HOME is isolated so no real credentials.
    Command::cargo_bin("bbr")
        .unwrap()
        .env("BITBUCKET_USERNAME", "fake@example.com")
        .env("BITBUCKET_TOKEN", "fake-token")
        .env("HOME", "/tmp")
        .args(["ci", "watch", "--notify", "sms"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid --notify value"));
}
