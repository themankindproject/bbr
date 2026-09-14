//! Abort and rebase failure handling; local Git fixtures and mock HTTP only.

use bbr::stack::{StackConfig, StackDef, StackPr};
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};
use tempfile::{tempdir, TempDir};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

struct Fixture {
    home: TempDir,
    repo: PathBuf,
}
impl Fixture {
    fn new(count: usize) -> Self {
        let home = tempdir().unwrap();
        let repo = home.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::write(home.path().join("gitconfig"), "").unwrap();
        let f = Self { home, repo };
        f.git(&["init", "--quiet", "-b", "main"]);
        f.commit("initial");
        fs::write(f.repo.join(".git/info/exclude"), ".bbr/\n").unwrap();
        let prs = (0..count)
            .map(|i| {
                let branch = format!("feature-{i}");
                f.git(&["branch", &branch]);
                StackPr {
                    branch,
                    pr_id: Some(101 + i as u64),
                    parent_branch: "main".into(),
                }
            })
            .collect();
        f.save(&StackConfig {
            active: Some("work".into()),
            stacks: vec![
                StackDef {
                    name: "work".into(),
                    base_branch: "main".into(),
                    prs,
                },
                StackDef {
                    name: "sibling".into(),
                    base_branch: "main".into(),
                    prs: vec![],
                },
            ],
        });
        f
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
            .timeout(Duration::from_secs(10));
    }
    fn git(&self, args: &[&str]) {
        let mut c = assert_cmd::Command::new("git");
        self.isolate(&mut c);
        c.args(args).assert().success();
    }
    fn commit(&self, msg: &str) {
        self.git(&[
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
            msg,
        ]);
    }
    fn path(&self) -> PathBuf {
        self.repo.join(".bbr/stack.toml")
    }
    fn config(&self) -> StackConfig {
        toml::from_str(&fs::read_to_string(self.path()).unwrap()).unwrap()
    }
    fn save(&self, cfg: &StackConfig) {
        fs::create_dir_all(self.repo.join(".bbr")).unwrap();
        fs::write(self.path(), toml::to_string_pretty(cfg).unwrap()).unwrap();
    }
    fn run(&self, server: &MockServer, args: &[&str]) -> std::process::Output {
        let mut c = assert_cmd::Command::cargo_bin("bbr").unwrap();
        self.isolate(&mut c);
        c.env("BITBUCKET_USERNAME", "test")
            .env("BITBUCKET_TOKEN", "fake-token")
            .env("BITBUCKET_API_BASE", server.uri())
            .env("BB_WORKSPACE", "ws")
            .env("BB_SLUG", "repo")
            .env("BBR_NO_INTERACTIVE", "1")
            .env("NO_COLOR", "1")
            .args(["pr", "stack"])
            .args(args)
            .arg("--json")
            .output()
            .unwrap()
    }
    fn abort(&self, s: &MockServer) -> std::process::Output {
        self.run(s, &["abort", "--yes"])
    }
    fn assert_branch(&self, b: &str) {
        self.git(&["show-ref", "--verify", &format!("refs/heads/{b}")]);
    }
}
fn pr(id: u64, state: &str) -> Value {
    json!({"id":id,"state":state,"source":{"branch":{"name":format!("feature-{}",id-101)},"repository":{"full_name":"ws/repo","type":"repository"}},"destination":{"branch":{"name":"main"}}})
}
async fn get(s: &MockServer, id: u64, state: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/repositories/ws/repo/pullrequests/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr(id, state)))
        .expect(1)
        .mount(s)
        .await;
}
async fn decline(s: &MockServer, id: u64, status: u16) {
    Mock::given(method("POST"))
        .and(path(format!(
            "/repositories/ws/repo/pullrequests/{id}/decline"
        )))
        .respond_with(
            ResponseTemplate::new(status).set_body_json(if status == 200 {
                pr(id, "DECLINED")
            } else {
                json!({"error":{"message":"denied"}})
            }),
        )
        .expect(1)
        .mount(s)
        .await;
}
async fn delete(s: &MockServer, branch: &str, status: u16) {
    Mock::given(method("DELETE"))
        .and(path(format!(
            "/repositories/ws/repo/refs/branches/{branch}"
        )))
        .respond_with(ResponseTemplate::new(status))
        .expect(1)
        .mount(s)
        .await;
}

