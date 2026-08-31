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
use crate::commands::make_formatter;
use crate::error::{BitbucketError, Result};
use crate::output::theme::Theme;

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
    // Drop prerelease/build metadata ("1.2.3-beta.1" -> "1.2.3") so the
    // numeric comparison below works and prerelease tags never compare
    // higher than the release they precede.
    let s = s.split(['-', '+']).next().unwrap_or(s);
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
    // release.yml packages Unix targets as .tar.gz and Windows as .zip —
    // the asset name must match what the pipeline actually publishes.
    if cfg!(windows) {
        Some(format!("bbr-{target}.zip"))
    } else {
        Some(format!("bbr-{target}.tar.gz"))
    }
}

/// Name of the binary inside the release archive / on disk.
fn binary_file_name() -> &'static str {
    if cfg!(windows) {
        "bbr.exe"
    } else {
        "bbr"
    }
}

// ---------------------------------------------------------------------------
// Install path detection
// ---------------------------------------------------------------------------

fn install_dir() -> Option<PathBuf> {
    // If the binary we're running was installed to a specific location, upgrade
    // it in place — this covers `cargo install` (which drops the binary in
    // ~/.cargo/bin) and other non-standard install paths. Falls back to the
    // conventional ~/.local/bin when the real path is unknowable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.is_dir() && !exe.is_symlink() {
                return Some(parent.to_path_buf());
            }
        }
    }
    // `dirs::home_dir()` resolves the home directory portably — `$HOME` is
    // not set on Windows (it uses `USERPROFILE`), so reading the env var
    // directly would make every fallback below unreachable there.
    if let Some(home) = dirs::home_dir() {
        let candidates = [home.join(".local").join("bin"), home.join("bin")];
        for d in &candidates {
            if d.is_dir() {
                return Some(d.clone());
            }
        }
        // Create ~/.local/bin if it doesn't exist
        let local_bin = home.join(".local").join("bin");
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
fn update_client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .ok()
    });
    CLIENT.get().and_then(|c| c.as_ref()).ok_or_else(|| {
        BitbucketError::Other(
            "failed to build HTTP client for update check (TLS backend missing?)".into(),
        )
    })
}

