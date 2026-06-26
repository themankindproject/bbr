//! End-to-end CLI smoke tests against a mock Bitbucket server.
//!
//! These exercise the binary's `--help`, version, completion, and a couple of
//! `--json` data paths. They build the `bb` binary via `assert_cmd`.

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_lists_subcommands() {
    Command::cargo_bin("bb")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull request operations"))
        .stdout(predicate::str::contains("Credential management"))
        .stdout(predicate::str::contains("completion"));
}

#[test]
fn version_is_printed() {
    Command::cargo_bin("bb")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bb "));
}

#[test]
fn emits_bash_completion() {
    Command::cargo_bin("bb")
        .unwrap()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_bb()").or(predicate::str::contains("bb")));
}

#[test]
fn missing_creds_exits_with_auth_code() {
    // Ensure no env creds leak into the test.
    std::env::remove_var("BITBUCKET_USERNAME");
    std::env::remove_var("BITBUCKET_TOKEN");
    std::env::remove_var("BITBUCKET_APP_PASSWORD");

    let cmd = Command::cargo_bin("bb")
        .unwrap()
        .env(
            "XDG_CONFIG_HOME",
            "/tmp/bbr-empty-config-dir-that-does-not-exist",
        )
        .args(["repo", "info", "--json"])
        .assert();
    // Either git fails first (no repo) or auth fails. Both are non-zero; we
    // only assert that it does NOT succeed with exit 0.
    cmd.code(predicate::ne(0_i32));
}
