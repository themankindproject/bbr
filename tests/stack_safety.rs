//! Stack persistence and active-selection safety using disposable repositories.

use bbr::stack::{StackConfig, StackDef};
use serde_json::Value;
use std::{fs, path::PathBuf, time::Duration};
use tempfile::{tempdir, TempDir};

struct Fixture {
    home: TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempdir().unwrap();
        let repo = home.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::write(home.path().join("gitconfig"), "").unwrap();
        let fixture = Self { home, repo };
        let mut git = assert_cmd::Command::new("git");
        fixture.isolate(&mut git);
        git.args(["init", "--quiet"]).assert().success();
        fixture
    }

    fn isolate(&self, cmd: &mut assert_cmd::Command) {
        cmd.current_dir(&self.repo)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path())
            .env("APPDATA", self.home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.home.path().join("gitconfig"))
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_CONFIG_COUNT")
            .timeout(Duration::from_secs(5));
    }

    fn command(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("bbr").unwrap();
        self.isolate(&mut cmd);
        cmd.env("BITBUCKET_USERNAME", "test")
            .env("BITBUCKET_TOKEN", "fake-token")
            .env("BITBUCKET_API_BASE", "http://127.0.0.1:9")
            .env("BB_WORKSPACE", "ws")
            .env("BB_SLUG", "repo")
            .env("BBR_NO_INTERACTIVE", "1")
            .env("NO_COLOR", "1");
        cmd
    }

    fn path(&self) -> PathBuf {
        self.repo.join(".bbr/stack.toml")
    }

    fn write(&self, text: &str) {
        fs::create_dir_all(self.repo.join(".bbr")).unwrap();
        fs::write(self.path(), text).unwrap();
    }
}

const VALID: &str =
    "active = \"first\"\n[[stacks]]\nname = \"first\"\nbase_branch = \"main\"\nprs = []\n";

#[test]
fn init_refuses_corrupt_state_without_overwriting_it() {
    let fixture = Fixture::new();
    let malformed = "active = \"first\"\n[[stacks]\nname = \"unfinished\"\n";
    fixture.write(malformed);
    let output = fixture
        .command()
        .args(["pr", "stack", "init", "second", "--base", "main", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("stack config"));
    assert_eq!(fs::read_to_string(fixture.path()).unwrap(), malformed);
}

#[cfg(unix)]
#[test]
fn stack_save_replaces_instead_of_truncating_live_state() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.write(VALID);
    let mut existing_reader = fs::File::open(fixture.path()).unwrap();
    fixture
        .command()
        .args(["pr", "stack", "init", "second", "--base", "main", "--json"])
        .assert()
        .success();
    let mut before = String::new();
    existing_reader.read_to_string(&mut before).unwrap();
    assert_eq!(before, VALID, "live inode was truncated");
    let saved: StackConfig = toml::from_str(&fs::read_to_string(fixture.path()).unwrap()).unwrap();
    assert_eq!(saved.stacks.len(), 2);
    assert_eq!(saved.active.as_deref(), Some("second"));
    assert_eq!(saved.stacks[0].name, "first");
    assert_eq!(
        fs::metadata(fixture.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(fixture.repo.join(".bbr")).unwrap().count(), 1);
}

#[test]
fn explicit_stale_selection_cannot_choose_an_unrelated_stack() {
    let mut config = StackConfig {
        active: Some("deleted".into()),
        stacks: vec![StackDef {
            name: "first".into(),
            base_branch: "main".into(),
            prs: vec![],
        }],
    };
    let error = config.active_stack().unwrap_err();
    assert!(error.to_string().contains("deleted"));
    assert!(error.to_string().contains("bbr pr stack use"));
    assert!(config.active_stack_mut().is_err());
    config.set_active("first").unwrap();
    assert_eq!(config.active_stack().unwrap().name, "first");
}

#[test]
fn legacy_missing_selection_still_uses_first_stack() {
    let config: StackConfig = toml::from_str(&VALID.replace("active = \"first\"\n", "")).unwrap();
    assert_eq!(config.active_stack().unwrap().name, "first");
}

#[tokio::test]
async fn stale_selection_blocks_abort_before_any_remote_operation() {
    let server = wiremock::MockServer::start().await;
    let fixture = Fixture::new();
    let original = VALID.replace("active = \"first\"", "active = \"deleted\"");
    fixture.write(&original);
    let output = fixture
        .command()
        .env("BITBUCKET_API_BASE", server.uri())
        .args(["pr", "stack", "abort", "--yes", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(fs::read_to_string(fixture.path()).unwrap(), original);
    fixture
        .command()
        .args(["pr", "stack", "use", "first", "--json"])
        .assert()
        .success();
    let repaired: StackConfig =
        toml::from_str(&fs::read_to_string(fixture.path()).unwrap()).unwrap();
    assert_eq!(repaired.active_stack().unwrap().name, "first");
}

#[test]
fn init_and_use_from_subdirectory_preserve_repo_root_state() {
    let fixture = Fixture::new();
    fixture.write(VALID);
    let nested = fixture.repo.join("nested");
    fs::create_dir(&nested).unwrap();
    fixture
        .command()
        .current_dir(&nested)
        .args(["pr", "stack", "init", "second", "--base", "main", "--json"])
        .assert()
        .success();
    fixture
        .command()
        .current_dir(&nested)
        .args(["pr", "stack", "use", "first", "--json"])
        .assert()
        .success();
    assert!(!nested.join(".bbr").exists());
    let saved: StackConfig = toml::from_str(&fs::read_to_string(fixture.path()).unwrap()).unwrap();
    assert_eq!(saved.stacks.len(), 2);
    assert_eq!(saved.active.as_deref(), Some("first"));
}

#[test]
fn invalid_stack_parent_reports_creation_error_without_altering_file() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join(".bbr"), "keep").unwrap();
    let output = fixture
        .command()
        .args(["pr", "stack", "init", "first", "--base", "main", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("stack config"));
    assert_eq!(
        fs::read_to_string(fixture.repo.join(".bbr")).unwrap(),
        "keep"
    );
}

#[test]
fn unreadable_stack_path_is_not_reinitialized() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path()).unwrap();
    fs::write(fixture.path().join("keep"), "unchanged").unwrap();
    let output = fixture
        .command()
        .args(["pr", "stack", "init", "first", "--base", "main", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("read stack config"));
    assert_eq!(
        fs::read_to_string(fixture.path().join("keep")).unwrap(),
        "unchanged"
    );
}
