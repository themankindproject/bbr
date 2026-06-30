//! `bbr update` — self-update and version-notification subsystem.
//!
//! Checks GitHub releases for a newer version, downloads and installs
//! the binary, and provides a lightweight background check for the
//! default `bbr` / `bbr status` command path.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::cli::GlobalArgs;
use crate::error::{BitbucketError, Result};
use crate::output::theme::Theme;
use crate::output::Formatter;

// ---------------------------------------------------------------------------
// GitHub API types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[allow(dead_code)]
    body: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

// ---------------------------------------------------------------------------
// Output model
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
pub struct UpdateOut {
    pub current_version: String,
    pub latest_version: String,
    pub up_to_date: bool,
    pub release_url: Option<String>,
    pub install_hint: Option<String>,
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, serde::Serialize)]
struct UpdateCache {
    last_check_epoch: u64,
    latest_version: String,
    release_url: String,
}

const CACHE_TTL_SECS: u64 = 86400; // 24 hours

fn cache_path() -> Option<PathBuf> {
    let dir = crate::config::config_dir()?;
    Some(dir.join("update-check.json"))
}

fn read_cache() -> Option<UpdateCache> {
    let path = cache_path()?;
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_cache(cache: &UpdateCache) {
    if let Some(path) = cache_path() {
        if let Ok(data) = serde_json::to_string(cache) {
            let _ = fs::write(path, data);
        }
    }
}

fn cache_is_fresh() -> bool {
    read_cache()
        .map(|c| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub(c.last_check_epoch) < CACHE_TTL_SECS
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Version comparison
// ---------------------------------------------------------------------------

fn parse_version(tag: &str) -> Option<Vec<u64>> {
    let s = tag.strip_prefix('v').unwrap_or(tag);
    s.split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => latest != current,
    }
}

// ---------------------------------------------------------------------------
// Target triple detection
// ---------------------------------------------------------------------------

fn current_target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "windows") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

fn asset_name() -> Option<String> {
    let target = current_target()?;
    Some(format!("bbr-{target}.tar.gz"))
}

// ---------------------------------------------------------------------------
// Install path detection
// ---------------------------------------------------------------------------

fn install_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let candidates = [
            PathBuf::from(&home).join(".local").join("bin"),
            PathBuf::from(&home).join("bin"),
        ];
        for d in &candidates {
            if d.is_dir() {
                return Some(d.clone());
            }
        }
        // Create ~/.local/bin if it doesn't exist
        let local_bin = PathBuf::from(&home).join(".local").join("bin");
        if fs::create_dir_all(&local_bin).is_ok() {
            return Some(local_bin);
        }
    }
    if Path::new("/usr/local/bin").is_dir() {
        return Some(PathBuf::from("/usr/local/bin"));
    }
    None
}

// ---------------------------------------------------------------------------
// GitHub API helpers
// ---------------------------------------------------------------------------

const GITHUB_API: &str = "https://api.github.com/repos/themankindproject/bbr/releases/latest";
const USER_AGENT: &str = concat!("bbr-update/", env!("CARGO_PKG_VERSION"));

/// Shared HTTP client for update checks (reused across calls).
fn update_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build update HTTP client")
    })
}

async fn fetch_latest_release() -> Result<GithubRelease> {
    let resp = update_client()
        .get(GITHUB_API)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(BitbucketError::Http)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(BitbucketError::Other(format!(
            "GitHub API returned {status}: {body:.200}"
        )));
    }

    resp.json().await.map_err(BitbucketError::Http)
}

// ---------------------------------------------------------------------------
// Background update check (printed to stderr, never fatal)
// ---------------------------------------------------------------------------

pub async fn notify_if_outdated() {
    // Skip in CI / automation environments
    if std::env::var("CI").is_ok()
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
        || std::env::var("TF_BUILD").is_ok()
        || std::env::var("BATCH").is_ok()
    {
        return;
    }

    // Only check once per cache TTL
    if cache_is_fresh() {
        return;
    }

    let release = match fetch_latest_release().await {
        Ok(r) => r,
        Err(_) => return,
    };

    let latest = release.tag_name.trim().to_string();
    let current = env!("CARGO_PKG_VERSION");

    write_cache(&UpdateCache {
        last_check_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        latest_version: latest.clone(),
        release_url: format!("https://github.com/themankindproject/bbr/releases/tag/{latest}"),
    });

    if !is_newer(&latest, current) {
        return;
    }

    let _ = writeln!(
        io::stderr(),
        "\n  A newer version of bbr is available: {} (current: {})",
        latest,
        current,
    );
    let _ = writeln!(
        io::stderr(),
        "  Run `bbr update` to upgrade automatically.\n"
    );
}

// ---------------------------------------------------------------------------
// `bbr update` command
// ---------------------------------------------------------------------------

pub async fn run(g: &GlobalArgs, check_only: bool) -> Result<()> {
    let loading = crate::commands::make_spinner(g.json);
    loading.set_message("Checking for updates...");

    let release = fetch_latest_release().await?;
    let current = env!("CARGO_PKG_VERSION");
    let latest = release.tag_name.trim().to_string();

    if !is_newer(&latest, current) {
        loading.finish_and_clear();
        let out = UpdateOut {
            current_version: current.to_string(),
            latest_version: latest,
            up_to_date: true,
            release_url: None,
            install_hint: None,
        };
        let human = render_update(&out);
        return Formatter::from_json_flag(g.json).print(&out, &human);
    }

    loading.finish_and_clear();

    if check_only {
        let out = UpdateOut {
            current_version: current.to_string(),
            latest_version: latest,
            up_to_date: false,
            release_url: Some(format!(
                "https://github.com/themankindproject/bbr/releases/tag/{}",
                release.tag_name.trim()
            )),
            install_hint: Some("Run `bbr update` to install.".into()),
        };
        let human = render_update(&out);
        return Formatter::from_json_flag(g.json).print(&out, &human);
    }

    let loading = crate::commands::make_spinner(g.json);
    loading.set_message(format!("Updating bbr {} → {}...", current, latest));

    download_and_install(&release, &latest).await?;

    loading.finish_and_clear();

    eprintln!("✓  Updated bbr to {latest}");

    write_cache(&UpdateCache {
        last_check_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        latest_version: latest,
        release_url: String::new(),
    });

    Ok(())
}

