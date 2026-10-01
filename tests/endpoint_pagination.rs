//! Endpoint adapters must preserve item limits independently from page sizes.

use bbr::{api::BitbucketClient, auth::Credentials};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> BitbucketClient {
    BitbucketClient::new(
        &server.uri(),
        Credentials {
            username: "test".into(),
            secret: "fake-token".into(),
        },
    )
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Endpoint {
    Issues,
    Comments,
    Source,
    Variables,
    Schedules,
    Executions,
    EnvVariables,
}

impl Endpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Issues => "/repositories/ws/repo/issues",
            Self::Comments => "/repositories/ws/repo/issues/1/comments",
            Self::Source => "/repositories/ws/repo/src/main/",
            Self::Variables => "/repositories/ws/repo/pipelines_config/variables/",
            Self::Schedules => "/repositories/ws/repo/pipelines_config/schedules/",
            Self::Executions => {
                "/repositories/ws/repo/pipelines_config/schedules/schedule/executions"
            }
            Self::EnvVariables => {
                "/repositories/ws/repo/deployments_config/environments/env/variables"
            }
        }
    }
    fn cap(self) -> u32 {
        match self {
            Self::Issues | Self::Comments => 50,
            _ => 100,
        }
    }
    fn limited(self) -> bool {
        matches!(self, Self::Issues | Self::Comments | Self::Executions)
    }
    async fn list(self, c: &BitbucketClient, limit: u32) -> bbr::Result<Value> {
        Ok(match self {
            Self::Issues => serde_json::to_value(
                c.list_issues("ws", "repo", limit, Some("open"), None, None, None, None)
                    .await?,
            )?,
            Self::Comments => {
                serde_json::to_value(c.list_issue_comments("ws", "repo", 1, limit).await?)?
            }
            Self::Source => serde_json::to_value(c.list_src("ws", "repo", "main", "").await?)?,
            Self::Variables => {
                serde_json::to_value(c.list_pipeline_variables("ws", "repo").await?)?
            }
            Self::Schedules => serde_json::to_value(c.list_schedules("ws", "repo").await?)?,
            Self::Executions => serde_json::to_value(
                c.schedule_executions("ws", "repo", "schedule", limit)
                    .await?,
            )?,
            Self::EnvVariables => {
                serde_json::to_value(c.list_env_variables("ws", "repo", "env").await?)?
            }
        })
    }
}

async fn assert_pages(endpoint: Endpoint) {
    let limits: &[u32] = if endpoint.limited() {
        &[1, 3, 101]
    } else {
        &[104]
    };
    for &limit in limits {
        let server = MockServer::start().await;
        // All endpoint DTOs accept this fixture; IDs make lost/reordered rows visible.
        let item = |i: u32| json!({"id": i, "uuid": format!("item-{i}"), "path": format!("file-{i}"), "key": format!("KEY_{i}")});
        let page_size = if endpoint.limited() {
            limit.min(endpoint.cap())
        } else {
            endpoint.cap()
        };
        let mut first = Mock::given(method("GET"))
            .and(path(endpoint.path()))
            .and(query_param("pagelen", page_size.to_string()));
        if matches!(endpoint, Endpoint::Issues) {
            first = first.and(query_param("q", "state=\"open\""));
        }
        first.respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [item(1), item(2)], "next": format!("{}/continuation?cursor=opaque", server.uri())
        }))).expect(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/continuation"))
            .and(query_param("cursor", "opaque"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"values": (3..=104).map(item).collect::<Vec<_>>()})),
            )
            .expect(if limit <= 2 { 0 } else { 1 })
            .mount(&server)
            .await;
        let result = endpoint.list(&client(&server), limit).await.unwrap();
        let rows = result.as_array().unwrap();
        assert_eq!(
            rows.len(),
            limit as usize,
            "{endpoint:?}: limit {limit} ignored"
        );
        let field = match endpoint {
            Endpoint::Issues | Endpoint::Comments => "id",
            Endpoint::Source => "path",
            _ => "uuid",
        };
        let expected = match field {
            "id" => json!(limit),
            "path" => json!(format!("file-{limit}")),
            _ => json!(format!("item-{limit}")),
        };
        assert_eq!(rows.last().unwrap()[field], expected);
    }
}

