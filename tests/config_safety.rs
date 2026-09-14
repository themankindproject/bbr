//! Configuration safety checks using throwaway homes and fake credentials.

use assert_cmd::Command;
use std::fs;
use tempfile::{tempdir, TempDir};

fn bbr(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("bbr").unwrap();
    cmd.current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_API_BASE", "http://127.0.0.1:9")
        .env("NO_COLOR", "1")
        .env_remove("BITBUCKET_USERNAME")
        .env_remove("BITBUCKET_TOKEN")
        .env_remove("BB_WORKSPACE")
        .env_remove("BB_SLUG");
    cmd
}

#[test]
fn credential_parse_errors_never_echo_file_contents() {
    let secret = "FAKE_SECRET_MUST_NOT_APPEAR";
    let cases = [
        // Syntax error: the parser's display includes the offending source line.
        format!("[default]\nusername = \"test\"\ntoken = \"{secret}\n"),
        // A semantic error on an inline table can echo the whole credential.
        format!("default = {{ username = 123, token = \"{secret}\" }}\n"),
        // Even Error::message() can contain input, e.g. duplicate key names.
        format!("[default]\nusername = \"test\"\n{secret} = 1\n{secret} = 2\n"),
    ];
    for raw in cases {
        let home = tempdir().unwrap();
        let dir = home.path().join("bbr");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("credentials.toml"), raw).unwrap();
        for json in [false, true] {
            let mut cmd = bbr(&home);
            cmd.args(["config", "show"]);
            if json {
                cmd.arg("--json");
            }
            let output = cmd.output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                !stderr.contains(secret),
                "credential content leaked to stderr"
            );
            assert!(stderr.contains("credentials.toml"));
            assert!(stderr.contains("line"), "retain a safe error location");
            if json {
                let error: serde_json::Value = serde_json::from_str(&stderr).unwrap();
                assert_eq!(error["error"]["kind"], "config");
                assert_eq!(error["error"]["exit_code"], 1);
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn config_save_replaces_file_without_truncating_existing_readers() {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;

    let home = tempdir().unwrap();
    let dir = home.path().join("bbr");
    fs::create_dir(&dir).unwrap();
    let path = dir.join("config.toml");
    let original =
        "[ui]\ntheme = \"dark\"\n\n[contexts.work]\nworkspace = \"team\"\nslug = \"repo\"\n";
    fs::write(&path, original).unwrap();
    let mut existing_reader = fs::File::open(&path).unwrap();

    bbr(&home)
        .args(["config", "set", "ui.theme", "light", "--json"])
        .assert()
        .success();

    let mut old_contents = String::new();
    existing_reader.read_to_string(&mut old_contents).unwrap();
    assert_eq!(
        old_contents, original,
        "saving must replace, not truncate, the live file"
    );
    let saved: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["ui"]["theme"].as_str(), Some("light"));
    assert_eq!(
        saved["contexts"]["work"]["workspace"].as_str(),
        Some("team")
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_dir(dir).unwrap().count(),
        1,
        "no temporary file remains"
    );
}

#[test]
fn config_save_failure_leaves_existing_directory_intact() {
    let home = tempdir().unwrap();
    let path = home.path().join("bbr/config.toml");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("keep"), "unchanged").unwrap();

    bbr(&home)
        .args(["config", "set", "ui.theme", "light", "--json"])
        .assert()
        .code(1);

    assert_eq!(fs::read_to_string(path.join("keep")).unwrap(), "unchanged");
}