async fn fetch_latest_release() -> Result<GithubRelease> {
    let client = update_client()?;
    let resp = client
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

/// Compare the running version against the latest GitHub release tag.
/// Returns `Ok(Some(latest))` when an update is available, `Ok(None)` when
/// current, and `Err` when the check itself failed. Used by `bbr update`
/// and the `bbr doctor` version check.
pub(crate) async fn outdated_version() -> Result<Option<String>> {
    let release = fetch_latest_release().await?;
    let latest = release.tag_name.trim().to_string();
    let current = env!("CARGO_PKG_VERSION");
    if is_newer(&latest, current) {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// `bbr update` command
// ---------------------------------------------------------------------------

pub async fn run(g: &GlobalArgs, check_only: bool) -> Result<()> {
    let loading =
        crate::commands::SpinnerGuard::new(crate::commands::make_spinner(g.json, g.quiet));
    loading.set_message("Checking for updates...");

    let release = fetch_latest_release().await?;
    let current = env!("CARGO_PKG_VERSION");
    let latest = release.tag_name.trim().to_string();

    if !is_newer(&latest, current) {
        loading.finish();
        let out = UpdateOut {
            current_version: current.to_string(),
            latest_version: latest,
            up_to_date: true,
            release_url: None,
            install_hint: None,
        };
        let human = render_update(&out);
        return make_formatter(g).print(&out, &human);
    }

    loading.finish();

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
        return make_formatter(g).print(&out, &human);
    }

    let loading =
        crate::commands::SpinnerGuard::new(crate::commands::make_spinner(g.json, g.quiet));
    let arrow = if crate::output::theme::Theme::current().unicode_enabled() {
        "→"
    } else {
        "->"
    };
    loading.set_message(format!("Updating bbr {} {arrow} {}...", current, latest));

    // The download below runs its own progress bar on stderr; finish this
    // spinner first, or the two animations fight over the terminal and the
    // download progress flickers.
    loading.finish();

    download_and_install(&release, &latest, g.json, g.quiet).await?;

    let theme = Theme::current();
    eprintln!("{}  Updated bbr to {latest}", theme.checkmark());

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
        let mark = if theme.unicode_enabled() { "✓" } else { "OK" };
        let _ = std::fmt::Write::write_fmt(
            &mut s,
            format_args!(
                "{}  bbr {} — up to date\n",
                theme.success(mark),
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

async fn download_and_install(
    release: &GithubRelease,
    _latest: &str,
    json: bool,
    quiet: bool,
) -> Result<()> {
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
    let dest_path = dest_dir.join(binary_file_name());

    // Probe writability BEFORE downloading so a permission problem fails fast
    // with an actionable message instead of after a multi-MB download.
    if let Err(e) = tempfile::NamedTempFile::new_in(&dest_dir) {
        return Err(BitbucketError::Other(format!(
            "Install directory {} is not writable: {e}.\n\
             Re-run with elevated permissions (e.g. `sudo bbr update`) \
             or install bbr somewhere on your PATH that you own.",
            dest_dir.display()
        )));
    }

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

    // Stream the download with a progress bar — multi-MB binaries on slow
    // links otherwise look like a hang.
    let total_size = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(total_size.unwrap_or(0) as usize);

    let pb = if json || quiet || std::env::var_os("BBR_QUIET").is_some() {
        indicatif::ProgressBar::hidden()
    } else if let Some(total) = total_size {
        let pb = indicatif::ProgressBar::new(total);
        pb.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(20));
        pb.set_style(
            indicatif::ProgressStyle::with_template(
                "{spinner:.cyan} Downloading {msg} {bytes}/{total_bytes} ({eta} left)",
            )
            .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar()),
        );
        pb.set_message(target_name.clone());
        pb
    } else {
        let pb = indicatif::ProgressBar::new_spinner();
        pb.set_draw_target(indicatif::ProgressDrawTarget::stderr_with_hz(20));
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        pb.set_style(
            indicatif::ProgressStyle::with_template("{spinner:.cyan} Downloading {msg} {bytes}")
                .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
        );
        pb.set_message(target_name.clone());
        pb
    };

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BitbucketError::Http)?;
        buf.extend_from_slice(&chunk);
        pb.set_position(buf.len() as u64);
    }
    pb.finish_and_clear();

    let bytes = buf;

    // --- SHA256 integrity verification (fail-closed) ---
    // Look for a checksums asset (checksums.txt or SHA256SUMS) in the release.
    // The release pipeline always publishes checksums.txt, so a missing or
    // incomplete checksum file is treated as a hard failure unless the user
    // explicitly opts out with BBR_SKIP_CHECKSUM=1.
    let skip_checksum = env_flag_enabled("BBR_SKIP_CHECKSUM");
    let checksum_asset = release.assets.iter().find(|a| {
        let name = a.name.to_ascii_lowercase();
        name == "checksums.txt"
            || name == "sha256sums"
            || name == "sha256sums.txt"
            || name.contains("checksum")
    });

    let mut verified = false;
    if let Some(cs_asset) = checksum_asset {
        let cs_resp = client
            .get(&cs_asset.browser_download_url)
            .send()
            .await
            .map_err(BitbucketError::Http)?;
        if cs_resp.status().is_success() {
            let cs_text = cs_resp.text().await.map_err(BitbucketError::Http)?;
            if let Some(expected_hash) = parse_checksum_for_asset(&cs_text, &target_name) {
                let actual_hash = sha256_hex(&bytes);
                if actual_hash != expected_hash {
                    return Err(BitbucketError::Other(format!(
                        "SHA256 checksum mismatch for {target_name}!\n\
                         Expected: {expected_hash}\n\
                         Got:      {actual_hash}\n\
                         The downloaded archive may be corrupted or tampered with. \
                         Aborting update."
                    )));
                }
                verified = true;
                tracing::debug!("SHA256 checksum verified for {target_name}");
            } else {
                tracing::warn!("Checksum file found but no entry for {target_name}");
            }
        } else {
            tracing::warn!(
                "Checksum asset returned HTTP {}; could not verify",
                cs_resp.status()
            );
        }
    }

    if !verified {
        if skip_checksum {
            let mark = if crate::output::theme::Theme::current().unicode_enabled() {
                "⚠"
            } else {
                "!"
            };
            eprintln!(
                "  {mark} Warning: BBR_SKIP_CHECKSUM=1 set — installing without \
                 integrity verification."
            );
        } else {
            return Err(BitbucketError::Other(format!(
                "Could not verify the integrity of {target_name}: no usable \
                 checksum entry in this release.\n\
                 Refusing to install an unverified binary. If you trust this \
                 source and accept the risk, re-run with BBR_SKIP_CHECKSUM=1."
            )));
        }
    }

    // Extract the binary from the archive (tar.gz on Unix, zip on Windows)
    // and install it atomically.
    let bin_name = binary_file_name();
    let extracted = if target_name.ends_with(".zip") {
        extract_from_zip(&bytes, &dest_dir, &dest_path, bin_name)?
    } else {
        extract_from_tar_gz(&bytes, &dest_dir, &dest_path, bin_name)?
    };

    if !extracted {
        return Err(BitbucketError::Other(format!(
            "Archive does not contain a '{bin_name}' binary"
        )));
    }

    Ok(())
}

/// Extract `bin_name` from a gzipped tar archive into `dest_path`.
fn extract_from_tar_gz(
    bytes: &[u8],
    dest_dir: &Path,
    dest_path: &Path,
    bin_name: &str,
) -> Result<bool> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));

    for entry in archive
        .entries()
        .map_err(|e| BitbucketError::Other(format!("Failed to read archive: {e}")))?
    {
        let mut entry = entry
            .map_err(|e| BitbucketError::Other(format!("Failed to read archive entry: {e}")))?;

        let path = entry
            .path()
            .map_err(|e| BitbucketError::Other(format!("Invalid archive entry path: {e}")))?
            .into_owned();

        if path.file_name().and_then(|n| n.to_str()) == Some(bin_name) {
            let mut tmp = tempfile::NamedTempFile::new_in(dest_dir).map_err(|e| {
                BitbucketError::Other(format!(
                    "Failed to create temp file in {}: {e}",
                    dest_dir.display()
                ))
            })?;
            std::io::copy(&mut entry, &mut tmp)
                .map_err(|e| BitbucketError::Other(format!("Failed to write temp file: {e}")))?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let tmp_path = tmp.path().to_path_buf();
                fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755)).map_err(|e| {
                    BitbucketError::Other(format!("Failed to set permissions: {e}"))
                })?;
            }

            install_binary(tmp, dest_path)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Extract `bin_name` from a zip archive into `dest_path` (Windows assets).
