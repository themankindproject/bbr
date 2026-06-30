//! Integration tests for rate-limit retry, pagination, and send_raw.

use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use bbr::api::pr::{PrState, PullRequest};
use bbr::api::BitbucketClient;
use bbr::auth::{CredentialKind, Credentials};
use bbr::error::ExitCode;

async fn client(base: &str) -> BitbucketClient {
    let creds = Credentials {
        username: "u@example.com".into(),
        secret: "tok".into(),
        kind: CredentialKind::ApiToken,
    };
    BitbucketClient::new(base, creds).unwrap()
}

const AUTH_BASIC: &str = "Basic dUBleGFtcGxlLmNvbTp0b2s=";

// ---------------------------------------------------------------------------
// Rate-limit retry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retries_on_rate_limit_then_succeeds() {
    let server = MockServer::start().await;

    // First two requests return 429
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": { "message": "rate limit exceeded" }
        })))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;

    // Third request succeeds
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "id": 1,
                "title": "PR after retry",
                "state": "OPEN",
                "source": { "branch": { "name": "feat" },
                            "repository": { "name": "slug", "full_name": "ws/slug", "type": "repository" } },
                "destination": { "branch": { "name": "main" },
                                 "repository": { "name": "slug", "full_name": "ws/slug", "type": "repository" } },
                "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/1" } }
            }],
            "pagelen": 25
        })))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let prs = c
        .list_prs(
            "ws",
            "slug",
            PrState::Open,
            25,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].id, 1);
    assert_eq!(prs[0].title, "PR after retry");
}

#[tokio::test]
async fn returns_error_after_max_retries_exhausted() {
    let server = MockServer::start().await;

    // All 3 requests (initial + 2 retries) return 429
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .and(header("authorization", AUTH_BASIC))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": { "message": "rate limit exceeded" }
        })))
        .up_to_n_times(3)
        .expect(3)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .list_prs(
            "ws",
            "slug",
            PrState::Open,
            25,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect_err("should fail after retries");
    assert_eq!(err.exit_code(), ExitCode::RateLimit);
}

#[tokio::test]
async fn retry_on_send_raw_succeeds_after_429() {
    let server = MockServer::start().await;

    // First request returns 429
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests/1/diff"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": { "message": "rate limit exceeded" }
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;

    // Second request succeeds with plain text
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests/1/diff"))
        .respond_with(ResponseTemplate::new(200).set_body_string("diff --git a/foo\n+bar"))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let diff = c
        .send_raw(
            reqwest::Method::GET,
            "/repositories/ws/slug/pullrequests/1/diff",
            "text/plain",
        )
        .await
        .unwrap();
    assert!(diff.contains("diff --git"));
    assert!(diff.contains("+bar"));
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_all_pages_follows_next_links() {
    let server = MockServer::start().await;

    // Page 1 (no page query param)
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                { "id": 1, "title": "PR 1", "state": "OPEN",
                  "source": { "branch": { "name": "a" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/1" } } },
                { "id": 2, "title": "PR 2", "state": "OPEN",
                  "source": { "branch": { "name": "b" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/2" } } }
            ],
            "next": format!("{}/repositories/ws/slug/pullrequests?page=2", server.uri()),
            "pagelen": 2
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Page 2 (with page=2 query param)
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                { "id": 3, "title": "PR 3", "state": "MERGED",
                  "source": { "branch": { "name": "c" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/3" } } }
            ],
            "pagelen": 2
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let all: Vec<PullRequest> = c
        .fetch_all_pages("/repositories/ws/slug/pullrequests", 100)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, 1);
    assert_eq!(all[1].id, 2);
    assert_eq!(all[2].id, 3);
}

#[tokio::test]
async fn fetch_all_pages_respects_limit() {
    let server = MockServer::start().await;

    // Page 1 with 2 items and a next link
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                { "id": 1, "title": "PR 1", "state": "OPEN",
                  "source": { "branch": { "name": "a" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/1" } } },
                { "id": 2, "title": "PR 2", "state": "OPEN",
                  "source": { "branch": { "name": "b" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/2" } } }
            ],
            "next": format!("{}/repositories/ws/slug/pullrequests?page=2", server.uri()),
            "pagelen": 2
        })))
        .mount(&server)
        .await;

    // Page 2 should NOT be fetched because limit=2 is already reached
    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                { "id": 3, "title": "PR 3", "state": "OPEN",
                  "source": { "branch": { "name": "c" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/3" } } }
            ],
            "pagelen": 2
        })))
        .expect(0) // Should NOT be called
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let all: Vec<PullRequest> = c
        .fetch_all_pages("/repositories/ws/slug/pullrequests", 2)
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, 1);
    assert_eq!(all[1].id, 2);
}

#[tokio::test]
async fn fetch_all_pages_single_page_no_next() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [
                { "id": 10, "title": "Only PR", "state": "OPEN",
                  "source": { "branch": { "name": "feat" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "destination": { "branch": { "name": "main" }, "repository": { "name": "s", "full_name": "ws/slug", "type": "repository" } },
                  "links": { "html": { "href": "https://bitbucket.org/ws/slug/pull-requests/10" } } }
            ],
            "pagelen": 25
        })))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let all: Vec<PullRequest> = c
        .fetch_all_pages("/repositories/ws/slug/pullrequests", 100)
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, 10);
}

// ---------------------------------------------------------------------------
// send_raw
// ---------------------------------------------------------------------------

#[tokio::test]
async fn send_raw_returns_body_on_success() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests/1/patch"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("diff --git a/file.rs b/file.rs\n+hello")
                .insert_header("content-type", "text/plain"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let body = c
        .send_raw(
            reqwest::Method::GET,
            "/repositories/ws/slug/pullrequests/1/patch",
            "text/plain",
        )
        .await
        .unwrap();
    assert!(body.contains("diff --git"));
    assert!(body.contains("+hello"));
}

#[tokio::test]
async fn send_raw_returns_error_on_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests/999/diff"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .send_raw(
            reqwest::Method::GET,
            "/repositories/ws/slug/pullrequests/999/diff",
            "text/plain",
        )
        .await
        .expect_err("should fail with 404");
    assert_eq!(err.exit_code(), ExitCode::NotFound);
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn error_envelope_includes_message() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repositories/ws/slug/pullrequests/1"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {
                "message": "access denied",
                "detail": {
                    "required": ["repo:write"],
                    "granted": ["repo:read"]
                }
            }
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .get_pr("ws", "slug", 1)
        .await
        .expect_err("should fail with 403");
    assert_eq!(err.exit_code(), ExitCode::Auth);
    let msg = format!("{err}");
    assert!(msg.contains("access denied"));
    assert!(msg.contains("repo:write"));
    assert!(msg.contains("MISSING"));
}
