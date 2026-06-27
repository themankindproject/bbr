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
        kind: CredentialKind::Pat,
    };
    BitbucketClient::new(base, creds).unwrap()
}

#[tokio::test]
async fn creates_commit_build_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/repositories/sdadev/bvrm/commit/abc123/statuses/build",
        ))
        .and(header("authorization", "Bearer tok"))
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