#[cfg(windows)]
fn extract_from_zip(
    bytes: &[u8],
    dest_dir: &Path,
    dest_path: &Path,
    bin_name: &str,
) -> Result<bool> {
    use std::io::Read;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| BitbucketError::Other(format!("Failed to read zip archive: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| BitbucketError::Other(format!("Failed to read zip entry: {e}")))?;
        if !entry.is_file() {
            continue;
        }
        // Match on the file name component only — release archives may nest
        // the binary under a directory.
        let name_matches = std::path::Path::new(entry.name())
            .file_name()
            .and_then(|n| n.to_str())
            == Some(bin_name);
        if !name_matches {
            continue;
        }

        let mut tmp = tempfile::NamedTempFile::new_in(dest_dir).map_err(|e| {
            BitbucketError::Other(format!(
                "Failed to create temp file in {}: {e}",
                dest_dir.display()
            ))
        })?;
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| BitbucketError::Other(format!("Failed to read zip entry: {e}")))?;
        tmp.write_all(&buf)
            .map_err(|e| BitbucketError::Other(format!("Failed to write temp file: {e}")))?;

        install_binary(tmp, dest_path)?;
        return Ok(true);
    }
    Ok(false)
}

/// Zip extraction is only compiled on Windows (the only platform that ships
/// .zip assets); other platforms never see a .zip target name.
#[cfg(not(windows))]
fn extract_from_zip(
    _bytes: &[u8],
    _dest_dir: &Path,
    _dest_path: &Path,
    _bin_name: &str,
) -> Result<bool> {
    Err(BitbucketError::Other(
        "zip assets are only supported on Windows builds".into(),
    ))
}

