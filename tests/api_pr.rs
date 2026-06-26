//! Integration tests for PR API endpoints against a mock server.

use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use bbr::api::pr::{PrState, PullRequest};
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
async fn lists_open_prs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "size": 1, "page": 1, "pagelen": 25,
            "values": [{
                "id": 467,
                "title": "Fix X",
                "state": "OPEN",
                "source": { "branch": { "name": "feat/x" },
                            "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
                "destination": { "branch": { "name": "main" },
                                 "repository": { "name": "bvrm", "full_name": "sdadev/bvrm", "type": "repository" } },
                "links": { "html": { "href": "https://bitbucket.org/sdadev/bvrm/pull-requests/467" } },
                "author": { "display_name": "Ash", "role": "AUTHOR", "approved": false }
            }]
        })))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let page = c
        .list_prs("sdadev", "bvrm", PrState::Open, 25)
        .await
        .unwrap();
    assert_eq!(page.values.len(), 1);
    let pr: &PullRequest = &page.values[0];
    assert_eq!(pr.id, 467);
    assert_eq!(pr.state, "OPEN");
    assert_eq!(pr.source.branch.as_ref().unwrap().name, "feat/x");
    assert_eq!(
        pr.web_url(),
        Some("https://bitbucket.org/sdadev/bvrm/pull-requests/467")
    );
}

#[tokio::test]
async fn auth_failure_maps_to_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/1"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .get_pr("sdadev", "bvrm", 1)
        .await
        .expect_err("should be an error");
    assert_eq!(err.exit_code(), bbr::error::ExitCode::Auth);
}

#[tokio::test]
async fn not_found_maps_correctly() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/sdadev/bvrm/pullrequests/9999"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let c = client(&server.uri()).await;
    let err = c
        .get_pr("sdadev", "bvrm", 9999)
        .await
        .expect_err("should be 404");
    assert_eq!(err.exit_code(), bbr::error::ExitCode::NotFound);
}
