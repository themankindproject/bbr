//! Integration tests for deploy-key endpoints.

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

// ---- LIST -----------------------------------------------------------------

#[tokio::test]
async fn list_deploy_keys_returns_all() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/deploy-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                {
                    "id": 1,
                    "key": "ssh-rsa AAAA1111",
                    "label": "ci-key",
                    "type": "deploy_key",
                    "created_on": "2024-01-01T00:00:00Z",
                    "comment": "user@host",
                    "last_used": null
                },
                {
                    "id": 2,
                    "key": "ssh-ed25519 BBBB2222",
                    "label": "deploy-prod",
                    "type": "deploy_key",
                    "created_on": "2024-06-15T12:00:00Z",
                    "comment": "deploy@server",
                    "last_used": "2024-07-01T08:00:00Z"
                }
            ]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let keys = c.list_deploy_keys("ws", "slug").await.unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].id, 1);
    assert_eq!(keys[0].label, "ci-key");
    assert_eq!(keys[0].key, "ssh-rsa AAAA1111");
    assert_eq!(keys[1].id, 2);
    assert_eq!(keys[1].label, "deploy-prod");
    assert_eq!(keys[1].last_used.as_deref(), Some("2024-07-01T08:00:00Z"));
}

#[tokio::test]
async fn list_deploy_keys_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/deploy-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": []
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let keys = c.list_deploy_keys("ws", "slug").await.unwrap();
    assert!(keys.is_empty());
}

// ---- ADD ------------------------------------------------------------------

#[tokio::test]
async fn add_deploy_key_returns_created() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repositories/ws/slug/deploy-keys"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 42,
            "key": "ssh-rsa NEWKEY123",
            "label": "new-key",
            "type": "deploy_key",
            "created_on": "2024-07-10T00:00:00Z",
            "comment": "admin@laptop",
            "last_used": null
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let dk = c
        .add_deploy_key("ws", "slug", "ssh-rsa NEWKEY123", "new-key")
        .await
        .unwrap();
    assert_eq!(dk.id, 42);
    assert_eq!(dk.key, "ssh-rsa NEWKEY123");
    assert_eq!(dk.label, "new-key");
    assert_eq!(dk.comment.as_deref(), Some("admin@laptop"));
}

// ---- VIEW -----------------------------------------------------------------

#[tokio::test]
async fn get_deploy_key_returns_single() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/deploy-keys/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 7,
            "key": "ssh-ed25519 VIEWKEY",
            "label": "staging",
            "type": "deploy_key",
            "created_on": "2023-03-01T10:00:00Z",
            "comment": "ops@ci",
            "last_used": "2024-06-30T18:00:00Z"
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let dk = c.get_deploy_key("ws", "slug", 7).await.unwrap();
    assert_eq!(dk.id, 7);
    assert_eq!(dk.label, "staging");
    assert_eq!(dk.key, "ssh-ed25519 VIEWKEY");
    assert_eq!(dk.last_used.as_deref(), Some("2024-06-30T18:00:00Z"));
}

// ---- DELETE ---------------------------------------------------------------

#[tokio::test]
async fn delete_deploy_key_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/repositories/ws/slug/deploy-keys/99"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let result = c.delete_deploy_key("ws", "slug", 99).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn delete_deploy_key_not_found() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/repositories/ws/slug/deploy-keys/404"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": {"message": "Resource not found"}
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let result = c.delete_deploy_key("ws", "slug", 404).await;
    assert!(result.is_err());
}
