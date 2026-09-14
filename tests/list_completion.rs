//! Full-list consumers must not lose pagination metadata or silently cap results.

use bbr::{api::BitbucketClient, auth::Credentials};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

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
        .env("NO_COLOR", "1")
        .env("BBR_NO_INTERACTIVE", "1")
        .timeout(std::time::Duration::from_secs(5));
    cmd
}

#[tokio::test]
async fn search_follows_cursors_and_keeps_total_independent_of_limit() {
    for limit in [1u32, 3, 101] {
        let server = MockServer::start().await;
        let home = tempfile::tempdir().unwrap();
        let hit = |i| json!({"file":{"path":format!("file-{i}.rs")},"content":[{"lines":[{"line":7,"segments":[{"text":"found"}]}]}]});
        Mock::given(path("/workspaces/ws/search/code"))
            .and(query_param("search_query", "test repo:repo"))
            .and(query_param("pagelen", limit.min(100).to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "size": 999, "values":[hit(1),hit(2)],
                "next":format!("{}/more-search?cursor=a%2Bb", server.uri())
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/more-search"))
            .and(query_param("cursor", "a+b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "size":1000,"values":(3..=104).map(hit).collect::<Vec<_>>()
            })))
            .expect(if limit == 1 { 0 } else { 1 })
            .mount(&server)
            .await;
        let output = command(&home, &server)
            .args([
                "search",
                "test",
                "--repo",
                "repo",
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
        assert_eq!(
            result["total"], 999,
            "retain first-page total as reported by the server"
        );
        assert_eq!(result["query"], "test");
        let rows = result["results"].as_array().unwrap();
        assert_eq!(rows.len(), limit as usize);
        assert_eq!(rows.last().unwrap()["file"], format!("file-{limit}.rs"));
        assert_eq!(
            rows.last().unwrap()["content_matches"][0],
            format!("file-{limit}.rs:7  found")
        );
    }
}

#[tokio::test]
async fn search_continuation_error_does_not_return_a_partial_success() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    Mock::given(path("/workspaces/ws/search/code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size":2,"values":[{"file":{"path":"one"}}],"next":format!("{}/failed",server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/failed"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let output = command(&home, &server)
        .args(["search", "test", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

fn pr(i: u64) -> Value {
    json!({"id":i,"title":format!("PR {i}"),"state":"OPEN","source":{"branch":{"name":"feature/x"}},"destination":{"branch":{"name":"main"}}})
}

#[tokio::test]
async fn full_branch_lookup_fetches_more_than_fifty_prs() {
    let server = MockServer::start().await;
    Mock::given(path("/repositories/ws/repo/pullrequests"))
        .and(query_param("pagelen", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values":(1..=50).map(pr).collect::<Vec<_>>(),"next":format!("{}/more-prs",server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/more-prs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values":[pr(51)]})))
        .expect(1)
        .mount(&server)
        .await;
    let prs = client(&server)
        .prs_for_branch("ws", "repo", "feature/x")
        .await
        .unwrap();
    assert_eq!(prs.len(), 51);
    assert_eq!(prs.last().unwrap().id, 51);
}

#[tokio::test]
async fn lightweight_branch_lookup_keeps_single_result_cost() {
    let server = MockServer::start().await;
    Mock::given(path("/repositories/ws/repo/pullrequests"))
        .and(query_param("pagelen", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values":[pr(1),pr(2)],"next":format!("{}/must-not-fetch",server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        client(&server)
            .pr_for_branch_light("ws", "repo", "feature/x")
            .await
            .unwrap()
            .unwrap()
            .id,
        1
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

const STEPS: &str = "/repositories/ws/repo/pipelines/%7Bpipe%7D/steps/";
fn step(i: u64) -> Value {
    json!({"uuid":format!("step-{i}"),"name":format!("Step {i}"),"state":{"name":if i==2 {"FAILED"} else {"SUCCESSFUL"}},"duration_in_seconds":i})
}

async fn mock_steps(server: &MockServer, fail_next: bool) {
    let next = format!("{}/more-steps?cursor=next", server.uri());
    Mock::given(path(STEPS))
        .respond_with(move |req: &Request| {
            let fields = req
                .url
                .query_pairs()
                .find(|(key, _)| key == "fields")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let mut body = json!({"values":[step(1)],"size":2,"page":1,"pagelen":1});
            // Emulate Bitbucket's partial-response fields projection.
            if fields.split(',').any(|field| field == "next") {
                body["next"] = json!(next);
            }
            ResponseTemplate::new(200).set_body_json(body)
        })
        .expect(1)
        .mount(server)
        .await;
    let response = if fail_next {
        ResponseTemplate::new(403)
    } else {
        ResponseTemplate::new(200)
            .set_body_json(json!({"values":[step(2)],"page":2,"size":2,"pagelen":1}))
    };
    Mock::given(path("/more-steps"))
        .and(query_param("cursor", "next"))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn ci_steps_includes_all_pages_without_changing_json_shape() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mock_steps(&server, false).await;
    let output = command(&home, &server)
        .args(["ci", "steps", "pipe", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["uuid"], "{pipe}");
    assert_eq!(result["steps"].as_array().unwrap().len(), 2);
    assert_eq!(result["steps"][1]["name"], "Step 2");
}

#[tokio::test]
async fn ci_logs_can_select_a_failed_step_on_a_later_page() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mock_steps(&server, false).await;
    Mock::given(path(
        "/repositories/ws/repo/pipelines/%7Bpipe%7D/steps/step-2/log",
    ))
    .respond_with(ResponseTemplate::new(200).set_body_string("page-two failure log\n"))
    .expect(1)
    .mount(&server)
    .await;
    let output = command(&home, &server)
        .args(["ci", "logs", "pipe", "--failed", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["step"], "step-2");
    assert_eq!(result["log"], "page-two failure log\n");
}

#[tokio::test]
async fn ci_tests_can_select_a_named_step_on_a_later_page() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mock_steps(&server, false).await;
    Mock::given(path(
        "/repositories/ws/repo/pipelines/%7Bpipe%7D/steps/step-2/test_reports",
    ))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"total":1,"failed":1})))
    .expect(1)
    .mount(&server)
    .await;
    Mock::given(path(
        "/repositories/ws/repo/pipelines/%7Bpipe%7D/steps/step-2/test_cases",
    ))
    .respond_with(
        ResponseTemplate::new(200)
            .set_body_json(json!({"values":[{"status":"FAILED","test_name":"failure"}]})),
    )
    .expect(1)
    .mount(&server)
    .await;
    let output = command(&home, &server)
        .args(["ci", "tests", "pipe", "--step", "Step 2", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["step_uuid"], "step-2");
    assert_eq!(result["report"]["failed"], 1);
}

#[tokio::test]
async fn ci_steps_surfaces_later_page_failures() {
    let server = MockServer::start().await;
    let home = tempfile::tempdir().unwrap();
    mock_steps(&server, true).await;
    let output = command(&home, &server)
        .args(["ci", "steps", "pipe", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[tokio::test]
async fn raw_step_page_keeps_its_pagination_metadata() {
    let server = MockServer::start().await;
    let next = format!("{}/more-steps", server.uri());
    let response = json!({"values":[step(1)],"page":1,"size":2,"pagelen":1,"next":next});
    Mock::given(path(STEPS))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;
    let page = client(&server)
        .list_steps("ws", "repo", "{pipe}")
        .await
        .unwrap();
    assert_eq!(page.values.len(), 1);
    assert_eq!(page.next, Some(next));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn pipeline_list_requests_next_metadata_and_honors_limit() {
    let server = MockServer::start().await;
    let next = format!("{}/more-pipelines", server.uri());
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pipelines/"))
        .respond_with(move |req: &Request| {
            let fields = req
                .url
                .query_pairs()
                .find(|(key, _)| key == "fields")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let mut response = json!({"values":[{"uuid":"one"}]});
            if fields.split(',').any(|field| field == "next") {
                response["next"] = json!(next);
            }
            ResponseTemplate::new(200).set_body_json(response)
        })
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/more-pipelines"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"values":[{"uuid":"two"},{"uuid":"three"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let pipelines = client(&server)
        .list_pipelines("ws", "repo", None, 2)
        .await
        .unwrap();
    assert_eq!(pipelines.len(), 2);
    assert_eq!(pipelines[1].uuid, "two");
}
