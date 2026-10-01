//! Git remote identity parsing: no API calls and no external Git configuration.

use assert_cmd::Command;
use bbr::git::{parse_remote_url, RepoIdentity};
use serde_json::Value;
use std::{fs, time::Duration};
use tempfile::tempdir;

#[test]
fn https_identity_requires_the_bitbucket_cloud_host() {
    for remote in [
        "https://github.com/team/repo.git",
        "https://gitlab.com/team/repo.git",
        "http://example.org/team/repo.git",
        "https://bitbucket.org.example.org/team/repo.git",
        "https://bitbucket.org@github.com/team/repo.git",
        "https://user:fake-password@github.com/team/repo.git",
        "https:///team/repo.git",
    ] {
        assert!(
            parse_remote_url(remote).is_none(),
            "unrelated host accepted: {remote}"
        );
    }
}

#[test]
fn accepts_cloud_https_scp_and_explicit_ssh_urls() {
    let expected = Some(RepoIdentity {
        workspace: "team".into(),
        slug: "repo".into(),
    });
    for remote in [
        "https://bitbucket.org/team/repo.git",
        "https://user:fake-password@bitbucket.org/team/repo.git",
        "HTTPS://BITBUCKET.ORG:443/team/repo.git",
        "http://bitbucket.org/team/repo",
        "git@bitbucket.org:team/repo.git",
        "ssh://git@bitbucket.org/team/repo.git",
        "ssh://git@bitbucket.org:22/team/repo.git",
        "ssh://git@altssh.bitbucket.org:443/team/repo.git",
        "git@work-bitbucket:team/repo.git",
        "ssh://git@work-bitbucket/team/repo.git",
    ] {
        assert_eq!(
            parse_remote_url(remote),
            expected,
            "unsupported valid form: {remote}"
        );
    }
}

#[test]
fn rejects_unrelated_ssh_domains_but_retains_single_label_aliases() {
    for remote in [
        "git@github.com:team/repo.git",
        "git@gitlab.com:team/repo.git",
        "git@bitbucket.org.example.org:team/repo.git",
        "ssh://git@github.com/team/repo.git",
        "git@:team/repo.git",
        "git@-option:team/repo.git",
        "ssh://git@127.0.0.1/team/repo.git",
        "ssh://git@[::1]/team/repo.git",
    ] {
        assert!(
            parse_remote_url(remote).is_none(),
            "unsafe SSH host: {remote}"
        );
    }
}

#[test]
fn remote_path_must_be_exactly_two_safe_segments() {
    for path in [
        "team/repo/extra",
        "team/repo.git/",
        "/team/repo",
        "team//repo",
        "team/../repo",
        "team/..",
        "../repo",
        "team/.",
        "./repo",
        "team/repo?query=yes",
        "team/repo#fragment",
        "team/repo?",
        "team/repo#",
        "team/re%2Fpo",
        "team/%2e%2e",
        "team/re%3Fpo",
        "team/repo%00",
        "team/re po",
        "team/re\tpo",
        "team/re\npo",
        "team/re\\po",
        "team/",
        "/repo",
        "team",
        "team/.git",
    ] {
        for prefix in [
            "https://bitbucket.org/",
            "git@bitbucket.org:",
            "ssh://git@bitbucket.org/",
        ] {
            let remote = format!("{prefix}{path}");
            assert!(
                parse_remote_url(&remote).is_none(),
                "unsafe path accepted: {remote:?}"
            );
        }
    }
}

#[test]
fn strips_only_one_git_suffix_and_preserves_valid_slug_characters() {
    assert_eq!(
        parse_remote_url("https://bitbucket.org/team_1/my.repo-name.git.git"),
        Some(RepoIdentity {
            workspace: "team_1".into(),
            slug: "my.repo-name.git".into()
        })
    );
}

#[test]
fn detects_cloud_remote_instead_of_unrelated_origin() {
    let home = tempdir().unwrap();
    let gitconfig = home.path().join("empty-gitconfig");
    fs::write(&gitconfig, "").unwrap();
    let isolate = |cmd: &mut Command| {
        cmd.current_dir(home.path())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path())
            .env("APPDATA", home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_CONFIG_COUNT")
            .timeout(Duration::from_secs(5));
    };
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        isolate(&mut cmd);
        cmd.args(args).assert().success();
    };
    git(&["init", "--quiet"]);
    git(&[
        "config",
        "--local",
        "remote.origin.url",
        "https://github.com/wrong/repo.git",
    ]);
    let bbr = || {
        let mut cmd = Command::cargo_bin("bbr").unwrap();
        isolate(&mut cmd);
        cmd.env("BITBUCKET_USERNAME", "test@example.com")
            .env("BITBUCKET_TOKEN", "fake-token")
            .env("BITBUCKET_API_BASE", "http://127.0.0.1:9")
            .env("NO_COLOR", "1")
            .env_remove("BB_WORKSPACE")
            .env_remove("BB_SLUG")
            .args(["open", "pipelines", "--json"]);
        cmd
    };
    let missing = bbr().output().unwrap();
    assert_eq!(
        missing.status.code(),
        Some(1),
        "GitHub must not become Bitbucket identity"
    );
    let error: Value = serde_json::from_slice(&missing.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "git");
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Bitbucket"));
    assert!(!String::from_utf8_lossy(&missing.stderr).contains("https://github.com"));

    git(&[
        "config",
        "--local",
        "remote.upstream.url",
        "https://bitbucket.org/right/project.git",
    ]);
    let found = bbr().output().unwrap();
    assert!(found.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&found.stdout).unwrap()["url"],
        "https://bitbucket.org/right/project/pipelines"
    );

    git(&[
        "config",
        "--local",
        "remote.origin.url",
        "git@github.com:wrong/repo.git",
    ]);
    let ssh_origin = bbr().output().unwrap();
    assert!(ssh_origin.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&ssh_origin.stdout).unwrap()["url"],
        "https://bitbucket.org/right/project/pipelines"
    );

    git(&[
        "config",
        "--local",
        "remote.origin.url",
        "git@bitbucket.org:preferred/repo.git",
    ]);
    let preferred = bbr().output().unwrap();
    assert!(preferred.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&preferred.stdout).unwrap()["url"],
        "https://bitbucket.org/preferred/repo/pipelines"
    );
}
