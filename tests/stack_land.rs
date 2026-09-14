//! Landing uses local Git and a mock Bitbucket API, never real PRs.

use bbr::stack::{StackConfig, StackDef, StackPr};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::{tempdir, TempDir};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

struct Fixture {
    home: TempDir,
    repo: PathBuf,
}
impl Fixture {
    fn new(ids: &[Option<u64>], sibling: bool) -> Self {
        let home = tempdir().unwrap();
        let repo = home.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::write(home.path().join("gitconfig"), "").unwrap();
        let f = Self { home, repo };
        f.git(&["init", "--quiet", "-b", "main"]);
        f.git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "initial",
        ]);
        fs::write(f.repo.join(".git/info/exclude"), ".bbr/\n").unwrap();
        let mut prs = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let branch = format!("feature-{index}");
            f.git(&["branch", &branch]);
            prs.push(StackPr {
                branch,
                pr_id: *id,
                parent_branch: "main".into(),
            });
        }
        let mut stacks = vec![StackDef {
            name: "work".into(),
            base_branch: "main".into(),
            prs,
        }];
        if sibling {
            stacks.push(StackDef {
                name: "other".into(),
                base_branch: "main".into(),
                prs: vec![],
            });
        }
        let cfg = StackConfig {
            active: Some("work".into()),
            stacks,
        };
        fs::create_dir(f.repo.join(".bbr")).unwrap();
        fs::write(f.path(), toml::to_string_pretty(&cfg).unwrap()).unwrap();
        f
    }
    fn isolate(&self, c: &mut assert_cmd::Command) {
        c.current_dir(&self.repo)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path())
            .env("APPDATA", self.home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.home.path().join("gitconfig"))
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_CONFIG_COUNT")
            .timeout(Duration::from_secs(10));
    }
    fn git(&self, args: &[&str]) {
        let mut cmd = assert_cmd::Command::new("git");
        self.isolate(&mut cmd);
        cmd.args(args).assert().success();
    }
    fn path(&self) -> PathBuf {
        self.repo.join(".bbr/stack.toml")
    }
    fn config(&self) -> StackConfig {
        toml::from_str(&fs::read_to_string(self.path()).unwrap()).unwrap()
    }
    fn land(&self, server: &MockServer) -> std::process::Output {
        let mut cmd = assert_cmd::Command::cargo_bin("bbr").unwrap();
        self.isolate(&mut cmd);
        cmd.env("BITBUCKET_USERNAME", "test")
            .env("BITBUCKET_TOKEN", "fake-token")
            .env("BITBUCKET_API_BASE", server.uri())
            .env("BB_WORKSPACE", "ws")
            .env("BB_SLUG", "repo")
            .env("BBR_NO_INTERACTIVE", "1")
            .env("NO_COLOR", "1")
            .args(["pr", "stack", "land", "--yes", "--json"])
            .output()
            .unwrap()
    }
}
fn pr(id: u64, state: &str) -> Value {
    json!({"id":id,"state":state,"source":{"branch":{"name":"feature"}},"destination":{"branch":{"name":"main"}}})
}
async fn get_pr(server: &MockServer, id: u64, state: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/repositories/ws/repo/pullrequests/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr(id, state)))
        .mount(server)
        .await;
}
async fn merge(server: &MockServer, id: u64, status: u16) {
    let body = if status == 200 {
        pr(id, "MERGED")
    } else {
        json!({"error":{"message":"merge denied"}})
    };
    Mock::given(method("POST"))
        .and(path(format!(
            "/repositories/ws/repo/pullrequests/{id}/merge"
        )))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn partial_failure_exits_nonzero_and_checkpoints_each_merge() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101), Some(102), Some(103)], true);
    for id in [101, 102, 103] {
        get_pr(&server, id, "OPEN").await;
    }
    merge(&server, 101, 200).await;
    let snapshot = Arc::new(Mutex::new(None));
    let observed = snapshot.clone();
    let file = f.path();
    Mock::given(method("POST"))
        .and(path("/repositories/ws/repo/pullrequests/102/merge"))
        .respond_with(move |_: &Request| {
            *observed.lock().unwrap() = Some(fs::read_to_string(&file).unwrap());
            ResponseTemplate::new(403).set_body_json(json!({"error":{"message":"merge denied"}}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let out = f.land(&server);
    assert_eq!(
        out.status.code(),
        Some(2),
        "API failure must retain auth exit code"
    );
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["merged"], json!([101]));
    assert_eq!(receipt["failed"][0]["pr_id"], 102);
    let before_next: StackConfig =
        toml::from_str(snapshot.lock().unwrap().as_ref().unwrap()).unwrap();
    assert_eq!(
        before_next.find_stack("work").unwrap().prs[0].pr_id,
        Some(102)
    );
    let saved = f.config();
    assert_eq!(saved.find_stack("work").unwrap().prs.len(), 2);
    assert!(saved.find_stack("other").is_some());
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| !r.url.path().contains("103")));
}

#[tokio::test]
async fn missing_zero_or_duplicate_ids_fail_before_remote_work() {
    for ids in [
        vec![Some(101), None],
        vec![Some(0)],
        vec![Some(101), Some(101)],
    ] {
        let f = Fixture::new(&ids, false);
        let server = MockServer::start().await;
        let original = fs::read(f.path()).unwrap();
        let out = f.land(&server);
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(server.received_requests().await.unwrap().is_empty());
        assert_eq!(fs::read(f.path()).unwrap(), original);
    }
}

