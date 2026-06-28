use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
async fn lists_repository_tags() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/refs/tags"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "name": "v1.0.0",
                "target": { "hash": "abc123" },
                "date": "2026-01-01T00:00:00Z"
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let tags = c.list_tags("sdadev", "bvrm", 20).await.unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v1.0.0");
    assert_eq!(
        tags[0].target.as_ref().map(|t| t.hash.as_str()),
        Some("abc123")
    );
}
