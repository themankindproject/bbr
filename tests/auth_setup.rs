//! Authentication UX and safety without using real credentials or remote APIs.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::time::Duration;
use tempfile::{tempdir, TempDir};

const TOKEN: &str = "fake-token-for-auth-tests";

fn bbr(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("bbr").unwrap();
    cmd.current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_API_BASE", "http://127.0.0.1:9")
        .env("NO_COLOR", "1")
        .env("BBR_NO_INTERACTIVE", "1")
        .env_remove("BITBUCKET_USERNAME")
        .env_remove("BITBUCKET_TOKEN")
        .timeout(Duration::from_secs(5));
    cmd
}

fn existing_credentials(home: &TempDir) -> (std::path::PathBuf, String) {
    let dir = home.path().join("bbr");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("credentials.toml");
    let original =
        "[default]\nusername = \"old-user\"\ntoken = \"old-token\"\nworkspace = \"team\"\n";
    fs::write(&path, original).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    (path, original.into())
}

#[test]
fn blank_credentials_are_rejected_without_overwriting_existing_file() {
    for (username, token) in [("   ", TOKEN), ("test@example.com", " \t\n")] {
        let home = tempdir().unwrap();
        let (path, original) = existing_credentials(&home);
        bbr(&home)
            .args([
                "auth",
                "setup",
                "--username",
                username,
                "--token",
                token,
                "--json",
            ])
            .assert()
            .code(64)
            .stdout("")
            .stderr(predicate::str::contains(TOKEN).not());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
}

#[test]
fn setup_json_reports_saved_not_authenticated_and_never_echoes_token() {
    for args in [
        vec![
            "--json",
            "auth",
            "setup",
            "--username",
            " test@example.com ",
            "--token",
            TOKEN,
        ],
        vec![
            "auth",
            "setup",
            "--username",
            " test@example.com ",
            "--token",
            TOKEN,
            "--json",
        ],
    ] {
        let home = tempdir().unwrap();
        let output = bbr(&home).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.contains(TOKEN));
        let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(value["saved"], true);
        assert_eq!(value["username"], "test@example.com");
        assert_eq!(
            value["path"],
            home.path()
                .join("bbr/credentials.toml")
                .display()
                .to_string()
        );
        assert!(
            value.get("authenticated").is_none(),
            "setup does not verify the token online"
        );
    }
}

#[test]
fn setup_stdin_saves_trimmed_token_and_preserves_workspace() {
    let home = tempdir().unwrap();
    let (path, _) = existing_credentials(&home);
    let output = bbr(&home)
        .args([
            "auth",
            "setup",
            "--username",
            "test@example.com",
            "--token-stdin",
            "--json",
        ])
        .write_stdin(format!("  {TOKEN}\n"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(TOKEN));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdout).unwrap()["saved"],
        true
    );
    let saved: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["default"]["token"].as_str(), Some(TOKEN));
    assert_eq!(saved["default"]["workspace"].as_str(), Some("team"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn setup_stdin_rejects_empty_or_oversized_input_without_overwriting() {
    for (input, expected_error) in [
        (b" \n".to_vec(), "API token must not be empty"),
        (vec![b'x'; 65537], "token input exceeds 64 KiB"),
        (vec![0xff, 0xfe], "token input must be UTF-8 text"),
    ] {
        let home = tempdir().unwrap();
        let (path, original) = existing_credentials(&home);
        bbr(&home)
            .args([
                "auth",
                "setup",
                "--username",
                "test@example.com",
                "--token-stdin",
                "--json",
            ])
            .write_stdin(input)
            .assert()
            .code(64)
            .stdout("")
            .stderr(predicate::str::contains(expected_error));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
}

#[test]
fn setup_without_arguments_never_prompts_when_noninteractive() {
    for json in [false, true] {
        let home = tempdir().unwrap();
        let mut cmd = bbr(&home);
        cmd.args(["auth", "setup"]);
        if json {
            cmd.arg("--json");
        }
        let output = cmd.write_stdin("").output().unwrap();
        assert_eq!(output.status.code(), Some(64));
        assert!(
            output.stdout.is_empty(),
            "no interactive banner in noninteractive mode"
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("--token-stdin"));
        assert!(!stderr.contains("Bitbucket username (email):"));
        if json {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&stderr).unwrap()["error"]["kind"],
                "usage"
            );
        }
        assert!(!home.path().join("bbr/credentials.toml").exists());
    }
}

#[test]
fn setup_stdin_requires_username_and_conflicts_with_token_argument() {
    for args in [
        vec!["auth", "setup", "--token-stdin"],
        vec![
            "auth",
            "setup",
            "--username",
            "test",
            "--token",
            TOKEN,
            "--token-stdin",
        ],
    ] {
        let home = tempdir().unwrap();
        bbr(&home).args(args).assert().code(64);
        assert!(!home.path().join("bbr/credentials.toml").exists());
    }
}

#[test]
fn auth_status_reports_corrupt_credentials_as_an_error() {
    let home = tempdir().unwrap();
    let (path, _) = existing_credentials(&home);
    fs::write(
        path,
        format!("[default]\nusername = \"test\"\ntoken = \"{TOKEN}\n"),
    )
    .unwrap();
    let output = bbr(&home)
        .args(["auth", "status", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains(TOKEN));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stderr).unwrap()["error"]["kind"],
        "config"
    );
}

#[test]
fn auth_status_reports_unreadable_credentials_as_an_error() {
    let home = tempdir().unwrap();
    fs::create_dir_all(home.path().join("bbr/credentials.toml")).unwrap();
    bbr(&home)
        .args(["auth", "status", "--json"])
        .assert()
        .code(1)
        .stdout("");
}

#[test]
fn setup_stdin_accepts_the_documented_size_boundary() {
    let home = tempdir().unwrap();
    let token = "x".repeat(65536);
    bbr(&home)
        .args([
            "auth",
            "setup",
            "--username",
            "test",
            "--token-stdin",
            "--json",
        ])
        .write_stdin(token.clone())
        .assert()
        .success();
    let saved: toml::Value =
        toml::from_str(&fs::read_to_string(home.path().join("bbr/credentials.toml")).unwrap())
            .unwrap();
    assert_eq!(saved["default"]["token"].as_str(), Some(token.as_str()));
}

#[test]
fn auth_status_without_credentials_still_succeeds() {
    let home = tempdir().unwrap();
    let output = bbr(&home)
        .args(["auth", "status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["authenticated"], false);
    assert_eq!(value["source"], "none");
}

#[test]
fn setup_schema_describes_the_saved_receipt() {
    let home = tempdir().unwrap();
    let output = bbr(&home).args(["schema", "auth-setup"]).output().unwrap();
    assert!(output.status.success());
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        schema["required"],
        serde_json::json!(["saved", "username", "path"])
    );
    assert_eq!(schema["properties"]["saved"]["type"], "boolean");
    assert!(schema["properties"].get("token").is_none());
}

#[test]
fn setup_help_lists_safe_stdin_option() {
    let home = tempdir().unwrap();
    bbr(&home)
        .args(["auth", "setup", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--token-stdin"));
}