#[tokio::test]
async fn checkpoint_failure_stops_before_next_merge_and_preserves_local_branch() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101), Some(102)], false);
    get_pr(&server, 101, "OPEN").await;
    get_pr(&server, 102, "OPEN").await;
    let file = f.path();
    Mock::given(method("POST"))
        .and(path("/repositories/ws/repo/pullrequests/101/merge"))
        .respond_with(move |_: &Request| {
            fs::remove_file(&file).unwrap();
            fs::create_dir(&file).unwrap();
            fs::write(file.join("keep"), "unchanged").unwrap();
            ResponseTemplate::new(200).set_body_json(pr(101, "MERGED"))
        })
        .expect(1)
        .mount(&server)
        .await;
    let out = f.land(&server);
    assert_eq!(out.status.code(), Some(1));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("101") && error.contains("checkpoint"));
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| !r.url.path().contains("102")));
    f.git(&["show-ref", "--verify", "refs/heads/feature-0"]);
    assert_eq!(
        fs::read_to_string(f.path().join("keep")).unwrap(),
        "unchanged"
    );
}

#[tokio::test]
async fn already_merged_pr_is_checkpointed_without_repeating_the_post() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101)], true);
    get_pr(&server, 101, "MERGED").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400))
        .expect(0)
        .mount(&server)
        .await;
    let out = f.land(&server);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["merged"],
        json!([101])
    );
    assert!(f.config().find_stack("work").is_none());
    assert!(f.config().find_stack("other").is_some());
}

#[tokio::test]
async fn declined_pr_is_not_merged_or_removed_from_stack() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101)], false);
    get_pr(&server, 101, "DECLINED").await;
    let original = fs::read(f.path()).unwrap();
    let out = f.land(&server);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read(f.path()).unwrap(), original);
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn unconfirmed_merge_response_does_not_delete_branch_or_checkpoint_success() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101)], false);
    get_pr(&server, 101, "OPEN").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr(101, "OPEN")))
        .expect(1)
        .mount(&server)
        .await;
    let out = f.land(&server);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(f.config().find_stack("work").unwrap().prs.len(), 1);
    f.git(&["show-ref", "--verify", "refs/heads/feature-0"]);
}

#[tokio::test]
async fn successful_last_merge_persists_empty_stack_config() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101)], false);
    get_pr(&server, 101, "OPEN").await;
    merge(&server, 101, 200).await;
    let out = f.land(&server);
    assert!(out.status.success());
    let config = f.config();
    assert!(config.stacks.is_empty());
    assert!(config.active.is_none());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["failed"],
        json!([])
    );
}

#[tokio::test]
async fn local_unmerged_commits_are_retained_after_remote_merge() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101)], false);
    f.git(&["switch", "feature-0"]);
    f.git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "local-only commit",
    ]);
    f.git(&["switch", "main"]);
    get_pr(&server, 101, "OPEN").await;
    merge(&server, 101, 200).await;
    let out = f.land(&server);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("retained"));
    f.git(&["show-ref", "--verify", "refs/heads/feature-0"]);
}

#[tokio::test]
async fn configuration_edit_during_preflight_stops_before_merge() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101)], true);
    let file = f.path();
    let mut changed = f.config();
    changed.active = Some("other".into());
    let changed_text = toml::to_string_pretty(&changed).unwrap();
    let new_text = changed_text.clone();
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests/101"))
        .respond_with(move |_: &Request| {
            fs::write(&file, &new_text).unwrap();
            ResponseTemplate::new(200).set_body_json(pr(101, "OPEN"))
        })
        .expect(1)
        .mount(&server)
        .await;
    let out = f.land(&server);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_to_string(f.path()).unwrap(), changed_text);
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.method.as_str() == "GET"));
}

#[tokio::test]
async fn retry_after_partial_merge_only_processes_remaining_prs() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101), Some(102)], true);
    get_pr(&server, 101, "OPEN").await;
    get_pr(&server, 102, "OPEN").await;
    merge(&server, 101, 200).await;
    merge(&server, 102, 403).await;
    assert_eq!(f.land(&server).status.code(), Some(2));
    server.verify().await;
    server.reset().await;
    get_pr(&server, 102, "OPEN").await;
    merge(&server, 102, 200).await;
    let out = f.land(&server);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["merged"],
        json!([102])
    );
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| !r.url.path().contains("101")));
    assert!(f.config().find_stack("work").is_none());
    assert!(f.config().find_stack("other").is_some());
}

#[tokio::test]
async fn concurrent_state_change_is_not_overwritten_after_merge() {
    let server = MockServer::start().await;
    let f = Fixture::new(&[Some(101), Some(102)], true);
    get_pr(&server, 101, "OPEN").await;
    let file = f.path();
    let mut changed = f.config();
    changed.active = Some("other".into());
    let changed_text = toml::to_string_pretty(&changed).unwrap();
    let write_text = changed_text.clone();
    Mock::given(method("POST"))
        .and(path("/repositories/ws/repo/pullrequests/101/merge"))
        .respond_with(move |_: &Request| {
            fs::write(&file, &write_text).unwrap();
            ResponseTemplate::new(200).set_body_json(pr(101, "MERGED"))
        })
        .expect(1)
        .mount(&server)
        .await;
    let out = f.land(&server);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_to_string(f.path()).unwrap(), changed_text);
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| !r.url.path().contains("102")));
}
