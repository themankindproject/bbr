use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use bbr::api::status::BuildStatusRequest;
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
async fn lists_commit_statuses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/commit/abc123/statuses"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 1,
            "pagelen": 100,
            "values": [{
                "state": "SUCCESSFUL",
                "key": "lint",
                "name": "Lint",
                "url": "https://ci.example/lint"
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let page = c.commit_statuses("sdadev", "bvrm", "abc123").await.unwrap();
    assert_eq!(page.values.len(), 1);
    assert_eq!(page.values[0].key, "lint");
}

#[tokio::test]
async fn commit_statuses_not_found_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/commit/deadbeef/statuses"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "type": "error",
            "error": { "message": "Commit not found" }
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .commit_statuses("sdadev", "bvrm", "deadbeef")
        .await
        .unwrap_err();
    assert!(matches!(err, bbr::error::BitbucketError::NotFound(_)));
}

#[tokio::test]
async fn creates_commit_build_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/repositories/sdadev/bvrm/commit/abc123/statuses/build",
        ))
        .and(header("authorization", AUTH_BASIC))
        .and(body_json(json!({
            "key": "lint",
            "state": "SUCCESSFUL",
            "name": "Lint",
            "url": "https://ci.example/lint",
            "description": "all good",
            "refname": "feature-x"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "key": "lint",
            "state": "SUCCESSFUL",
            "name": "Lint",
            "url": "https://ci.example/lint",
            "description": "all good",
            "refname": "feature-x"
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let status = c
        .create_commit_status(
            "sdadev",
            "bvrm",
            "abc123",
            &BuildStatusRequest {
                key: "lint".into(),
                state: "SUCCESSFUL".into(),
                name: Some("Lint".into()),
                url: Some("https://ci.example/lint".into()),
                description: Some("all good".into()),
                refname: Some("feature-x".into()),
            },
        )
        .await
        .unwrap();

    assert_eq!(status.key, "lint");
    assert_eq!(status.state, "SUCCESSFUL");
}
