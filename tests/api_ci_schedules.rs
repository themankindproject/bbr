//! Integration tests for pipeline schedule API methods.

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

#[tokio::test]
async fn list_schedules_returns_all() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pipelines_config/schedules/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                {
                    "type": "pipeline_schedule",
                    "uuid": "{sched-1}",
                    "enabled": true,
                    "cron_pattern": "0 2 * * *",
                    "target": {
                        "type": "pipeline_ref_target",
                        "ref_name": "main",
                        "ref_type": "branch",
                        "selector": { "type": "branches", "pattern": "default" }
                    },
                    "created_on": "2024-01-01T00:00:00.000000+00:00",
                    "updated_on": "2024-06-01T00:00:00.000000+00:00"
                },
                {
                    "type": "pipeline_schedule",
                    "uuid": "{sched-2}",
                    "enabled": false,
                    "cron_pattern": "0 4 * * 1",
                    "target": {
                        "type": "pipeline_ref_target",
                        "ref_name": "develop",
                        "ref_type": "branch"
                    },
                    "created_on": "2024-02-01T00:00:00.000000+00:00",
                    "updated_on": "2024-07-01T00:00:00.000000+00:00"
                }
            ],
            "pagelen": 100
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let schedules = c.list_schedules("ws", "repo").await.unwrap();
    assert_eq!(schedules.len(), 2);
    assert_eq!(schedules[0].uuid, "{sched-1}");
    assert!(schedules[0].enabled);
    assert_eq!(schedules[0].cron_pattern, "0 2 * * *");
    assert_eq!(
        schedules[0].target.as_ref().unwrap().ref_name.as_deref(),
        Some("main")
    );
    assert_eq!(schedules[1].uuid, "{sched-2}");
    assert!(!schedules[1].enabled);
}

#[tokio::test]
async fn create_schedule_posts_correctly() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repositories/ws/repo/pipelines_config/schedules/"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "type": "pipeline_schedule",
            "uuid": "{new-sched}",
            "enabled": true,
            "cron_pattern": "0 3 * * *",
            "target": {
                "type": "pipeline_ref_target",
                "ref_name": "main",
                "ref_type": "branch",
                "selector": { "type": "custom" }
            },
            "created_on": "2024-07-01T00:00:00.000000+00:00",
            "updated_on": "2024-07-01T00:00:00.000000+00:00"
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let schedule = c
        .create_schedule("ws", "repo", "0 3 * * *", "main", Some("custom"))
        .await
        .unwrap();
    assert_eq!(schedule.uuid, "{new-sched}");
    assert!(schedule.enabled);
    assert_eq!(schedule.cron_pattern, "0 3 * * *");
}

#[tokio::test]
async fn get_schedule_fetches_by_uuid() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/schedules/%7Bsched-1%7D",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "pipeline_schedule",
            "uuid": "{sched-1}",
            "enabled": true,
            "cron_pattern": "0 2 * * *",
            "target": {
                "type": "pipeline_ref_target",
                "ref_name": "main",
                "ref_type": "branch"
            },
            "created_on": "2024-01-01T00:00:00.000000+00:00",
            "updated_on": "2024-06-01T00:00:00.000000+00:00"
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let schedule = c.get_schedule("ws", "repo", "%7Bsched-1%7D").await.unwrap();
    assert_eq!(schedule.uuid, "{sched-1}");
    assert!(schedule.enabled);
}

#[tokio::test]
async fn update_schedule_sends_put() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/schedules/%7Bsched-1%7D",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "pipeline_schedule",
            "uuid": "{sched-1}",
            "enabled": false,
            "cron_pattern": "0 5 * * *",
            "target": {
                "type": "pipeline_ref_target",
                "ref_name": "main",
                "ref_type": "branch"
            },
            "created_on": "2024-01-01T00:00:00.000000+00:00",
            "updated_on": "2024-07-01T12:00:00.000000+00:00"
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let schedule = c
        .update_schedule(
            "ws",
            "repo",
            "%7Bsched-1%7D",
            Some("0 5 * * *"),
            Some(false),
        )
        .await
        .unwrap();
    assert_eq!(schedule.uuid, "{sched-1}");
    assert!(!schedule.enabled);
    assert_eq!(schedule.cron_pattern, "0 5 * * *");
}

#[tokio::test]
async fn delete_schedule_sends_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/schedules/%7Bsched-1%7D",
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    c.delete_schedule("ws", "repo", "%7Bsched-1%7D")
        .await
        .unwrap();
}

#[tokio::test]
async fn schedule_executions_returns_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/schedules/%7Bsched-1%7D/executions",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                {
                    "type": "pipeline_schedule_execution",
                    "uuid": "{exec-1}",
                    "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } },
                    "created_on": "2024-06-01T02:00:00.000000+00:00"
                },
                {
                    "type": "pipeline_schedule_execution",
                    "uuid": "{exec-2}",
                    "state": { "name": "COMPLETED", "result": { "name": "FAILED" } },
                    "created_on": "2024-06-02T02:00:00.000000+00:00"
                }
            ],
            "pagelen": 25
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let execs = c
        .schedule_executions("ws", "repo", "%7Bsched-1%7D", 25)
        .await
        .unwrap();
    assert_eq!(execs.len(), 2);
    assert_eq!(execs[0].uuid.as_deref(), Some("{exec-1}"));
    assert_eq!(execs[0].state.as_ref().unwrap().name, "COMPLETED");
    assert_eq!(execs[1].uuid.as_deref(), Some("{exec-2}"));
}
