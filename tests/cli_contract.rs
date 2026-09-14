//! Public CLI contract tests.
//!
//! These lock in the guarantees documented in README.md and USAGE.md that a
//! script or agent depends on, and that are easy to break by accident:
//!
//! * the exit-code contract (especially usage errors being `64`, not `1`);
//! * destructive commands refusing to act non-interactively without `--yes`;
//! * `--json` never implying consent;
//! * closed value sets rejected by the parser;
//! * credentials never being sent to a non-loopback plaintext host.
//!
//! Every test runs against a throwaway `HOME` and a loopback `--api-base`, so
//! nothing touches the network or the developer's real credentials.

use assert_cmd::Command;

/// Exit code for a usage error (BSD sysexits `EX_USAGE`).
const USAGE: i32 = 64;

/// A `bbr` command with an isolated `HOME` and fake credentials, so tests never
/// read the developer's real config or reach the network.
fn bbr() -> Command {
    let mut cmd = Command::cargo_bin("bbr").unwrap();
    let home = std::env::temp_dir().join(format!("bbr-contract-{}", std::process::id()));
    std::fs::create_dir_all(&home).ok();
    cmd.env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("APPDATA", &home)
        .env("BITBUCKET_USERNAME", "test@example.com")
        .env("BITBUCKET_TOKEN", "not-a-real-token")
        // These test consent/transport, not identity inference from this checkout.
        .env("BB_WORKSPACE", "test-workspace")
        .env("BB_SLUG", "test-repository")
        // Port 9 (discard) refuses instantly, so any command that gets past
        // validation fails fast instead of hanging.
        .env("BITBUCKET_API_BASE", "http://127.0.0.1:9");
    cmd
}

// ---------------------------------------------------------------------------
// Exit-code contract
// ---------------------------------------------------------------------------

#[test]
fn unknown_flag_is_a_usage_error() {
    bbr().arg("--definitely-not-a-flag").assert().code(USAGE);
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    bbr().arg("notasubcommand").assert().code(USAGE);
}

#[test]
fn missing_required_argument_is_a_usage_error() {
    // `pr create` requires --title. (`pr view`/`pr merge` deliberately infer
    // the PR from the current branch, so they are not usable as probes here.)
    bbr().args(["pr", "create"]).assert().code(USAGE);
}

#[test]
fn help_and_version_succeed_without_a_runtime() {
    // These must not need credentials, a network, or an executor.
    bbr().arg("--help").assert().success();
    bbr().arg("--version").assert().success();
    bbr().args(["help", "pr"]).assert().success();
    bbr().args(["pr", "--help"]).assert().success();
}

#[test]
fn help_subcommand_matches_the_flag() {
    let via_subcommand = bbr().args(["help", "pr"]).output().unwrap();
    let via_flag = bbr().args(["pr", "--help"]).output().unwrap();
    assert!(via_subcommand.status.success());
    assert!(via_flag.status.success());
    // Both render the same command help.
    assert_eq!(via_subcommand.stdout, via_flag.stdout);
}

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

#[test]
fn limit_above_the_cap_is_rejected() {
    bbr()
        .args(["api", "GET", "/x", "--limit", "99999999"])
        .assert()
        .code(USAGE);
}

#[test]
fn limit_of_zero_is_rejected() {
    bbr()
        .args(["api", "GET", "/x", "--limit", "0"])
        .assert()
        .code(USAGE);
}

#[test]
fn limit_non_numeric_is_rejected() {
    bbr()
        .args(["api", "GET", "/x", "--limit", "lots"])
        .assert()
        .code(USAGE);
}

#[test]
fn closed_value_sets_reject_unknown_values() {
    // (args, the accepted choices the error should mention)
    let cases: &[(&[&str], &str)] = &[
        (&["pr", "list", "--state", "bogus"], "open"),
        (&["pr", "merge", "1", "--strategy", "bogus"], "squash"),
        (
            &["deploy", "env", "create", "n", "--env-type", "bogus"],
            "production",
        ),
    ];
    for (args, expected_choice) in cases {
        let out = bbr().args(*args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(USAGE),
            "expected usage error for {args:?}"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(expected_choice),
            "error for {args:?} should list valid choices, got: {stderr}"
        );
    }
}

