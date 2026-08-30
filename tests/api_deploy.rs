//! Integration tests for the deployment fetch / environment-history endpoints.

use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use bbr::api::BitbucketClient;
use bbr::auth::Credentials;

async fn client(base: &str) -> BitbucketClient {
    let creds = Credentials {
        username: "u@example.com".into(),
        secret: "tok".into(),
    };
    BitbucketClient::new(base, creds).unwrap()
}

#[tokio::test]
async fn get_deployment_adds_braces_and_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/deployments/%7Bdep-1%7D"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "uuid": "{dep-1}",
            "state": {"name": "SUCCESSFUL"},
            "environment": {"uuid": "{env-1}", "name": "production"},
            "deployable": {
                "pipeline": {"uuid": "{pipe-1}", "build_number": 7},
                "commit": {"hash": "cafe1234abcd"}
            }
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    // Pass the UUID without braces — the client must add them.
    let d = c.get_deployment("ws", "slug", "dep-1").await.unwrap();
    assert_eq!(d.uuid, "{dep-1}");
    assert_eq!(d.state.name, "SUCCESSFUL");
    assert_eq!(
        d.deployable
            .as_ref()
            .unwrap()
            .pipeline
            .as_ref()
            .unwrap()
            .build_number,
        7
    );
    assert_eq!(
        d.deployable.as_ref().unwrap().commit.as_ref().unwrap().hash,
        "cafe1234abcd"
    );
}

#[tokio::test]
async fn get_deployment_404_maps_to_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex("^/repositories/ws/slug/deployments/"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"error": {"message": "Resource not found"}})),
        )
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .get_deployment("ws", "slug", "{missing}")
        .await
        .unwrap_err();
    assert!(
        matches!(err, bbr::error::BitbucketError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[tokio::test]
async fn list_deployments_for_environment_returns_values() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/repositories/ws/slug/deployments_config/environments/%7Benv-1%7D/changes",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                {
                    "uuid": "{newest}",
                    "state": {"name": "SUCCESSFUL"},
                    "environment": {"uuid": "{env-1}", "name": "production"},
                    "deployable": {"commit": {"hash": "newhash111"}}
                },
                {
                    "uuid": "{older}",
                    "state": {"name": "SUCCESSFUL"},
                    "environment": {"uuid": "{env-1}", "name": "production"},
                    "deployable": {"commit": {"hash": "oldhash222"}}
                }
            ]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let deps = c
        .list_deployments_for_environment("ws", "slug", "{env-1}", 50)
        .await
        .unwrap();
    assert_eq!(deps.len(), 2);
    assert_eq!(deps[0].uuid, "{newest}");
    assert_eq!(deps[1].uuid, "{older}");
    assert_eq!(
        deps[1]
            .deployable
            .as_ref()
            .and_then(|d| d.commit.as_ref())
            .unwrap()
            .hash,
        "oldhash222"
    );
}
