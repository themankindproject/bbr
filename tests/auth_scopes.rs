//! Auth scope names and credential-source reporting stay truthful.

use std::time::Duration;
use tempfile::TempDir;

#[test]
fn setup_scope_list_matches_the_readme_scope_table() {
    let readme = include_str!("../README.md");
    for (scope, _) in bbr::commands::auth::TOKEN_SCOPES {
        assert!(
            readme.contains(&format!("`{scope}`")),
            "auth setup lists {scope}, which the README scope table does not document"
        );
        let parts: Vec<&str> = scope.split(':').collect();
        assert_eq!(
            parts.len(),
            3,
            "{scope} is not <action>:<resource>:bitbucket"
        );
        assert!(
            ["read", "write", "delete", "admin"].contains(&parts[0]),
            "{scope}"
        );
        assert_eq!(parts[2], "bitbucket", "{scope}");
    }
    // Every scope the README documents is offered at setup time.
    for line in readme.lines().filter(|l| l.starts_with("| `")) {
        for scope in line.split('`').skip(1).step_by(2) {
            if scope.ends_with(":bitbucket") {
                assert!(
                    bbr::commands::auth::TOKEN_SCOPES
                        .iter()
                        .any(|(s, _)| *s == scope),
                    "README documents {scope}, but auth setup does not list it"
                );
            }
        }
    }
}

#[test]
fn partial_environment_credentials_are_reported_as_config_file_source() {
    let home = TempDir::new().unwrap();
    let cfg = home.path().join("bbr");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("credentials.toml"),
        "[default]\nusername = \"file-user@example.com\"\ntoken = \"file-token\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            cfg.join("credentials.toml"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    // BITBUCKET_TOKEN is set but unusable without a username, so the file is
    // what actually authenticates. `doctor` reports it without network access.
    let out = assert_cmd::Command::cargo_bin("bbr")
        .unwrap()
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("APPDATA", home.path())
        .env("BITBUCKET_TOKEN", "env-token")
        .env_remove("BITBUCKET_USERNAME")
        .env("BITBUCKET_API_BASE", "http://127.0.0.1:9")
        .env("CI", "1")
        .env("NO_COLOR", "1")
        .args(["doctor", "--json"])
        .timeout(Duration::from_secs(30))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {text}"));
    let creds = report
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "credentials")
        .unwrap_or_else(|| panic!("no credentials check in {text}"));
    let detail = creds["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("file-user@example.com"), "{detail}");
    assert!(detail.contains("config file"), "{detail}");
}
