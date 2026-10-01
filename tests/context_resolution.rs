//! Context precedence and fail-closed targeting, using disposable Git repositories.

use assert_cmd::Command;
use serde_json::Value;
use std::{fs, path::PathBuf, time::Duration};
use tempfile::{tempdir, TempDir};

struct Fixture {
    home: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new(with_remote: bool) -> Self {
        let home = tempdir().unwrap();
        let repo = home.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(home.path().join("bbr")).unwrap();
        fs::write(home.path().join("gitconfig"), "").unwrap();
        let fixture = Self { home, repo };
        if with_remote {
            fixture.git(&["init", "--quiet"]);
            fixture.git(&[
                "config",
                "--local",
                "remote.origin.url",
                "https://bitbucket.org/git-team/git-repo.git",
            ]);
        }
        fixture
    }

    fn isolate(&self, cmd: &mut Command) {
        cmd.current_dir(&self.repo)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path())
            .env("APPDATA", self.home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.home.path().join("gitconfig"))
            .env("GIT_CEILING_DIRECTORIES", self.home.path())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_CONFIG_COUNT")
            .timeout(Duration::from_secs(5));
    }

    fn git(&self, args: &[&str]) {
        let mut cmd = Command::new("git");
        self.isolate(&mut cmd);
        cmd.args(args).assert().success();
    }

    fn config(&self, text: &str) {
        fs::write(self.home.path().join("bbr/config.toml"), text).unwrap();
    }

    fn bbr(&self) -> Command {
        let mut cmd = Command::cargo_bin("bbr").unwrap();
        self.isolate(&mut cmd);
        cmd.env("BITBUCKET_USERNAME", "test@example.com")
            .env("BITBUCKET_TOKEN", "fake-token")
            .env("BITBUCKET_API_BASE", "http://127.0.0.1:9")
            .env("BBR_NO_INTERACTIVE", "1")
            .env("NO_COLOR", "1")
            .env_remove("BB_WORKSPACE")
            .env_remove("BB_SLUG");
        cmd
    }

    fn assert_target(&self, args: &[&str], workspace: &str, slug: &str) {
        let output = self
            .bbr()
            .args(["open", "pipelines", "--json"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["url"],
            format!("https://bitbucket.org/{workspace}/{slug}/pipelines")
        );
        assert_eq!(value["opened"], false);
    }

    fn assert_config_error(&self, args: &[&str]) {
        let output = self
            .bbr()
            .args(["open", "pipelines", "--json"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty(), "must not select a fallback repo");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "config");
    }
}

const VALID: &str = "active_context = \"work\"\n[contexts.work]\nworkspace = \"context-team\"\nslug = \"context-repo\"\n";

#[test]
fn malformed_context_never_falls_back_to_git_for_missing_identity_fields() {
    let fixture = Fixture::new(true);
    fixture.config("active_context = [\n");
    for args in [
        vec![],
        vec!["--workspace", "explicit-team"],
        vec!["--slug", "explicit-repo"],
    ] {
        fixture.assert_config_error(&args);
    }
}

