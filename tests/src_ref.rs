//! `bbr src` revision handling (mock HTTP only).

use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn bbr(server: &MockServer, args: &[&str]) -> std::process::Output {
    let home = TempDir::new().unwrap();
    assert_cmd::Command::cargo_bin("bbr")
        .unwrap()
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_USERNAME", "test")
        .env("BITBUCKET_TOKEN", "fake-token")
        .env("BITBUCKET_API_BASE", server.uri())
        .env("BB_WORKSPACE", "ws")
        .env("BB_SLUG", "repo")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .args(args)
        .timeout(Duration::from_secs(15))
        .output()
        .unwrap()
}

#[tokio::test]
async fn branch_with_slash_is_resolved_to_its_commit_before_reading_files() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/refs/branches"))
        .and(query_param("q", "name=\"feature/login\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{"name": "feature/login", "target": {"hash": SHA}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repositories/ws/repo/src/{SHA}/Cargo.toml")))
        .respond_with(ResponseTemplate::new(200).set_body_string("[package]\n"))
        .expect(1)
        .mount(&server)
        .await;

    let out = bbr(
        &server,
        &["src", "cat", "Cargo.toml", "--git-ref", "feature/login"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"[package]\n", "piped output stays byte-exact");
    server.verify().await;
}

#[tokio::test]
async fn tag_with_slash_falls_back_to_the_tag_lookup() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/refs/branches"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/refs/tags"))
        .and(query_param("q", "name=\"release/1.0\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{"name": "release/1.0", "target": {"hash": SHA}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repositories/ws/repo/src/{SHA}/")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{"type": "commit_file", "path": "README.md", "size": 3}]
        })))
        .mount(&server)
        .await;

    let out = bbr(
        &server,
        &["src", "ls", "--git-ref", "release/1.0", "--json"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let entries: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(entries[0]["path"], "README.md");
}

#[tokio::test]
async fn plain_ref_needs_no_lookup_and_unknown_slash_ref_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/src/main/a.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("a"))
        .mount(&server)
        .await;
    let out = bbr(&server, &["src", "cat", "a.txt", "--git-ref", "main"]);
    assert!(out.status.success());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/refs/branches"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/refs/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": []})))
        .mount(&server)
        .await;
    let out = bbr(
        &server,
        &["src", "cat", "a.txt", "--git-ref", "no/such", "--json"],
    );
    assert_eq!(out.status.code(), Some(3));
}