#[tokio::test]
async fn decline_failure_keeps_branches_and_state_and_stops() {
    let s = MockServer::start().await;
    let f = Fixture::new(2);
    let before = f.config();
    get(&s, 101, "OPEN").await;
    decline(&s, 101, 403).await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(2));
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["declined"], json!([]));
    assert_eq!(receipt["branches_deleted"], json!([]));
    assert_eq!(f.config(), before);
    f.assert_branch("feature-0");
    f.assert_branch("feature-1");
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn partial_abort_checkpoints_finished_entries_before_next_decline() {
    let s = MockServer::start().await;
    let f = Fixture::new(2);
    get(&s, 101, "OPEN").await;
    decline(&s, 101, 200).await;
    delete(&s, "feature-0", 204).await;
    let file = f.path();
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests/102"))
        .respond_with(move |_: &Request| {
            let cfg: StackConfig = toml::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(cfg.find_stack("work").unwrap().prs[0].pr_id, Some(102));
            ResponseTemplate::new(200).set_body_json(pr(102, "OPEN"))
        })
        .expect(1)
        .mount(&s)
        .await;
    decline(&s, 102, 403).await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(2));
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["declined"], json!([101]));
    assert_eq!(f.config().find_stack("work").unwrap().prs.len(), 1);
    assert!(f.config().find_stack("sibling").is_some());
    f.assert_branch("feature-1");
}

#[tokio::test]
async fn already_declined_and_missing_branches_reconcile_on_retry() {
    let s = MockServer::start().await;
    let f = Fixture::new(1);
    f.git(&["branch", "-d", "feature-0"]);
    get(&s, 101, "DECLINED").await;
    delete(&s, "feature-0", 404).await;
    let out = f.abort(&s);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(f.config().find_stack("work").is_none());
    assert!(f.config().find_stack("sibling").is_some());
    assert_eq!(f.config().active.as_deref(), Some("sibling"));
    assert!(s
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|r| r.method.as_str() != "POST"));
}

#[tokio::test]
async fn remote_delete_failure_does_not_delete_local_branch_or_drop_entry() {
    let s = MockServer::start().await;
    let f = Fixture::new(1);
    let before = f.config();
    get(&s, 101, "DECLINED").await;
    delete(&s, "feature-0", 403).await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(f.config(), before);
    f.assert_branch("feature-0");
}

#[tokio::test]
async fn unmerged_local_commits_are_not_force_deleted_and_retry_can_finish() {
    let s = MockServer::start().await;
    let f = Fixture::new(1);
    f.git(&["switch", "feature-0"]);
    f.commit("local-only");
    f.git(&["switch", "main"]);
    get(&s, 101, "DECLINED").await;
    delete(&s, "feature-0", 204).await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(1));
    f.assert_branch("feature-0");
    assert_eq!(f.config().find_stack("work").unwrap().prs.len(), 1);
    s.verify().await;
    s.reset().await;
    // Simulate an explicit user decision to retain the commit elsewhere and remove the branch.
    f.git(&["branch", "backup", "feature-0"]);
    f.git(&["branch", "-D", "feature-0"]);
    get(&s, 101, "DECLINED").await;
    delete(&s, "feature-0", 404).await;
    assert!(f.abort(&s).status.success());
    f.assert_branch("backup");
}