/// Move a fully-written temp file into place as the new binary.
///
/// On Windows the currently-running executable is locked against deletion or
/// overwrite, so the old binary is first renamed out of the way (renaming a
/// running .exe IS permitted). The stale `.old` copy from a previous update
/// is removed best-effort.
fn install_binary(tmp: tempfile::NamedTempFile, dest_path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        if dest_path.exists() {
            let mut old_path = dest_path.to_path_buf();
            let mut old_name = dest_path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            old_name.push(".old");
            old_path.set_file_name(old_name);
            let _ = fs::remove_file(&old_path);
            fs::rename(dest_path, &old_path).map_err(|e| {
                BitbucketError::Other(format!(
                    "Failed to move the running binary out of the way ({}): {e}. \
                     Close other bbr processes and retry.",
                    dest_path.display()
                ))
            })?;
        }
    }

    tmp.persist(dest_path).map_err(|e| {
        BitbucketError::Other(format!(
            "Failed to install to {}: {}",
            dest_path.display(),
            e.error
        ))
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// SHA256 helpers
// ---------------------------------------------------------------------------

/// Compute the hex-encoded SHA256 hash of some bytes.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    // Format as lowercase hex
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// Parse a checksums file (format: `<hex-hash>  <filename>` or `<hex-hash> *<filename>`)
/// and return the expected hash for the given asset name.
fn parse_checksum_for_asset(checksums_text: &str, asset_name: &str) -> Option<String> {
    for line in checksums_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Common formats:
        //   abc123def  filename.tar.gz
        //   abc123def *filename.tar.gz
        let parts: Vec<&str> = line.splitn(2, |c: char| c.is_whitespace()).collect();
        if parts.len() == 2 {
            let hash = parts[0].trim();
            let name = parts[1].trim().trim_start_matches('*');
            // Match on the basename of the entry path. Comparing with
            // `ends_with` would let a sibling asset whose name merely ends
            // with ours (e.g. "old-bbr-x86_64.tar.gz") match by accident.
            let basename = std::path::Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(name);
            if basename == asset_name {
                // Validate it looks like a hex hash (64 chars for SHA256)
                if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hash.to_lowercase());
                }
            }
        }
    }
    None
}

