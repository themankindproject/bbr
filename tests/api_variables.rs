use serde_json::json;
use wiremock::matchers::{method, path_regex};
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

#[tokio::test]
async fn list_repo_pipeline_variables() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines_config/variables/",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pagelen": 10,
            "values": [
                {
                    "type": "pipeline_variable",
                    "uuid": "{var-uuid-1}",
                    "key": "AWS_ACCESS_KEY",
                    "value": "AKIA...",
                    "secured": false
                },
                {
                    "type": "pipeline_variable",
                    "uuid": "{var-uuid-2}",
                    "key": "AWS_SECRET_KEY",
                    "secured": true
                }
            ]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let vars = c.list_pipeline_variables("ws", "repo").await.unwrap();

    assert_eq!(vars.len(), 2);
    assert_eq!(vars[0].key, "AWS_ACCESS_KEY");
    assert!(!vars[0].secured);
    assert_eq!(vars[0].value, Some("AKIA...".to_string()));
    assert_eq!(vars[1].key, "AWS_SECRET_KEY");
    assert!(vars[1].secured);
    assert_eq!(vars[1].value, None);
}

#[tokio::test]
async fn create_repo_pipeline_variable() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines_config/variables/",
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "type": "pipeline_variable",
            "uuid": "{new-uuid}",
            "key": "MY_VAR",
            "value": "my_value",
            "secured": false
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let var = c
        .create_pipeline_variable("ws", "repo", "MY_VAR", "my_value", false)
        .await
        .unwrap();

    assert_eq!(var.key, "MY_VAR");
    assert_eq!(var.value, Some("my_value".to_string()));
    assert!(!var.secured);
}

#[tokio::test]
async fn update_repo_pipeline_variable() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines_config/variables/%7Bvar-uuid-1%7D",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "pipeline_variable",
            "uuid": "{var-uuid-1}",
            "key": "MY_VAR",
            "value": "updated_value",
            "secured": false
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let var = c
        .update_pipeline_variable("ws", "repo", "{var-uuid-1}", "MY_VAR", "updated_value", false)
        .await
        .unwrap();

    assert_eq!(var.key, "MY_VAR");
    assert_eq!(var.value, Some("updated_value".to_string()));
}

#[tokio::test]
async fn delete_repo_pipeline_variable() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path_regex(
            r"^/repositories/ws/repo/pipelines_config/variables/%7Bvar-uuid-1%7D",
        ))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    // delete_pipeline_variable returns Result<()>
    c.delete_pipeline_variable("ws", "repo", "{var-uuid-1}")
        .await
        .unwrap();
}
