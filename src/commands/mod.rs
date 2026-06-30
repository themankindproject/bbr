//! Subcommand implementations.

pub mod api;
pub mod audit;
pub mod auth;
pub mod batch;
pub mod ci;
pub mod ci_compare;
pub mod ci_vars;
pub mod commit;
pub mod completion;
pub mod config;
pub mod dashboard;
pub mod deploy;
pub mod export;
pub mod issue;
pub mod open;
pub mod pr;
pub mod repo;
pub mod schema;
pub mod search;
pub mod src_cmd;
pub mod stack;
pub mod status;
pub mod update;
pub mod webhook;

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

/// Detect the current repo identity respecting `--workspace` and `--slug` overrides.
pub fn resolve_repo(g: &GlobalArgs) -> Result<RepoIdentity> {
    match (&g.workspace, &g.repo_slug) {
        (Some(ws), Some(slug)) => Ok(RepoIdentity {
            workspace: ws.clone(),
            slug: slug.clone(),
        }),
        (Some(ws), None) => {
            let slug = current_repo()?.slug;
            Ok(RepoIdentity {
                workspace: ws.clone(),
                slug,
            })
        }
        (None, Some(slug)) => {
            let ws = current_repo()?.workspace;
            Ok(RepoIdentity {
                workspace: ws,
                slug: slug.clone(),
            })
        }
        (None, None) => current_repo(),
    }
}

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

/// Create a Formatter respecting --json and --no-pager flags.
pub fn make_formatter(g: &GlobalArgs) -> crate::output::Formatter {
    crate::output::Formatter::from_args(g.json, g.no_pager)
}

/// Check if quiet mode is enabled (via --quiet flag or BBR_QUIET env).
fn is_quiet() -> bool {
    std::env::var_os("BBR_QUIET").is_some()
}

/// Create a spinner if stdout is a TTY and we're not in JSON or quiet mode.
pub fn make_spinner(json: bool) -> ProgressBar {
    if json || is_quiet() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_duration_seconds_only() {
        assert_eq!(human_duration(30), "30s");
        assert_eq!(human_duration(0), "0s");
        assert_eq!(human_duration(59), "59s");
    }

    #[test]
    fn human_duration_minutes_and_seconds() {
        assert_eq!(human_duration(60), "1m 0s");
        assert_eq!(human_duration(90), "1m 30s");
        assert_eq!(human_duration(3599), "59m 59s");
    }

    #[test]
    fn human_duration_hours() {
        assert_eq!(human_duration(3600), "1h 0m 0s");
        assert_eq!(human_duration(3661), "1h 1m 1s");
        assert_eq!(human_duration(7202), "2h 0m 2s");
    }

    #[test]
    fn truncate_returns_string_when_shorter_than_limit() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_returns_string_when_equal() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_appends_ellipsis_when_longer() {
        let result = truncate("hello world", 5);
        assert_eq!(result, "hello…");
        assert_eq!(result.chars().count(), 6);
    }

    #[test]
    fn truncate_handles_empty() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_handles_unicode() {
        let result = truncate("héllo wörld", 6);
        assert!(result.starts_with("héllo "));
        assert!(result.ends_with('…'));
    }

    #[test]
    fn make_spinner_hidden_in_json_mode() {
        let pb = make_spinner(true);
        assert!(pb.is_hidden());
    }

    #[test]
    fn resolve_body_direct() {
        let result = resolve_body(Some("hello"), None, false).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn resolve_body_errors_without_source() {
        let err = resolve_body(None, None, false).unwrap_err();
        assert!(format!("{err}").contains("no body provided"));
    }
}