/// True when an env var is set to a truthy value (`1`, `true`, `yes`, `on`,
/// case-insensitive). Merely being present (or set to `0`/empty) is NOT
/// enough — `BBR_SKIP_CHECKSUM=0` must not skip verification.
fn env_flag_enabled(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
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

    #[test]
    fn sha256_hex_computes_known_hash() {
        // SHA256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn parse_checksum_for_asset_standard_format() {
        let checksums = "\
abc123def456abc123def456abc123def456abc123def456abc123def456abcd  bbr-x86_64-unknown-linux-gnu.tar.gz\n\
def456abc123def456abc123def456abc123def456abc123def456abc123def4  bbr-aarch64-apple-darwin.tar.gz\n";
        let result = parse_checksum_for_asset(checksums, "bbr-x86_64-unknown-linux-gnu.tar.gz");
        assert_eq!(
            result,
            Some("abc123def456abc123def456abc123def456abc123def456abc123def456abcd".to_string())
        );
    }

    #[test]
    fn parse_checksum_for_asset_star_prefix() {
        let checksums =
            "abc123def456abc123def456abc123def456abc123def456abc123def456abcd *bbr-x86_64-unknown-linux-gnu.tar.gz\n";
        let result = parse_checksum_for_asset(checksums, "bbr-x86_64-unknown-linux-gnu.tar.gz");
        assert_eq!(
            result,
            Some("abc123def456abc123def456abc123def456abc123def456abc123def456abcd".to_string())
        );
    }

    #[test]
    fn parse_checksum_for_asset_not_found() {
        let checksums =
            "abc123def456abc123def456abc123def456abc123def456abc123def456abcd  other-file.tar.gz\n";
        let result = parse_checksum_for_asset(checksums, "bbr-x86_64-unknown-linux-gnu.tar.gz");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_checksum_for_asset_skips_comments_and_empty() {
        let checksums = "\
# SHA256 checksums\n\
\n\
abc123def456abc123def456abc123def456abc123def456abc123def456abcd  bbr-x86_64-unknown-linux-gnu.tar.gz\n";
        let result = parse_checksum_for_asset(checksums, "bbr-x86_64-unknown-linux-gnu.tar.gz");
        assert_eq!(
            result,
            Some("abc123def456abc123def456abc123def456abc123def456abc123def456abcd".to_string())
        );
    }

    #[test]
    fn parse_checksum_for_asset_matches_basename_in_path() {
        // Checksum entries may carry a directory prefix; match on basename.
        let checksums =
            "abc123def456abc123def456abc123def456abc123def456abc123def456abcd  dist/bbr-x86_64-unknown-linux-gnu.tar.gz\n";
        let result = parse_checksum_for_asset(checksums, "bbr-x86_64-unknown-linux-gnu.tar.gz");
        assert!(result.is_some());
    }

    #[test]
    fn parse_checksum_for_asset_rejects_suffix_collision() {
        // A sibling asset whose name merely ENDS with ours must not match
        // (the old `ends_with` logic would have accepted this).
        let checksums =
            "abc123def456abc123def456abc123def456abc123def456abc123def456abcd  old-bbr-x86_64-unknown-linux-gnu.tar.gz\n";
        let result = parse_checksum_for_asset(checksums, "bbr-x86_64-unknown-linux-gnu.tar.gz");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_version_strips_prerelease_metadata() {
        // Prerelease/build metadata must not break numeric comparison.
        assert_eq!(parse_version("v1.2.3-beta.1"), Some(vec![1, 2, 3]));
        assert_eq!(parse_version("1.2.3+build.5"), Some(vec![1, 2, 3]));
    }

    #[test]
    fn is_newer_prerelease_not_newer_than_release() {
        // A prerelease of the same version must not be offered as an update.
        assert!(!is_newer("v1.2.3-beta.1", "v1.2.3"));
        // But a prerelease of a HIGHER version still is.
        assert!(is_newer("v1.3.0-rc.1", "v1.2.3"));
    }

    #[test]
    fn env_flag_enabled_requires_truthy_value() {
        let _guard = crate::test_support::env_lock();
        std::env::set_var("BBR_TEST_FLAG", "1");
        assert!(env_flag_enabled("BBR_TEST_FLAG"));
        std::env::set_var("BBR_TEST_FLAG", "true");
        assert!(env_flag_enabled("BBR_TEST_FLAG"));
        std::env::set_var("BBR_TEST_FLAG", "0");
        assert!(!env_flag_enabled("BBR_TEST_FLAG"));
        std::env::set_var("BBR_TEST_FLAG", "");
        assert!(!env_flag_enabled("BBR_TEST_FLAG"));
        std::env::remove_var("BBR_TEST_FLAG");
        assert!(!env_flag_enabled("BBR_TEST_FLAG"));
    }

    #[test]
    fn asset_name_matches_platform_archive_format() {
        // release.yml ships .zip for Windows and .tar.gz elsewhere; the
        // updater must look for exactly what the pipeline publishes.
        if let Some(name) = asset_name() {
            if cfg!(windows) {
                assert!(
                    name.ends_with(".zip"),
                    "windows asset must be a zip: {name}"
                );
            } else {
                assert!(
                    name.ends_with(".tar.gz"),
                    "unix asset must be tar.gz: {name}"
                );
            }
        }
    }

    #[test]
    fn binary_file_name_matches_platform() {
        if cfg!(windows) {
            assert_eq!(binary_file_name(), "bbr.exe");
        } else {
            assert_eq!(binary_file_name(), "bbr");
        }
    }
}
