//! Integration tests for pipeline (CI) endpoints against a mock server.

use serde_json::json;
use wiremock::matchers::{method, path_regex};
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
async fn fetches_latest_pipeline_and_steps() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(r"^/repositories/sdadev/bvrm/pipelines/$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 1, "pagelen": 1,
            "values": [{
                "uuid": "{abc-123}",
                "build_number": 42,
                "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } },
                "result": { "name": "SUCCESSFUL" },
                "duration_in_seconds": 172,
                "target": { "ref": { "name": "test-ci", "target": { "hash": "4644ec4b" } } }
            }]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repositories/sdadev/bvrm/pipelines/abc-123/steps/$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "uuid": "{step-1}",
                "name": "Run Tests",
                "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } },
                "duration_in_seconds": 172
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let pipeline = c
        .latest_pipeline("sdadev", "bvrm", Some("test-ci"))
        .await
        .unwrap()
        .expect("a pipeline");
    assert_eq!(pipeline.state_name(), "SUCCESSFUL");
    assert_eq!(pipeline.duration_in_seconds, 172);
    assert_eq!(
        pipeline
            .target
            .ref_
            .as_ref()
            .and_then(|r| r.target.as_ref())
            .map(|t| t.hash.as_str()),
        Some("4644ec4b")
    );

    let steps = c
        .list_steps("sdadev", "bvrm", &pipeline.uuid)
        .await
        .unwrap();
    assert_eq!(steps.values.len(), 1);
    assert_eq!(steps.values[0].name, "Run Tests");
}

#[tokio::test]
async fn no_pipeline_yields_not_found_exit_code() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repositories/sdadev/bvrm/pipelines/$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [], "pagelen": 1
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let result = c.latest_pipeline("sdadev", "bvrm", None).await.unwrap();
    assert!(result.is_none());
}
