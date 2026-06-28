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

// ---- DEPLOYMENTS & ENVIRONMENTS -------------------------------------------

#[tokio::test]
async fn test_deployments_and_environments() {
    let server = MockServer::start().await;

    // Mock list deployments
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/deployments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "uuid": "{deploy-1}",
                "state": { "name": "SUCCESSFUL" },
                "environment": { "name": "Production" },
                "deployable": {
                    "pipeline": { "uuid": "{pipe-1}", "build_number": 100 },
                    "commit": { "hash": "a1222bb" }
                },
                "last_update_time": "2026-06-29T00:00:00Z"
            }]
        })))
        .mount(&server)
        .await;

    // Mock list environments
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/environments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "uuid": "{env-1}",
                "name": "Production",
                "environment_type": { "name": "production", "rank": 4 },
                "rank": 4,
                "hidden": false
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;

    let deployments = c.list_deployments("sdadev", "bvrm", 10).await.unwrap();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].uuid, "{deploy-1}");
    assert_eq!(deployments[0].state.name, "SUCCESSFUL");

    let environments = c.list_environments("sdadev", "bvrm").await.unwrap();
    assert_eq!(environments.len(), 1);
    assert_eq!(environments[0].uuid, "{env-1}");
    assert_eq!(environments[0].name, "Production");
}

// ---- WEBHOOKS -------------------------------------------------------------

#[tokio::test]
async fn test_webhooks_crud() {
    let server = MockServer::start().await;

    // Mock create webhook
    Mock::given(method("POST"))
        .and(path("/repositories/sdadev/bvrm/hooks"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "uuid": "{hook-1}",
            "url": "https://example.com/callback",
            "active": true,
            "secret_set": false,
            "events": ["repo:push"]
        })))
        .mount(&server)
        .await;

    // Mock delete webhook
    Mock::given(method("DELETE"))
        .and(path("/repositories/sdadev/bvrm/hooks/%7Bhook-1%7D"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;

    let webhook = c.create_webhook("sdadev", "bvrm", "https://example.com/callback", None, &["repo:push".to_string()], true, None).await.unwrap();
    assert_eq!(webhook.uuid, "{hook-1}");
    assert_eq!(webhook.url, "https://example.com/callback");
    assert!(webhook.active);

    let delete_res = c.delete_webhook("sdadev", "bvrm", "{hook-1}").await;
    assert!(delete_res.is_ok());
}

// ---- ISSUE TRACKER --------------------------------------------------------

#[tokio::test]
async fn test_issue_tracker() {
    let server = MockServer::start().await;

    // Mock list issues
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "id": 1,
                "title": "A test issue",
                "state": "new",
                "kind": "bug",
                "priority": "major",
                "comment_count": 0,
                "votes": 0,
                "watches": 0,
                "links": { "html": { "href": "https://bitbucket.org/sdadev/bvrm/issues/1" } }
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;

    let issues = c.list_issues("sdadev", "bvrm", 10, None, None, None, None, None).await.unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].id, 1);
    assert_eq!(issues[0].title, "A test issue");
    assert_eq!(issues[0].state, "new");
}

// ---- SOURCE BROWSER -------------------------------------------------------

#[tokio::test]
async fn test_source_browser() {
    let server = MockServer::start().await;

    // Mock list src
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/src/main/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "type": "commit_file",
                "path": "README.md",
                "size": 120,
                "attributes": [],
                "commit": { "hash": "a1222bb", "date": "2026-06-29T00:00:00Z" }
            }]
        })))
        .mount(&server)
        .await;

    // Mock get file raw
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/src/main/README.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello world"))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;

    let entries = c.list_src("sdadev", "bvrm", "main", "").await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "README.md");
    assert_eq!(entries[0].size, Some(120));

    let raw = c.get_file_raw("sdadev", "bvrm", "main", "README.md").await.unwrap();
    assert_eq!(raw, "hello world");
}
