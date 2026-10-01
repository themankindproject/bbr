//! Remote text must never reach the terminal with live escape sequences
//! (OSC 52 clipboard writes, screen clears), while verbatim modes stay exact.

use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OSC52: &str = "\u{1b}]52;c;aGFja2Vk\u{7}";
const CLEAR: &str = "\u{1b}[2J\u{1b}[H";

fn hostile_diff() -> String {
    format!(
        "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1,1 +1,1 @@ fn {CLEAR}header(){OSC52}\n-old {OSC52}\n+new\r\n"
    )
}

async fn mount(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 7,
            "state": "OPEN",
            "title": format!("Title {OSC52}{CLEAR}end"),
            "description": format!("body {OSC52}"),
            "source": {"branch": {"name": "feature"}},
            "destination": {"branch": {"name": "main"}},
            "author": {"display_name": format!("Mallory{CLEAR}")},
            "links": {"html": {"href": "https://bitbucket.org/ws/repo/pull-requests/7"}}
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repositories/ws/repo/pullrequests/7/diff"))
        .respond_with(ResponseTemplate::new(200).set_body_string(hostile_diff()))
        .mount(server)
        .await;
}

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
        .env("CI", "1")
        // Colors on: SGR must survive while every other escape is removed.
        .env("CLICOLOR_FORCE", "1")
        .env_remove("NO_COLOR")
        .args(args)
        .timeout(Duration::from_secs(15))
        .output()
        .unwrap()
}

fn assert_no_live_escapes(text: &str) {
    assert!(!text.contains("\u{1b}]"), "OSC survived: {text:?}");
    assert!(
        !text.contains("\u{1b}[2J") && !text.contains("\u{1b}[H"),
        "screen control survived: {text:?}"
    );
    assert!(!text.contains('\u{7}'), "BEL survived: {text:?}");
}

#[tokio::test]
async fn pr_view_with_diff_sanitizes_title_author_and_hunk_header() {
    let server = MockServer::start().await;
    mount(&server).await;
    let out = bbr(&server, &["pr", "view", "7", "--diff", "--no-pager"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Title") && stdout.contains("end"),
        "{stdout}"
    );
    assert!(stdout.contains("header()"), "{stdout}");
    assert_no_live_escapes(&stdout);
}

#[tokio::test]
async fn pr_diff_render_is_sanitized_but_raw_diff_is_byte_exact() {
    let server = MockServer::start().await;
    mount(&server).await;
    let rendered = bbr(&server, &["pr", "diff", "7"]);
    assert!(rendered.status.success());
    let stdout = String::from_utf8_lossy(&rendered.stdout);
    assert!(stdout.contains("header()"), "{stdout}");
    assert_no_live_escapes(&stdout);

    // `--raw` to a pipe is for `git apply`: every byte, including CRLF, survives.
    let raw = bbr(&server, &["pr", "diff", "7", "--raw"]);
    assert!(raw.status.success());
    assert_eq!(raw.stdout, hostile_diff().as_bytes());
}

#[tokio::test]
async fn json_output_keeps_exact_remote_text() {
    let server = MockServer::start().await;
    mount(&server).await;
    let out = bbr(&server, &["pr", "view", "7", "--json"]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["title"], format!("Title {OSC52}{CLEAR}end"));
}
