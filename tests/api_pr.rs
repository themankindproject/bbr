//! Integration tests for PR API endpoints against a mock server.

use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use bbr::api::pr::{PrState, PullRequest};
use bbr::api::BitbucketClient;
use bbr::auth::{CredentialKind, Credentials};

async fn client(base: &str) -> BitbucketClient {
    let creds = Credentials {
        username: "u@example.com".into(),
        secret: "tok".into(),
        kind: CredentialKind::ApiToken,
    };
    BitbucketClient::new(base, creds).unwrap()
}

const AUTH_BASIC: &str = "Basic dUBleGFtcGxlLmNvbTp0b2s=";

#[tokio::test]
async fn lists_open_prs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 1, "page": 1, "pagelen": 25,
            "values": [{
                "id": 467,
                "title": "Fix X",
                "state": "OPEN",
                "source": { "branch": { "name": "feat/x" },
                            "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
                "destination": { "branch": { "name": "main" },
                                 "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
                "links": { "html": { "href": "https://bitbucket.org/sdadev/bvrm/pull-requests/467" } },
                "author": { "display_name": "Ash", "role": "AUTHOR", "approved": false }
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let values = c
        .list_prs(
            "sdadev",
            "bvrm",
            PrState::Open,
            25,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    let pr: &PullRequest = &values[0];
    assert_eq!(pr.id, 467);
    assert_eq!(pr.state, "OPEN");
    assert_eq!(pr.source.branch.as_ref().unwrap().name, "feat/x");
    assert_eq!(pr.source_branch(), "feat/x");
    assert_eq!(pr.destination_branch(), "main");
    assert_eq!(
        pr.web_url(),
        Some("https://bitbucket.org/sdadev/bvrm/pull-requests/467")
    );
}

#[tokio::test]
async fn auth_failure_maps_to_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/1"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .get_pr("sdadev", "bvrm", 1)
        .await
        .expect_err("should be an error");
    assert_eq!(err.exit_code(), bbr::error::ExitCode::Auth);
}

#[tokio::test]
async fn not_found_maps_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/9999"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .get_pr("sdadev", "bvrm", 9999)
        .await
        .expect_err("should be 404");
    assert_eq!(err.exit_code(), bbr::error::ExitCode::NotFound);
}

#[tokio::test]
async fn lists_pr_review_resources() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/comments"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "id": 10,
                "content": { "raw": "Looks good" },
                "user": { "display_name": "Ash" },
                "created_on": "2026-01-01T00:00:00Z"
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/tasks"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "id": 20,
                "state": "UNRESOLVED",
                "content": { "raw": "Update docs" },
                "assignee": { "display_name": "Sam" }
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/commits"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "hash": "abc123",
                "message": "Fix bug\n\nBody",
                "author": { "raw": "Dev <dev@example.com>" }
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/statuses"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "state": "SUCCESSFUL",
                "key": "lint",
                "name": "Lint",
                "url": "https://ci.example/lint"
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/conflicts"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "path": "src/lib.rs",
                "conflict_type": "content"
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let comments = c.pr_comments("sdadev", "bvrm", 467, 50).await.unwrap();
    assert_eq!(comments[0].content.as_ref().unwrap().raw, "Looks good");
    let tasks = c.pr_tasks("sdadev", "bvrm", 467, 50).await.unwrap();
    assert_eq!(tasks[0].state, "UNRESOLVED");
    let commits = c.pr_commits("sdadev", "bvrm", 467, 50).await.unwrap();
    assert_eq!(commits[0].hash, "abc123");
    let statuses = c.pr_statuses("sdadev", "bvrm", 467, 50).await.unwrap();
    assert_eq!(statuses[0].key, "lint");
    let conflicts = c.pr_conflicts("sdadev", "bvrm", 467, 50).await.unwrap();
    assert_eq!(conflicts[0].path, "src/lib.rs");
}

#[tokio::test]
async fn toggles_pr_change_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/repositories/sdadev/bvrm/pullrequests/467/request-changes",
        ))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(
            "/repositories/sdadev/bvrm/pullrequests/467/request-changes",
        ))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(204).set_body_string(""))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    c.request_pr_changes("sdadev", "bvrm", 467).await.unwrap();
    c.unrequest_pr_changes("sdadev", "bvrm", 467).await.unwrap();
}
