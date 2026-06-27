//! Subcommand implementations.

pub mod auth;
pub mod ci;
pub mod open;
pub mod pr;
pub mod repo;
pub mod status;

use std::sync::OnceLock;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::api::BitbucketClient;
use crate::cli::{resolve_api_base, GlobalArgs};
use crate::error::{BitbucketError, Result};
use crate::git::{self, Head, RepoIdentity};

/// Resolve credentials + API base into a ready-to-use client.
pub fn client(g: &GlobalArgs) -> Result<BitbucketClient> {
    let creds = crate::auth::resolve()?;
    let base = resolve_api_base(g);
    creds.into_client(base)
}

static CACHED_REPO: OnceLock<RepoIdentity> = OnceLock::new();
static CACHED_HEAD: OnceLock<Head> = OnceLock::new();

/// Detect the current repo identity from git (cached per process).
pub fn current_repo() -> Result<RepoIdentity> {
    if let Some(r) = CACHED_REPO.get() {
        return Ok(r.clone());
    }
    let repo = git::detect_repo()?;
    let _ = CACHED_REPO.set(repo.clone());
    Ok(repo)
}

/// Current branch + commit (cached per process).
pub fn current_head() -> Result<Head> {
    if let Some(h) = CACHED_HEAD.get() {
        return Ok(h.clone());
    }
    let head = git::head()?;
    let _ = CACHED_HEAD.set(head.clone());
    Ok(head)
}

/// Read body text from one of: direct `--body`, a `--body-file`, or stdin
/// (when `body_stdin` is set). Returns the resolved text or an error if none.
pub fn resolve_body(
    body: Option<&str>,
    body_file: Option<&str>,
    body_stdin: bool,
) -> Result<String> {
    if body_stdin {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(BitbucketError::Io)?;
        return Ok(buf);
    }
    if let Some(path) = body_file {
        return std::fs::read_to_string(path)
            .map_err(|e| BitbucketError::Other(format!("reading {path}: {e}")));
    }
    if let Some(b) = body {
        return Ok(b.to_string());
    }
    Err(BitbucketError::Other(
        "no body provided (use --body, --body-file, or --body-stdin)".into(),
    ))
}

/// Create a spinner if stdout is a TTY and we're not in JSON mode.
pub fn make_spinner(json: bool) -> ProgressBar {
    if json {
        ProgressBar::hidden()
    } else {
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_style(
            ProgressStyle::with_template("{spinner} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb
    }
}

/// Format seconds as a human-friendly duration string.
pub fn human_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

/// Truncate a string to `n` characters, appending an ellipsis if truncated.
pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

/// Prompt the user for a yes/no confirmation on stderr.
/// Returns `true` if the user typed `y` or `yes`.
pub fn confirm(msg: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    let mut out = std::io::stderr().lock();
    out.write_all(msg.as_bytes()).map_err(BitbucketError::Io)?;
    out.flush().map_err(BitbucketError::Io)?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(BitbucketError::Io)?;
    let trimmed = line.trim();
    Ok(trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes"))
}