#[test]
fn body_source_flags_are_mutually_exclusive() {
    for args in [
        vec![
            "pr",
            "create",
            "--title",
            "t",
            "--body",
            "a",
            "--body-stdin",
        ],
        vec![
            "pr",
            "create",
            "--title",
            "t",
            "--body",
            "a",
            "--body-file",
            "/tmp/x",
        ],
        vec!["pr", "comment", "1", "--body", "a", "--body-stdin"],
    ] {
        bbr().args(&args).assert().code(USAGE);
    }
}

#[test]
fn ci_log_conflicting_selectors_are_rejected() {
    bbr()
        .args(["ci", "logs", "--step", "x", "--failed"])
        .assert()
        .code(USAGE);
    bbr()
        .args(["ci", "logs", "--failed", "--latest"])
        .assert()
        .code(USAGE);
}

#[test]
fn boolean_flags_accept_an_explicit_false() {
    // `--enabled=false` must parse; otherwise a disabled schedule is
    // impossible to express.
    let out = bbr()
        .args([
            "ci",
            "schedules",
            "create",
            "--cron",
            "0 2 * * *",
            "--branch",
            "main",
            "--enabled=false",
        ])
        .output()
        .unwrap();
    assert_ne!(
        out.status.code(),
        Some(USAGE),
        "--enabled=false should parse: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn auth_setup_requires_both_username_and_token() {
    bbr()
        .args(["auth", "setup", "--username", "someone"])
        .assert()
        .code(USAGE);
    bbr()
        .args(["auth", "setup", "--token", "secret"])
        .assert()
        .code(USAGE);
}

// ---------------------------------------------------------------------------
// Destructive-action safety
// ---------------------------------------------------------------------------

/// Destructive commands that must not act without `--yes` when stdin is not a
/// terminal.
const DESTRUCTIVE: &[&[&str]] = &[
    &["pr", "merge", "999999"],
    &["pr", "decline", "999999"],
    &["pr", "comment-delete", "999999", "1"],
    &["repo", "delete", "some-repo"],
    &["webhook", "delete", "some-uid"],
    &["deploy-keys", "delete", "1"],
    &["ci", "schedules", "delete", "some-uuid"],
    &["ci", "rerun"],
    &["ci", "stop"],
    &["deploy", "rollback", "some-env-uuid"],
    &["batch", "merge-approved"],
    &["batch", "rerun-failed"],
    &["batch", "cleanup-merged-branches"],
    &["pr", "stack", "land"],
    &["pr", "stack", "abort"],
];

/// The universally true safety property: a destructive command must never
/// report success when it was not granted consent.
#[test]
fn destructive_commands_never_succeed_without_yes() {
    for args in DESTRUCTIVE {
        let out = bbr()
            .args(*args)
            .write_stdin("") // stdin is a pipe, not a terminal
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "{args:?} exited 0 without --yes on non-interactive stdin — a script \
             would take the success branch for work that never happened.\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The subset that reaches the consent gate must reject the call with a usage
/// error and name the flag that unblocks it.
///
/// `pr stack land` / `pr stack abort` are excluded: they validate the local git
/// state first, so from a dirty working tree they fail on that precondition
/// before consent is even relevant. Both are still covered by the
/// never-succeeds property above and by the `--yes` acceptance test below.
#[test]
fn destructive_commands_reach_the_consent_gate() {
    const GATED: &[&[&str]] = &[
        &["pr", "merge", "999999"],
        &["pr", "decline", "999999"],
        &["pr", "comment-delete", "999999", "1"],
        &["repo", "delete", "some-repo"],
        &["webhook", "delete", "some-uid"],
        &["deploy-keys", "delete", "1"],
        &["ci", "schedules", "delete", "some-uuid"],
        &["ci", "rerun"],
        &["ci", "stop"],
        &["deploy", "rollback", "some-env-uuid"],
        &["batch", "merge-approved"],
        &["batch", "rerun-failed"],
        &["batch", "cleanup-merged-branches"],
    ];
    for args in GATED {
        let out = bbr().args(*args).write_stdin("").output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(USAGE),
            "{args:?} must exit 64 without --yes on non-interactive stdin, got {:?}\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("--yes"),
            "{args:?} should tell the user to pass --yes, got: {stderr}"
        );
    }
}

#[test]
fn json_does_not_imply_consent() {
    // `--json` selects an output format. It must not silently approve a
    // destructive action.
    for args in [
        vec!["pr", "merge", "999999", "--json"],
        vec!["repo", "delete", "some-repo", "--json"],
        vec!["deploy", "rollback", "an-env", "--json"],
        vec!["batch", "merge-approved", "--json"],
    ] {
        let out = bbr().args(&args).write_stdin("").output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(USAGE),
            "{args:?} with --json must still require --yes, got {:?}",
            out.status.code()
        );
    }
}

#[test]
fn yes_flag_is_accepted_by_every_destructive_command() {
    // `--yes` is the only consent mechanism, so it must exist everywhere a
    // confirmation can appear. Passing it must get past argument parsing; a
    // missing flag would surface as an "unexpected argument" usage error.
    for args in DESTRUCTIVE {
        let mut with_yes: Vec<&str> = args.to_vec();
        with_yes.push("--yes");
        let out = bbr().args(&with_yes).write_stdin("").output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("unexpected argument") && !stderr.contains("unrecognized"),
            "{args:?} does not accept --yes: {stderr}"
        );
        // And with --yes the confirmation gate must not be what stops it.
        assert!(
            !stderr.contains("needs confirmation"),
            "{args:?} still demanded confirmation despite --yes: {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// Credential safety
// ---------------------------------------------------------------------------

#[test]
fn refuses_to_send_credentials_over_plaintext_http() {
    // Use a command that actually sends the token. `doctor` is a diagnostic
    // report and deliberately exits 0 on a failed check (see `--strict`), so
    // it cannot be the probe here.
    let out = bbr()
        .args(["--api-base", "http://evil.example.com/2.0", "pr", "list"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(USAGE));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("http"),
        "should explain the plaintext refusal, got: {stderr}"
    );
}

#[test]
fn refuses_non_http_api_base_schemes() {
    for base in ["ftp://x/2.0", "file:///tmp/x", "not-a-url"] {
        let out = bbr()
            .args(["--api-base", base, "pr", "list"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(USAGE),
            "api-base {base} should be a usage error"
        );
    }
}

#[test]
fn allows_loopback_http_for_tests() {
    // The test suite itself depends on this being allowed: reaching the
    // discard port means validation passed and only the network failed.
    let out = bbr().args(["pr", "list"]).output().unwrap();
    assert_ne!(
        out.status.code(),
        Some(USAGE),
        "loopback http api-base must be allowed"
    );
}

#[test]
fn doctor_reports_a_plaintext_base_as_a_failed_check() {
    // `doctor` must still *surface* the misconfiguration, even though it
    // exits 0 by contract.
    let out = bbr()
        .args(["--api-base", "http://evil.example.com/2.0", "doctor"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("fail"),
        "doctor should flag the plaintext API base: {stdout}"
    );
    // And --strict must turn that into a non-zero exit.
    let strict = bbr()
        .args([
            "--api-base",
            "http://evil.example.com/2.0",
            "doctor",
            "--strict",
        ])
        .output()
        .unwrap();
    assert!(
        !strict.status.success(),
        "--strict should fail on a failed check"
    );
}

// ---------------------------------------------------------------------------
// Output hygiene
// ---------------------------------------------------------------------------

#[test]
fn json_errors_go_to_stderr_as_structured_data() {
    // The documented contract: errors are machine-readable on stderr when
    // --json is in effect, so an agent can branch on `kind`.
    let out = bbr().args(["pr", "list", "--json"]).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    let parsed: serde_json::Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("stderr is not JSON ({e}): {stderr}"));
    assert!(
        parsed.pointer("/error/kind").is_some(),
        "error payload should carry a kind: {stderr}"
    );
    assert!(
        parsed.pointer("/error/exit_code").is_some(),
        "error payload should carry an exit_code: {stderr}"
    );
}
