//! Updating a secured variable must not silently downgrade it to plaintext.

use serde_json::{json, Value};
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn bbr(server: &MockServer, home: &TempDir, args: &[&str]) -> std::process::Output {
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
async fn updating_a_secured_variable_keeps_it_secured() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pipelines_config/variables/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "uuid": "{v1}",
                "key": "SECRET",
                "value": null,
                "secured": true
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/variables/%7Bv1%7D",
        ))
        .and(body_partial_json(json!({
            "key": "SECRET",
            "value": "rotated",
            "secured": true
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "uuid": "{v1}", "key": "SECRET", "value": null, "secured": true
        })))
        .mount(&server)
        .await;

    let home = TempDir::new().unwrap();
    let out = bbr(
        &server,
        &home,
        &["variable", "set", "SECRET", "--value", "rotated", "--json"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let receipt: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(receipt["action"], "updated");
}

#[tokio::test]
async fn explicit_false_downgrades_a_secured_variable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pipelines_config/variables/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [{
                "uuid": "{v1}", "key": "SECRET", "value": null, "secured": true
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path(
            "/repositories/ws/repo/pipelines_config/variables/%7Bv1%7D",
        ))
        .and(body_partial_json(json!({"secured": false})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "uuid": "{v1}", "key": "SECRET", "value": "x", "secured": false
        })))
        .mount(&server)
        .await;

    let home = TempDir::new().unwrap();
    let out = bbr(
        &server,
        &home,
        &[
            "variable",
            "set",
            "SECRET",
            "--value",
            "x",
            "--secured=false",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
