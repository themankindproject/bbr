//! Conditional request correctness and diagnostic privacy, with local mock servers.

use bbr::api::BitbucketClient;
use bbr::auth::Credentials;
use reqwest::Method;
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
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

async fn cached_json(server: &MockServer, c: &BitbucketClient) {
    Mock::given(method("GET"))
        .and(path("/item"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"old\"")
                .set_body_json(json!({"old": true})),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(server)
        .await;
    assert_eq!(
        c.send::<Value>(Method::GET, "/item", None).await.unwrap(),
        json!({"old":true})
    );
}

#[tokio::test]
async fn conditional_get_reuses_body_when_server_returns_304() {
    let server = MockServer::start().await;
    let c = client(&server);
    cached_json(&server, &c).await;
    Mock::given(method("GET"))
        .and(path("/item"))
        .and(header("if-none-match", "\"old\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        c.send::<Value>(Method::GET, "/item", None).await.unwrap(),
        json!({"old":true})
    );
}

#[tokio::test]
async fn accept_representations_have_independent_validators_and_bodies() {
    let server = MockServer::start().await;
    let c = client(&server);
    cached_json(&server, &c).await;
    Mock::given(method("GET"))
        .and(path("/item"))
        .and(header("accept", "text/plain"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"text\"")
                .set_body_string("plain text"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    assert_eq!(
        c.send_raw(Method::GET, "/item", "text/plain")
            .await
            .unwrap(),
        "plain text"
    );
    let requests = server.received_requests().await.unwrap();
    assert!(
        !requests
            .last()
            .unwrap()
            .headers
            .contains_key("if-none-match"),
        "JSON validator sent for a text representation"
    );
    Mock::given(method("GET"))
        .and(path("/item"))
        .and(header("accept", "application/json"))
        .and(header("if-none-match", "\"old\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        c.send::<Value>(Method::GET, "/item", None).await.unwrap(),
        json!({"old":true})
    );
    Mock::given(method("GET"))
        .and(path("/item"))
        .and(header("accept", "text/plain"))
        .and(header("if-none-match", "\"text\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        c.send_raw(Method::GET, "/item", "text/plain")
            .await
            .unwrap(),
        "plain text"
    );
}

#[tokio::test]
async fn successful_mutations_invalidate_all_get_variants_without_caching_mutation_bodies() {
    for mode in ["json", "empty", "raw"] {
        let server = MockServer::start().await;
        let c = client(&server);
        cached_json(&server, &c).await;
        Mock::given(method("POST"))
            .and(path("/mutation"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"mutation\"")
                    .set_body_json(json!({"receipt":true})),
            )
            .expect(1)
            .mount(&server)
            .await;
        match mode {
            "json" => {
                c.send::<Value>(Method::POST, "/mutation", Some("{}"))
                    .await
                    .unwrap();
            }
            "empty" => {
                c.send_empty(Method::POST, "/mutation", Some("{}"))
                    .await
                    .unwrap();
            }
            _ => {
                c.send_raw(Method::POST, "/mutation", "application/json")
                    .await
                    .unwrap();
            }
        }
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"fresh":true})))
            .mount(&server)
            .await;
        c.send::<Value>(Method::GET, "/item", None).await.unwrap();
        c.send::<Value>(Method::GET, "/mutation", None)
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        assert!(
            !requests[2].headers.contains_key("if-none-match"),
            "{mode}: stale validator after mutation"
        );
        assert!(
            !requests[3].headers.contains_key("if-none-match"),
            "{mode}: mutation receipt was cached as a GET"
        );
    }
}

#[tokio::test]
async fn failed_mutations_also_invalidate_because_the_server_may_have_applied_them() {
    let server = MockServer::start().await;
    let c = client(&server);
    cached_json(&server, &c).await;
    Mock::given(method("POST"))
        .and(path("/item"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    assert!(c.send_empty(Method::POST, "/item", None).await.is_err());
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"fresh":true})))
        .mount(&server)
        .await;
    c.send::<Value>(Method::GET, "/item", None).await.unwrap();
    assert!(!server
        .received_requests()
        .await
        .unwrap()
        .last()
        .unwrap()
        .headers
        .contains_key("if-none-match"));
}

#[tokio::test]
async fn full_response_without_etag_discards_the_old_validator() {
    let server = MockServer::start().await;
    let c = client(&server);
    cached_json(&server, &c).await;
    Mock::given(method("GET"))
        .and(path("/item"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"fresh":true})))
        .mount(&server)
        .await;
    c.send::<Value>(Method::GET, "/item", None).await.unwrap();
    c.send::<Value>(Method::GET, "/item", None).await.unwrap();
    assert!(!server
        .received_requests()
        .await
        .unwrap()
        .last()
        .unwrap()
        .headers
        .contains_key("if-none-match"));
}

