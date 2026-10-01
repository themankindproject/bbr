//! Keep installation examples tied to this repository rather than the crates.io namesake.

#[test]
fn binary_install_examples_never_rely_on_bare_crates_io_lookup() {
    for text in [
        include_str!("../README.md"),
        include_str!("../docs/distribution.md"),
    ] {
        let mut in_code = false;
        for line in text.lines() {
            if line.starts_with("```") {
                in_code = !in_code;
                continue;
            }
            if in_code || line.starts_with('|') {
                assert!(
                    !line.contains("cargo binstall bbr"),
                    "bare binstall recommendation: {line}"
                );
            }
        }
    }
}

#[test]
fn binstall_metadata_matches_release_names_and_disables_unrelated_fallbacks() {
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    let meta = &manifest["package"]["metadata"]["binstall"];
    assert_eq!(
        meta["pkg-url"].as_str().unwrap(),
        "{ repo }/releases/download/v{ version }/{ name }-{ target }.tar.gz"
    );
    assert_eq!(
        meta["overrides"]["x86_64-pc-windows-msvc"]["pkg-url"]
            .as_str()
            .unwrap(),
        "{ repo }/releases/download/v{ version }/{ name }-{ target }.zip"
    );
    let disabled = meta["disabled-strategies"]
        .as_array()
        .expect("no quick-install/registry compile fallback");
    assert!(disabled.iter().any(|v| v.as_str() == Some("quick-install")));
    assert!(disabled.iter().any(|v| v.as_str() == Some("compile")));
}

#[test]
fn readme_does_not_use_unrelated_license_or_static_test_count_badges() {
    let readme = include_str!("../README.md");
    assert!(!readme.contains("img.shields.io/crates/l/bbr"));
    assert!(!readme.contains("img.shields.io/badge/tests-"));
}

#[test]
fn auth_reference_uses_api_token_scope_names() {
    for doc in [include_str!("../README.md"), include_str!("../USAGE.md")] {
        for old in [
            "`account:read`",
            "`repository:read`",
            "`pullrequest:write`",
            "`pipeline:write`",
        ] {
            assert!(!doc.contains(old), "obsolete scope {old}");
        }
        assert!(doc.contains("read:user:bitbucket"));
        assert!(doc.contains("api-token-permissions"));
    }
}