#[tokio::test]
async fn unconfirmed_or_mismatched_prs_never_delete_branches() {
    for response in [
        pr(101, "MERGED"),
        {
            let mut p = pr(101, "OPEN");
            p["source"]["branch"]["name"] = json!("different");
            p
        },
        {
            let mut p = pr(101, "OPEN");
            p["source"]["repository"]["full_name"] = json!("fork/repo");
            p
        },
    ] {
        let s = MockServer::start().await;
        let f = Fixture::new(1);
        let before = f.config();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&s)
            .await;
        let out = f.abort(&s);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(f.config(), before);
        f.assert_branch("feature-0");
        assert_eq!(s.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn successful_http_status_without_declined_state_does_not_trigger_deletion() {
    let s = MockServer::start().await;
    let f = Fixture::new(1);
    get(&s, 101, "OPEN").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr(101, "OPEN")))
        .expect(1)
        .mount(&s)
        .await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(1));
    f.assert_branch("feature-0");
    assert!(f.config().find_stack("work").is_some());
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn unsafe_stack_entries_are_rejected_before_any_remote_work() {
    for case in ["base", "current", "invalid", "missing-id", "duplicate"] {
        let s = MockServer::start().await;
        let f = Fixture::new(1);
        let mut cfg = f.config();
        match case {
            "base" => cfg.stacks[0].prs[0].branch = "main".into(),
            "current" => f.git(&["switch", "feature-0"]),
            "invalid" => cfg.stacks[0].prs[0].branch = "../bad".into(),
            "missing-id" => cfg.stacks[0].prs[0].pr_id = None,
            _ => {
                let p = cfg.stacks[0].prs[0].clone();
                cfg.stacks[0].prs.push(p);
            }
        }
        f.save(&cfg);
        let out = f.abort(&s);
        assert!(!out.status.success(), "{case}");
        assert_eq!(f.config(), cfg);
        assert!(s.received_requests().await.unwrap().is_empty());
        f.assert_branch("feature-0");
    }
}

#[tokio::test]
async fn state_changed_after_decline_stops_before_branch_deletion() {
    let s = MockServer::start().await;
    let f = Fixture::new(1);
    get(&s, 101, "OPEN").await;
    let mut changed = f.config();
    changed.active = Some("sibling".into());
    let text = toml::to_string(&changed).unwrap();
    let file = f.path();
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            fs::write(&file, &text).unwrap();
            ResponseTemplate::new(200).set_body_json(pr(101, "DECLINED"))
        })
        .expect(1)
        .mount(&s)
        .await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(f.config(), changed);
    f.assert_branch("feature-0");
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn checkpoint_failure_preserves_unfinished_state_and_stops_next_pr() {
    let s = MockServer::start().await;
    let f = Fixture::new(2);
    get(&s, 101, "DECLINED").await;
    let file = f.path();
    Mock::given(method("DELETE"))
        .and(path("/repositories/ws/repo/refs/branches/feature-0"))
        .respond_with(move |_: &Request| {
            fs::remove_file(&file).unwrap();
            fs::create_dir(&file).unwrap();
            fs::write(file.join("keep"), "preserved").unwrap();
            ResponseTemplate::new(204)
        })
        .expect(1)
        .mount(&s)
        .await;
    let out = f.abort(&s);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("stack"));
    f.assert_branch("feature-1");
    assert_eq!(s.received_requests().await.unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(f.path().join("keep")).unwrap(),
        "preserved"
    );
}

#[tokio::test]
async fn rebase_failure_returns_nonzero_with_partial_step_receipt() {
    let s = MockServer::start().await;
    let f = Fixture::new(2);
    let mut cfg = f.config();
    cfg.stacks[0].prs[0].parent_branch = "missing-parent".into();
    f.save(&cfg);
    let out = f.run(&s, &["rebase"]);
    assert_eq!(out.status.code(), Some(1));
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["steps"].as_array().unwrap().len(), 1);
    assert_ne!(receipt["steps"][0]["status"], "ok");
    assert_eq!(f.config(), cfg);
    assert!(!f.repo.join(".git/rebase-merge").exists());
}

#[tokio::test]
async fn rebase_push_failure_is_not_reported_as_success() {
    let s = MockServer::start().await;
    let f = Fixture::new(1);
    f.git(&[
        "config",
        "remote.origin.url",
        f.home.path().join("missing-remote.git").to_str().unwrap(),
    ]);
    let out = f.run(&s, &["rebase", "--push"]);
    assert_eq!(out.status.code(), Some(1));
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["steps"][0]["status"], "error");
}