#[tokio::test]
async fn no_store_and_vary_star_responses_are_not_cached() {
    for (name, value) in [("cache-control", "private, no-store"), ("vary", "*")] {
        let server = MockServer::start().await;
        let c = client(&server);
        Mock::given(method("GET"))
            .and(path("/item"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"sensitive\"")
                    .insert_header(name, value)
                    .set_body_json(json!({"value":true})),
            )
            .mount(&server)
            .await;
        c.send::<Value>(Method::GET, "/item", None).await.unwrap();
        c.send::<Value>(Method::GET, "/item", None).await.unwrap();
        assert!(
            !server
                .received_requests()
                .await
                .unwrap()
                .last()
                .unwrap()
                .headers
                .contains_key("if-none-match"),
            "cached despite {name}: {value}"
        );
    }
}

#[tokio::test]
async fn get_requests_with_bodies_bypass_the_path_only_cache() {
    let server = MockServer::start().await;
    let c = client(&server);
    cached_json(&server, &c).await;
    Mock::given(method("GET"))
        .and(path("/item"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"body-result\"")
                .set_body_json(json!({"body_result":true})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    c.send::<Value>(Method::GET, "/item", Some("{\"q\":1}"))
        .await
        .unwrap();
    assert!(!server
        .received_requests()
        .await
        .unwrap()
        .last()
        .unwrap()
        .headers
        .contains_key("if-none-match"));
    Mock::given(method("GET"))
        .and(path("/item"))
        .and(header("if-none-match", "\"old\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        c.send::<Value>(Method::GET, "/item", None).await.unwrap(),
        json!({"old":true})
    );
}

#[tokio::test]
async fn mutation_cannot_succeed_via_a_304_cached_get_body() {
    for mode in ["json", "empty", "raw"] {
        let server = MockServer::start().await;
        let c = client(&server);
        cached_json(&server, &c).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&server)
            .await;
        let failed = match mode {
            "json" => c.send::<Value>(Method::POST, "/item", None).await.is_err(),
            "empty" => c.send_empty(Method::POST, "/item", None).await.is_err(),
            _ => c
                .send_raw(Method::POST, "/item", "text/plain")
                .await
                .is_err(),
        };
        assert!(failed, "{mode} mutation incorrectly succeeded on HTTP 304");
    }
}

#[tokio::test]
async fn in_flight_304_uses_request_snapshot_after_mutation_invalidation() {
    for raw in [false, true] {
        let server = MockServer::start().await;
        let c = client(&server);
        cached_json(&server, &c).await;
        let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
        let notify = arrived.clone();
        Mock::given(method("GET"))
            .and(path("/item"))
            .and(header("if-none-match", "\"old\""))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(304).set_delay(std::time::Duration::from_millis(100))
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let clone = c.clone();
        let in_flight = tokio::spawn(async move {
            if raw {
                let text = clone
                    .send_raw(Method::GET, "/item", "application/json")
                    .await?;
                Ok::<Value, bbr::BitbucketError>(serde_json::from_str(&text)?)
            } else {
                clone.send::<Value>(Method::GET, "/item", None).await
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), arrived.notified())
            .await
            .unwrap();
        c.send_empty(Method::POST, "/mutation", None).await.unwrap();
        assert_eq!(in_flight.await.unwrap().unwrap(), json!({"old":true}));
    }
}

#[tokio::test]
async fn unexpected_304_without_validator_is_an_error() {
    let server = MockServer::start().await;
    let c = client(&server);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&server)
        .await;
    assert!(c
        .send::<Value>(Method::GET, "/missing", None)
        .await
        .is_err());
    assert!(c
        .send_raw(Method::GET, "/missing", "text/plain")
        .await
        .is_err());
    assert!(c.send_empty(Method::GET, "/missing", None).await.is_err());
}

#[tokio::test]
async fn verbose_json_errors_do_not_print_response_bodies_or_query_values() {
    let server = MockServer::start().await;
    let secret = "FAKE_RESPONSE_SECRET";
    Mock::given(path("/bad-json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(format!("{{\"token\":\"{secret}\"")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let home = tempfile::tempdir().unwrap();
    let mut cmd = assert_cmd::Command::cargo_bin("bbr").unwrap();
    let output = cmd
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_USERNAME", "test")
        .env("BITBUCKET_TOKEN", "fake-token")
        .env("BITBUCKET_API_BASE", server.uri())
        .env("RUST_LOG", "debug")
        .args([
            "api",
            "GET",
            "/bad-json?private=FAKE_QUERY_SECRET",
            "--json",
            "-vv",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains(secret), "debug leaked API body");
    assert!(
        !stderr.contains("FAKE_QUERY_SECRET"),
        "debug leaked query values"
    );
    assert!(stderr.contains("JSON decode failed"));
}