#[test]
fn stale_active_context_is_an_actionable_error_not_git_fallback() {
    let fixture = Fixture::new(true);
    fixture.config("active_context = \"deleted-context\"\n");
    for args in [
        vec![],
        vec!["--workspace", "explicit-team"],
        vec!["--slug", "explicit-repo"],
    ] {
        fixture.assert_config_error(&args);
    }
    let output = fixture.bbr().args(["open", "pipelines"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("deleted-context"));
    assert!(stderr.contains("bbr context"));
}

#[test]
fn unreadable_config_is_not_treated_as_missing() {
    let fixture = Fixture::new(true);
    fs::create_dir(fixture.home.path().join("bbr/config.toml")).unwrap();
    fixture.assert_config_error(&[]);
}

#[test]
fn full_explicit_target_bypasses_broken_or_stale_context_without_git() {
    let fixture = Fixture::new(false);
    for text in [
        "active_context = [\n",
        "active_context = \"missing\"\n",
        VALID,
    ] {
        fixture.config(text);
        fixture.assert_target(
            &["--workspace", "explicit-team", "--slug", "explicit-repo"],
            "explicit-team",
            "explicit-repo",
        );
    }
}

#[test]
fn valid_context_and_partial_overrides_preserve_precedence() {
    let fixture = Fixture::new(true);
    fixture.config(VALID);
    fixture.assert_target(&[], "context-team", "context-repo");
    fixture.assert_target(
        &["--workspace", "explicit-team"],
        "explicit-team",
        "context-repo",
    );
    fixture.assert_target(
        &["--slug", "explicit-repo"],
        "context-team",
        "explicit-repo",
    );
}

#[test]
fn missing_config_or_no_active_context_uses_git() {
    let fixture = Fixture::new(true);
    fixture.assert_target(&[], "git-team", "git-repo");
    fixture.config("[contexts.inactive]\nworkspace = \"unused\"\nslug = \"unused\"\n");
    fixture.assert_target(&[], "git-team", "git-repo");
    fixture.assert_target(
        &["--workspace", "explicit-team"],
        "explicit-team",
        "git-repo",
    );
    fixture.assert_target(&["--slug", "explicit-repo"], "git-team", "explicit-repo");
}

#[test]
fn workspace_only_context_keeps_its_workspace_when_slug_comes_from_git() {
    let fixture = Fixture::new(true);
    fixture.config("active_context = \"work\"\n[contexts.work]\nworkspace = \"context-team\"\n");
    fixture.assert_target(&[], "context-team", "git-repo");
    fixture.assert_target(
        &["--workspace", "explicit-team"],
        "explicit-team",
        "git-repo",
    );
    fixture.assert_target(
        &["--slug", "explicit-repo"],
        "context-team",
        "explicit-repo",
    );
}

#[test]
fn complete_context_does_not_require_git() {
    let fixture = Fixture::new(false);
    fixture.config(VALID);
    fixture.assert_target(&[], "context-team", "context-repo");
}

#[test]
fn empty_active_context_fields_are_not_usable_targets() {
    let fixture = Fixture::new(true);
    for text in [
        "active_context = \"\"\n",
        "active_context = \"work\"\n[contexts.work]\nworkspace = \"  \"\nslug = \"repo\"\n",
        "active_context = \"work\"\n[contexts.work]\nworkspace = \"team\"\nslug = \"\"\n",
    ] {
        fixture.config(text);
        fixture.assert_config_error(&[]);
    }
}

#[test]
fn full_env_target_can_override_broken_context_and_flags_override_env() {
    let fixture = Fixture::new(false);
    fixture.config("active_context = [\n");
    let output = fixture
        .bbr()
        .env("BB_WORKSPACE", "env-team")
        .env("BB_SLUG", "env-repo")
        .args(["open", "pipelines", "--json", "--slug", "cli-repo"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["url"],
        "https://bitbucket.org/env-team/cli-repo/pipelines"
    );
}

#[test]
fn stale_context_can_be_repaired_using_context_commands() {
    let fixture = Fixture::new(false);
    fixture.config(&VALID.replace("active_context = \"work\"", "active_context = \"missing\""));
    fixture
        .bbr()
        .args(["context", "list", "--json"])
        .assert()
        .success();
    fixture
        .bbr()
        .args(["context", "use", "work", "--json"])
        .assert()
        .success();
    fixture.assert_target(&[], "context-team", "context-repo");
}

#[test]
fn partial_env_target_does_not_hide_broken_configuration() {
    let fixture = Fixture::new(true);
    fixture.config("active_context = [\n");
    for (key, value) in [("BB_WORKSPACE", "env-team"), ("BB_SLUG", "env-repo")] {
        let output = fixture
            .bbr()
            .env(key, value)
            .args(["open", "pipelines", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "config");
    }
}

#[test]
fn incomplete_target_without_git_still_reports_missing_half() {
    let fixture = Fixture::new(false);
    for (args, hint) in [
        (vec!["--workspace", "team"], "Pass --slug too"),
        (vec!["--slug", "repo"], "Pass --workspace too"),
    ] {
        let output = fixture
            .bbr()
            .args(["open", "pipelines", "--json"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "git");
        assert!(error["error"]["message"].as_str().unwrap().contains(hint));
    }
}

#[tokio::test]
async fn invalid_context_prevents_authenticated_api_requests() {
    use wiremock::MockServer;
    let server = MockServer::start().await;
    let fixture = Fixture::new(true);
    fixture.config("active_context = \"missing\"\n");
    for args in [
        vec!["repo", "info", "--json"],
        vec!["pr", "approve", "123", "--json"],
    ] {
        let output = fixture
            .bbr()
            .env("BITBUCKET_API_BASE", server.uri())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "config");
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "no credentials or mutations sent to a guessed target"
    );
}

#[tokio::test]
async fn url_syntax_in_any_identity_source_is_rejected_before_requests() {
    use wiremock::MockServer;
    let server = MockServer::start().await;
    let fixture = Fixture::new(false);
    type Env<'a> = Vec<(&'a str, &'a str)>;
    let cases: Vec<(Env, Vec<&str>)> = vec![
        (
            vec![("BB_WORKSPACE", "mine/../victim"), ("BB_SLUG", "repo")],
            vec![],
        ),
        (
            vec![],
            vec!["--workspace", "team", "--slug", "repo?role=admin"],
        ),
        (vec![], vec!["--workspace", "team", "--slug", "repo#frag"]),
        (vec![], vec!["--workspace", "a b", "--slug", "repo"]),
        (vec![], vec!["--workspace", "team", "--slug", "%2e%2e"]),
    ];
    for (env, args) in cases {
        let mut cmd = fixture.bbr();
        cmd.env("BITBUCKET_API_BASE", server.uri());
        for (k, v) in &env {
            cmd.env(k, v);
        }
        let output = cmd
            .args(["pr", "list", "--json"])
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(64), "{env:?} {args:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "usage");
    }
    // A hand-edited context is validated too.
    fixture.config(
        "active_context = \"work\"\n[contexts.work]\nworkspace = \"..\"\nslug = \"repo\"\n",
    );
    let output = fixture
        .bbr()
        .env("BITBUCKET_API_BASE", server.uri())
        .args(["repo", "info", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(64));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn context_create_and_config_set_refuse_unsafe_identities() {
    let fixture = Fixture::new(false);
    for args in [
        vec!["context", "create", "x", "--set-workspace", "ws/../other"],
        vec![
            "context",
            "create",
            "x",
            "--set-workspace",
            "ws",
            "--set-slug",
            "a?b",
        ],
        vec!["config", "set", "workspace", "evil/.."],
    ] {
        let output = fixture.bbr().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(64), "{args:?}");
    }
    assert!(!fixture.home.path().join("bbr/config.toml").exists());
    // Braced workspace UUIDs remain valid identities.
    fixture.assert_target(
        &[
            "--workspace",
            "{0e3a9a5c-1f2b-4c3d-9e8f-123456789abc}",
            "--slug",
            "repo",
        ],
        "{0e3a9a5c-1f2b-4c3d-9e8f-123456789abc}",
        "repo",
    );
}
