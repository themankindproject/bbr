//! Irreversible variable deletes need `--yes` (or an interactive "y"); mock HTTP only.

use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
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

async fn mount_variables(server: &MockServer) {
    let vars =
        json!({"values": [{"uuid": "{v1}", "key": "SECRET", "value": null, "secured": true}]});
    Mock::given(method("GET"))
        .and(path_regex(r"/pipelines_config/variables/?$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vars.clone()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"/deployments_config/environments/.+/variables/?$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(vars))
        .mount(server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;
}

const DELETES: &[&[&str]] = &[
    &["ci", "vars", "delete", "SECRET"],
    &["variable", "delete", "SECRET"],
    &["deploy", "env", "vars", "delete", "{env}", "SECRET"],
];

#[tokio::test]
async fn variable_deletes_refuse_without_yes_before_any_request() {
    let server = MockServer::start().await;
    mount_variables(&server).await;
    let home = TempDir::new().unwrap();
    for args in DELETES {
        let out = bbr(&server, &home, &[args, &["--json"][..]].concat());
        assert_eq!(out.status.code(), Some(64), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("--yes"),
            "{args:?}"
        );
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "nothing may be fetched or deleted without consent"
    );
}

#[tokio::test]
async fn variable_deletes_proceed_with_yes() {
    let server = MockServer::start().await;
    mount_variables(&server).await;
    let home = TempDir::new().unwrap();
    for args in DELETES {
        let out = bbr(&server, &home, &[args, &["--yes", "--json"][..]].concat());
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let receipt: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(receipt["action"], "deleted");
    }
    let deletes = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == "DELETE")
        .count();
    assert_eq!(deletes, DELETES.len());
}
