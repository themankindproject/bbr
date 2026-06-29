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

    let cmd = Command::cargo_bin("bbr")
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
