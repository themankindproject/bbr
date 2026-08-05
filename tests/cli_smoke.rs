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
