//! `bbr batch` exit status and plan/apply re-validation (mock HTTP only).

use serde_json::{json, Value};
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn approved_pr(id: u64) -> Value {
    json!({
        "id": id,
        "state": "OPEN",
        "title": format!("PR {id}"),
        "draft": false,
        "source": {"branch": {"name": format!("feature-{id}")}},
        "destination": {"branch": {"name": "main"}},
        "reviewers": [{"display_name": "R", "uuid": "{r}", "approved": true, "state": "approved"}],
        "participants": []
    })
}

fn bbr(server: &MockServer, args: &[&str]) -> std::process::Output {
    let home = TempDir::new().unwrap();
    assert_cmd::Command::cargo_bin("bbr")
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
        .env("BBR_NO_INTERACTIVE", "1")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .args(args)
        .timeout(Duration::from_secs(15))
        .output()
        .unwrap()
}

async fn mount_list(server: &MockServer, prs: Vec<Value>) {
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "values": prs })))
        .mount(server)
        .await;
}

async fn mount_get(server: &MockServer, pr: Value) {
    let id = pr["id"].as_u64().unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/repositories/ws/repo/pullrequests/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr))
        .mount(server)
        .await;
}

fn merge_posts(requests: &[wiremock::Request]) -> Vec<String> {
    requests
        .iter()
        .filter(|r| r.method.as_str() == "POST")
        .map(|r| r.url.path().to_string())
        .collect()
}

#[tokio::test]
async fn failed_merges_exit_nonzero_but_keep_the_receipt() {
    let server = MockServer::start().await;
    mount_list(&server, vec![approved_pr(5), approved_pr(6)]).await;
    mount_get(&server, approved_pr(5)).await;
    mount_get(&server, approved_pr(6)).await;
    Mock::given(method("POST"))
        .and(path("/repositories/ws/repo/pullrequests/5/merge"))
        .respond_with(
            ResponseTemplate::new(409).set_body_json(json!({"error": {"message": "conflict"}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repositories/ws/repo/pullrequests/6/merge"))
        .respond_with(ResponseTemplate::new(200).set_body_json(approved_pr(6)))
        .mount(&server)
        .await;

    let out = bbr(&server, &["batch", "merge-approved", "--yes", "--json"]);
    assert_eq!(out.status.code(), Some(1), "a failed merge must not exit 0");
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["failed"][0]["id"], "5");
    assert_eq!(receipt["succeeded"][0]["id"], "6");
    let err: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(err["error"]["message"]
        .as_str()
        .unwrap()
        .contains("1 of 2 merges failed"));
}

#[tokio::test]
async fn pr_that_lost_its_approval_after_planning_is_not_merged() {
    let server = MockServer::start().await;
    mount_list(
        &server,
        vec![approved_pr(5), approved_pr(6), approved_pr(7)],
    )
    .await;
    // #5: approval withdrawn; #6: marked draft; #7 still qualifies.
    let mut withdrawn = approved_pr(5);
    withdrawn["reviewers"][0]["approved"] = json!(false);
    withdrawn["reviewers"][0]["state"] = json!(null);
    mount_get(&server, withdrawn).await;
    let mut draft = approved_pr(6);
    draft["draft"] = json!(true);
    mount_get(&server, draft).await;
    mount_get(&server, approved_pr(7)).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(approved_pr(7)))
        .mount(&server)
        .await;

    let out = bbr(&server, &["batch", "merge-approved", "--yes", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    let reasons: Vec<String> = receipt["failed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| format!("{}: {}", f["id"], f["error"]))
        .collect();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("\"5\"") && r.contains("approvals dropped")),
        "{reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("\"6\"") && r.contains("draft")),
        "{reasons:?}"
    );
    assert_eq!(
        merge_posts(&server.received_requests().await.unwrap()),
        vec!["/repositories/ws/repo/pullrequests/7/merge"]
    );
}

#[tokio::test]
async fn draft_prs_are_excluded_from_the_plan() {
    let server = MockServer::start().await;
    let mut draft = approved_pr(8);
    draft["draft"] = json!(true);
    mount_list(&server, vec![draft]).await;
    let out = bbr(&server, &["batch", "merge-approved", "--dry-run", "--json"]);
    assert!(out.status.success());
    let plan: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(plan["action_count"], 0);
    let list = &server.received_requests().await.unwrap()[0];
    assert!(
        list.url
            .query()
            .unwrap_or_default()
            .contains("values.draft"),
        "the draft flag must survive the fields projection"
    );
}

#[tokio::test]
async fn all_successful_batch_still_exits_zero() {
    let server = MockServer::start().await;
    mount_list(&server, vec![approved_pr(9)]).await;
    mount_get(&server, approved_pr(9)).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(approved_pr(9)))
        .mount(&server)
        .await;
    let out = bbr(&server, &["batch", "merge-approved", "--yes", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
