//! Regression tests for `--state all` and paginated diffstat.

use bbr::api::pr::PrState;
use bbr::api::BitbucketClient;
use bbr::auth::Credentials;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(base: &str) -> BitbucketClient {
    BitbucketClient::new(
        base,
        Credentials {
            username: "test".into(),
            secret: "fake".into(),
        },
    )
    .unwrap()
}

#[tokio::test]
async fn state_all_queries_every_state_and_merges_the_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests"))
        .respond_with(|req: &Request| {
            let q = req
                .url
                .query_pairs()
                .find(|(k, _)| k == "q")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let (id, state, updated) = if q.contains("MERGED") {
                (2, "MERGED", "2026-02-01T00:00:00Z")
            } else if q.contains("DECLINED") {
                (3, "DECLINED", "2026-03-01T00:00:00Z")
            } else {
                (1, "OPEN", "2026-01-01T00:00:00Z")
            };
            ResponseTemplate::new(200).set_body_json(json!({
                "size": 1,
                "values": [{
                    "id": id,
                    "state": state,
                    "updated_on": updated,
                    "source": {"branch": {"name": "feature"}},
                    "destination": {"branch": {"name": "main"}}
                }]
            }))
        })
        .mount(&server)
        .await;

    let prs = client(&server.uri())
        .list_prs("ws", "repo", PrState::All, 10, None, None, None, None, None)
        .await
        .unwrap();

    let ids: Vec<u64> = prs.iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![3, 2, 1], "all states, newest updated first");
}

#[tokio::test]
async fn diffstat_follows_next_links() {
    let server = MockServer::start().await;
    let next = format!("{}/more-diffstat", server.uri());
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests/7/diffstat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 2,
            "values": [{"status": "modified", "new": {"path": "a.rs"}}],
            "next": next
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/more-diffstat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 2,
            "values": [{"status": "added", "new": {"path": "b.rs"}}]
        })))
        .mount(&server)
        .await;

    let stat = client(&server.uri())
        .pr_diffstat("ws", "repo", 7)
        .await
        .unwrap();
    assert_eq!(stat["size"], 2);
    assert_eq!(stat["values"].as_array().unwrap().len(), 2);
    assert_eq!(stat["values"][1]["new"]["path"], "b.rs");
}
