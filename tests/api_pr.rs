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
    BitbucketClient::from_credentials(base, creds).unwrap()
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

fn sample_pr_json(id: u64, state: &str) -> serde_json::Value {
    json!({
        "id": id,
        "title": "Fix X",
        "state": state,
        "source": { "branch": { "name": "feat/x" },
                    "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
        "destination": { "branch": { "name": "main" },
                         "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
        "links": { "html": { "href": format!("https://bitbucket.org/sdadev/bvrm/pull-requests/{id}") } },
        "author": { "display_name": "Ash", "role": "AUTHOR", "approved": false }
    })
}

#[tokio::test]
async fn creates_approves_merges_and_declines_pr() {
    use bbr::api::pr::{CreateBranchRef, CreateNamed, CreatePrRequest, MergePrRequest};
    use wiremock::matchers::body_partial_json;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repositories/sdadev/bvrm/pullrequests"))
        .and(header("authorization", AUTH_BASIC))
        .and(body_partial_json(json!({ "title": "Fix X" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(sample_pr_json(467, "OPEN")))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/approve"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/467/merge"))
        .and(header("authorization", AUTH_BASIC))
        .and(body_partial_json(json!({ "merge_strategy": "squash" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_pr_json(467, "MERGED")))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/468/decline"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_pr_json(468, "DECLINED")))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let created = c
        .create_pr(
            "sdadev",
            "bvrm",
            &CreatePrRequest {
                title: "Fix X".into(),
                description: None,
                source: CreateBranchRef {
                    branch: CreateNamed {
                        name: "feat/x".into(),
                    },
                },
                destination: CreateBranchRef {
                    branch: CreateNamed {
                        name: "main".into(),
                    },
                },
                close_source_branch: None,
                reviewers: vec![],
                draft: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(created.id, 467);
    assert_eq!(created.state, "OPEN");

    c.approve_pr("sdadev", "bvrm", 467).await.unwrap();

    let merged = c
        .merge_pr(
            "sdadev",
            "bvrm",
            467,
            Some(&MergePrRequest {
                close_source_branch: None,
                merge_strategy: Some("squash".into()),
                message: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(merged.state, "MERGED");

    let declined = c.decline_pr("sdadev", "bvrm", 468).await.unwrap();
    assert_eq!(declined.state, "DECLINED");
}

#[tokio::test]
async fn prs_for_branch_returns_open_prs() {
    use wiremock::matchers::query_param;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests"))
        .and(header("authorization", AUTH_BASIC))
        .and(query_param(
            "q",
            "source.branch.name=\"feat/x\" AND state=\"OPEN\"",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 1, "page": 1, "pagelen": 50,
            "values": [sample_pr_json(467, "OPEN")]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let prs = c.prs_for_branch("sdadev", "bvrm", "feat/x").await.unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].id, 467);
    assert_eq!(prs[0].source_branch(), "feat/x");
}