fn render_update(out: &UpdateOut) -> String {
    let theme = Theme::current();
    let mut s = String::new();

    if out.up_to_date {
        let _ = std::fmt::Write::write_fmt(
            &mut s,
            format_args!(
                "{}  bbr {} — up to date\n",
                theme.success("✓"),
                out.current_version
            ),
        );
    } else {
        let _ = std::fmt::Write::write_fmt(
            &mut s,
            format_args!(
                "{}  New version available: {} (current: {})\n",
                theme.warn("!"),
                out.latest_version,
                out.current_version
            ),
        );
        if let Some(url) = &out.release_url {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("   Release: {url}\n"));
        }
        if let Some(hint) = &out.install_hint {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!("   {hint}\n"));
        }
    }

    s
}

// ---------------------------------------------------------------------------
// Download + extract helper
// ---------------------------------------------------------------------------

async fn download_and_install(release: &GithubRelease, _latest: &str) -> Result<()> {
    let target_name = asset_name().ok_or_else(|| {
        BitbucketError::Other(format!(
            "Unsupported platform: {}-{}",
            std::env::consts::ARCH,
            std::env::consts::OS,
        ))
    })?;

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == target_name)
        .ok_or_else(|| {
            BitbucketError::Other(format!("No release asset found for {target_name}"))
        })?;

    let dest_dir = install_dir().ok_or_else(|| {
        BitbucketError::Other(
            "Cannot determine install directory. Try: \
             `curl -fsSL https://raw.githubusercontent.com/themankindproject/bbr/main/install.sh | sh`"
                .into(),
        )
    })?;
    let dest_path = dest_dir.join("bbr");

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(BitbucketError::Http)?;

    let resp = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(BitbucketError::Http)?;

    if !resp.status().is_success() {
        return Err(BitbucketError::Other(format!(
            "Download failed: HTTP {}",
            resp.status()
        )));
    }

    let bytes = resp.bytes().await.map_err(BitbucketError::Http)?;

    use std::io::Read;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
    let mut extracted = false;

    for entry in archive
        .entries()
        .map_err(|e| BitbucketError::Other(format!("Corrupt archive: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| BitbucketError::Other(format!("Archive read error: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| BitbucketError::Other(format!("Archive path error: {e}")))?;

        if path.file_name().is_some_and(|n| n == "bbr") {
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|e| BitbucketError::Other(format!("Extract error: {e}")))?;

            let tmp_path = dest_dir.join(".bbr.tmp");
            fs::write(&tmp_path, &data).map_err(|e| {
                BitbucketError::Other(format!("Failed to write {}: {e}", tmp_path.display()))
            })?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755)).map_err(|e| {
                    BitbucketError::Other(format!("Failed to set permissions: {e}"))
                })?;
            }

            fs::rename(&tmp_path, &dest_path).map_err(|e| {
                BitbucketError::Other(format!("Failed to install to {}: {e}", dest_path.display()))
            })?;

            extracted = true;
            break;
        }
    }

    if !extracted {
        return Err(BitbucketError::Other(
            "Archive does not contain a 'bbr' binary".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_strips_v_prefix() {
        assert_eq!(parse_version("v1.2.3"), Some(vec![1, 2, 3]));
        assert_eq!(parse_version("1.2.3"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn parse_version_handles_two_parts() {
        assert_eq!(parse_version("v1.2"), Some(vec![1, 2]));
    }

    #[test]
    fn parse_version_returns_none_for_invalid() {
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("v1.x.3"), None);
    }

    #[test]
    fn is_newer_detects_higher_version() {
        assert!(is_newer("v1.1.0", "v1.0.0"));
        assert!(is_newer("v2.0.0", "v1.9.9"));
        assert!(is_newer("v0.2.0", "v0.1.1"));
    }

    #[test]
    fn is_newer_returns_false_for_same() {
        assert!(!is_newer("v1.0.0", "v1.0.0"));
    }

    #[test]
    fn is_newer_returns_false_for_older() {
        assert!(!is_newer("v1.0.0", "v1.1.0"));
    }

    #[test]
    fn is_newer_falls_back_to_string_compare() {
        assert!(is_newer("v1.0.1", "v1.0.0"));
        assert!(!is_newer("v1.0.0", "v1.0.1"));
    }

    #[test]
    fn render_update_shows_up_to_date() {
        let out = UpdateOut {
            current_version: "1.0.0".into(),
            latest_version: "1.0.0".into(),
            up_to_date: true,
            release_url: None,
            install_hint: None,
        };
        let rendered = render_update(&out);
        assert!(rendered.contains("up to date"));
        assert!(rendered.contains("1.0.0"));
    }

    #[test]
    fn render_update_shows_new_version() {
        let out = UpdateOut {
            current_version: "1.0.0".into(),
            latest_version: "2.0.0".into(),
            up_to_date: false,
            release_url: Some("https://github.com/test/releases/tag/v2.0.0".into()),
            install_hint: Some("Run `bbr update`".into()),
        };
        let rendered = render_update(&out);
        assert!(rendered.contains("2.0.0"));
        assert!(rendered.contains("1.0.0"));
    }
}