#[tokio::test]
async fn issues_follow_next_and_honor_limits() {
    assert_pages(Endpoint::Issues).await;
}
#[tokio::test]
async fn issue_comments_follow_next_and_honor_limits() {
    assert_pages(Endpoint::Comments).await;
}
#[tokio::test]
async fn source_directory_includes_later_pages() {
    assert_pages(Endpoint::Source).await;
}
#[tokio::test]
async fn pipeline_variables_include_later_pages() {
    assert_pages(Endpoint::Variables).await;
}
#[tokio::test]
async fn schedules_include_later_pages() {
    assert_pages(Endpoint::Schedules).await;
}
#[tokio::test]
async fn schedule_executions_follow_next_and_honor_limits() {
    assert_pages(Endpoint::Executions).await;
}
#[tokio::test]
async fn environment_variables_include_later_pages() {
    assert_pages(Endpoint::EnvVariables).await;
}

#[tokio::test]
async fn zero_limit_lists_make_no_requests() {
    let server = MockServer::start().await;
    let c = client(&server);
    for endpoint in [Endpoint::Issues, Endpoint::Comments, Endpoint::Executions] {
        assert!(endpoint
            .list(&c, 0)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

fn command(home: &tempfile::TempDir, server: &MockServer) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("bbr").unwrap();
    cmd.current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_USERNAME", "test")
        .env("BITBUCKET_TOKEN", "fake-token")
        .env("BITBUCKET_API_BASE", server.uri())
        .env("BB_WORKSPACE", "ws")
        .env("BB_SLUG", "repo")
        .env("BBR_NO_INTERACTIVE", "1")
        .timeout(std::time::Duration::from_secs(5));
    cmd
}

#[tokio::test]
async fn workspace_list_caps_page_size_but_returns_requested_items() {
    for limit in [1u32, 3, 101] {
        let server = MockServer::start().await;
        let home = tempfile::tempdir().unwrap();
        let item = |i| json!({"workspace": {"slug": format!("ws-{i}"), "uuid": format!("id-{i}")}});
        Mock::given(path("/user/workspaces")).and(query_param("pagelen", limit.min(100).to_string()))
            .and(query_param("q", "permission=\"admin\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [item(1), item(2)], "next": format!("{}/next?cursor=workspaces", server.uri())
            }))).expect(1).mount(&server).await;
        Mock::given(path("/next"))
            .and(query_param("cursor", "workspaces"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"values": (3..=104).map(item).collect::<Vec<_>>()})),
            )
            .expect(if limit == 1 { 0 } else { 1 })
            .mount(&server)
            .await;
        let output = command(&home, &server)
            .args([
                "workspace",
                "list",
                "--role",
                "admin",
                "--limit",
                &limit.to_string(),
                "--json",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        let rows = result["workspaces"].as_array().unwrap();
        assert_eq!(rows.len(), limit as usize);
        assert_eq!(rows.last().unwrap()["name"], format!("ws-{limit}"));
    }
}

#[tokio::test]
async fn variable_set_does_not_write_when_a_later_page_fails() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    Mock::given(method("GET"))
        .and(path(Endpoint::Variables.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [], "next": format!("{}/more-vars", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/more-vars"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&home, &server)
        .args(["variable", "set", "TARGET", "--stdin", "--json"])
        .write_stdin("fake-value")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["kind"],
        "auth"
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() == "GET"));
}

#[tokio::test]
async fn variable_set_updates_existing_key_on_later_page_instead_of_creating_duplicate() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    Mock::given(method("GET"))
        .and(path(Endpoint::Variables.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{"uuid":"one", "key":"OTHER"}], "next": format!("{}/more-vars", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/more-vars"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"values":[{"uuid":"target-id", "key":"TARGET"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/variables/target-id",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"uuid":"target-id", "key":"TARGET"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let output = command(&home, &server)
        .args(["variable", "set", "TARGET", "--stdin", "--json"])
        .write_stdin("fake-value")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["action"],
        "updated"
    );
}
