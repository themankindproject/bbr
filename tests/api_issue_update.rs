//! Issue updates must preserve fields the caller did not change.

use bbr::api::BitbucketClient;
use bbr::auth::Credentials;
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn client(base: &str) -> BitbucketClient {
    BitbucketClient::new(
        base,
        Credentials {
            username: "u".into(),
            secret: "t".into(),
        },
    )
    .unwrap()
}

#[tokio::test]
async fn update_issue_preserves_the_current_assignee() {
    let server = MockServer::start().await;
    let issue = json!({
        "id": 1,
        "title": "old",
        "content": {"raw": "body"},
        "state": "open",
        "kind": "bug",
        "priority": "major",
        "assignee": {"display_name": "Alice", "nickname": "alice"}
    });
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/issues/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue.clone()))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/repositories/ws/repo/issues/1"))
        .and(body_json(json!({
            "title": "new",
            "content": {"raw": "body"},
            "state": "open",
            "kind": "bug",
            "priority": "major",
            "assignee": {"nickname": "alice"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let updated = c
        .update_issue("ws", "repo", 1, Some("new"), None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(updated.title, "old");
}
