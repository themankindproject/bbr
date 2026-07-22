//! Integration tests for default reviewers, PR reviewers, and related APIs.

use serde_json::json;
use wiremock::matchers::{header, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use bbr::api::pr::{ReviewerRef, UpdatePrRequest};
use bbr::api::BitbucketClient;
use bbr::auth::{CredentialKind, Credentials};

async fn client(base: &str) -> BitbucketClient {
    let creds = Credentials {
        username: "u@example.com".into(),
        secret: "tok".into(),
        kind: CredentialKind::ApiToken,
    };
    BitbucketClient::from_credentials(base, creds).unwrap()
}

const AUTH_BASIC: &str = "Basic dUBleGFtcGxlLmNvbTp0b2s=";

fn sample_pr(id: u64, reviewers: serde_json::Value) -> serde_json::Value {
    json!({
        "id": id,
        "title": "Fix X",
        "state": "OPEN",
        "draft": false,
        "source": { "branch": { "name": "feat/x" },
                    "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
        "destination": { "branch": { "name": "main" },
                         "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
        "links": { "html": { "href": format!("https://bitbucket.org/sdadev/bvrm/pull-requests/{id}") } },
        "author": { "display_name": "Ash", "role": "AUTHOR", "approved": false },
        "reviewers": reviewers
    })
}

#[tokio::test]
async fn adds_and_removes_default_reviewer() {
    let server = MockServer::start().await;
    let encoded = "%7B11111111-1111-1111-1111-111111111111%7D";

    Mock::given(method("PUT"))
        .and(path(format!(
            "/repositories/sdadev/bvrm/default-reviewers/{encoded}"
        )))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("DELETE"))
        .and(path(format!(
            "/repositories/sdadev/bvrm/default-reviewers/{encoded}"
        )))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    c.add_default_reviewer("sdadev", "bvrm", "{11111111-1111-1111-1111-111111111111}")
        .await
        .unwrap();
    c.remove_default_reviewer("sdadev", "bvrm", "{11111111-1111-1111-1111-111111111111}")
        .await
        .unwrap();
}

#[tokio::test]
async fn resolve_user_uuid_looks_up_username() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/alice"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "display_name": "Alice",
            "uuid": "{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}",
            "nickname": "alice"
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let uuid = c.resolve_user_uuid("alice").await.unwrap();
    assert_eq!(uuid, "{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}");
}

#[tokio::test]
async fn adds_pr_reviewer() {
    let server = MockServer::start().await;
    let reviewer_uuid = "{bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb}";

    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/10"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_pr(10, json!([]))))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/10"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_pr(
            10,
            json!([{
                "display_name": "Bob",
                "uuid": reviewer_uuid,
                "role": "REVIEWER",
                "approved": false
            }]),
        )))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let added = c
        .add_pr_reviewer("sdadev", "bvrm", 10, reviewer_uuid)
        .await
        .unwrap();
    assert_eq!(added.reviewers.len(), 1);
    assert_eq!(added.reviewers[0].uuid.as_deref(), Some(reviewer_uuid));
}

#[tokio::test]
async fn removes_pr_reviewer() {
    let server = MockServer::start().await;
    let reviewer_uuid = "{bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb}";

    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/10"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_pr(
            10,
            json!([{
                "display_name": "Bob",
                "uuid": reviewer_uuid,
                "role": "REVIEWER",
                "approved": false
            }]),
        )))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/10"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_pr(10, json!([]))))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let removed = c
        .remove_pr_reviewer("sdadev", "bvrm", 10, reviewer_uuid)
        .await
        .unwrap();
    assert!(removed.reviewers.is_empty());
}

#[test]
fn update_pr_request_includes_reviewers() {
    let req = UpdatePrRequest {
        title: "T".into(),
        description: None,
        close_source_branch: None,
        reviewers: Some(vec![ReviewerRef { uuid: "{u}".into() }]),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["reviewers"][0]["uuid"], "{u}");
}

#[tokio::test]
async fn lists_default_reviewers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/repositories/sdadev/bvrm/default-reviewers.*"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "user": {
                    "display_name": "Alice",
                    "uuid": "{aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa}",
                    "nickname": "alice"
                }
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let list = c.list_default_reviewers("sdadev", "bvrm").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].user.as_ref().unwrap().display_name, "Alice");
}
