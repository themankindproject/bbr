//! `bbr open` — open Bitbucket pages in the user's browser.

use serde::Serialize;

use crate::cli::{GlobalArgs, OpenAction};
use crate::commands::{client, current_head, make_formatter, resolve_repo};
use crate::error::{BitbucketError, Result};

#[derive(Debug, Serialize)]
pub struct OpenOut {
    pub target: String,
    pub url: String,
    pub opened: bool,
}

pub async fn run(g: &GlobalArgs, action: Option<OpenAction>) -> Result<()> {
    let action = action.unwrap_or(OpenAction::Repo);
    let repo = resolve_repo(g)?;
    let (target, url) = match action {
        OpenAction::Repo => repo_url(g).await?,
        OpenAction::PrList => (
            "pr-list".into(),
            format!(
                "https://bitbucket.org/{}/{}/pull-requests",
                repo.workspace, repo.slug
            ),
        ),
        OpenAction::Pr { id } => pr_url(g, id).await?,
        OpenAction::Pipelines => (
            "pipelines".into(),
            format!(
                "https://bitbucket.org/{}/{}/pipelines",
                repo.workspace, repo.slug
            ),
        ),
        OpenAction::Ci { branch } => ci_url(g, branch.as_deref()).await?,
    };

    let opened = if g.json { false } else { open_url(&url).await? };
    let out = OpenOut {
        target,
        url: url.clone(),
        opened,
    };
    let human = if opened { format!("Opened {url}") } else { url };
    make_formatter(g).print(&out, &human)
}

async fn repo_url(g: &GlobalArgs) -> Result<(String, String)> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let info = client.get_repo(&repo.workspace, &repo.slug).await?;
    let url = info
        .links
        .html
        .href
        .unwrap_or_else(|| format!("https://bitbucket.org/{}/{}", repo.workspace, repo.slug));
    Ok(("repo".into(), url))
}

async fn pr_url(g: &GlobalArgs, id: Option<u64>) -> Result<(String, String)> {
    let repo = resolve_repo(g)?;
    let client = client(g)?;
    let pr = match id {
        Some(id) => client.get_pr(&repo.workspace, &repo.slug, id).await?,
        None => {
            let head = current_head()?;
            client
                .pr_for_branch_light(&repo.workspace, &repo.slug, &head.branch)
                .await?
                .ok_or_else(|| {
                    BitbucketError::NotFound(format!("no open PR for branch '{}'", head.branch))
                })?
        }
    };
    let url = pr.web_url().ok_or_else(|| {
        BitbucketError::NotFound(format!("PR #{} does not include an HTML URL", pr.id))
    })?;
    Ok(("pr".into(), url.to_string()))
}

async fn ci_url(g: &GlobalArgs, branch: Option<&str>) -> Result<(String, String)> {
    let repo = resolve_repo(g)?;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => current_head()?.branch,
    };
    let client = client(g)?;
    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;
    let url = pipeline.links.html.href.unwrap_or_else(|| {
        format!(
            "https://bitbucket.org/{}/{}/pipelines/results/{}",
            repo.workspace, repo.slug, pipeline.build_number
        )
    });
    Ok(("ci".into(), url))
}

/// Whether a URL is safe to hand to an external opener.
///
/// The URL may come straight from an API response (`links.html.href`), so it
/// is untrusted input. Only `http`/`https` are allowed: a `file://`, a
/// `javascript:` or an option-shaped string like `--version` must never reach
/// `xdg-open`/`open`/`cmd start`.
fn is_safe_browser_url(url: &str) -> bool {
    let trimmed = url.trim();
    // Reject anything that could be parsed as an option rather than a URL.
    if trimmed.starts_with('-') {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else {
        return false;
    };
    // Must have a non-empty host and no characters that shell/opener tools
    // treat specially.
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
        && !host.starts_with(':')
        && !trimmed
            .chars()
            .any(|c| c.is_control() || c == '"' || c == '|' || c == '^' || c == '&')
}

async fn open_url(url: &str) -> Result<bool> {
    if !is_safe_browser_url(url) {
        return Err(BitbucketError::Other(format!(
            "refusing to open '{url}': not an http(s) URL"
        )));
    }
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        let status = match opener_command(&url).status() {
            Ok(s) => s,
            Err(e) => {
                crate::log_debug!("failed to launch browser opener: {e}");
                return Ok(false);
            }
        };
        Ok(status.success())
    })
    .await
    .map_err(|e| BitbucketError::Other(format!("open_url task panicked: {e}")))?
}

#[cfg(target_os = "macos")]
fn opener_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("open");
    // `--` stops `open` from treating a URL as a flag.
    cmd.arg("--").arg(url);
    cmd
}

/// Windows: use `ShellExecuteW` via `rundll32`-free path.
///
/// `cmd /C start "" <url>` is a command-injection sink: `cmd.exe` re-parses
/// the argument, so `&`, `|`, `^` and `"` in an attacker-supplied
/// `links.html.href` execute commands. `explorer.exe` receives the URL as a
/// single opaque argument and does not run a command interpreter.
#[cfg(target_os = "windows")]
fn opener_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("explorer");
    cmd.arg(url);
    cmd
}

#[cfg(all(unix, not(target_os = "macos")))]
fn opener_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https() {
        assert!(is_safe_browser_url("https://bitbucket.org/ws/r"));
        assert!(is_safe_browser_url("http://127.0.0.1:8080/x"));
        assert!(is_safe_browser_url("  https://x.com/y  "));
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(!is_safe_browser_url("file:///etc/passwd"));
        assert!(!is_safe_browser_url("javascript:alert(1)"));
        assert!(!is_safe_browser_url("ftp://x/y"));
        assert!(!is_safe_browser_url("not a url"));
    }

    #[test]
    fn rejects_option_shaped_and_injection_payloads() {
        // `xdg-open --version` would be a flag, not a URL.
        assert!(!is_safe_browser_url("--version"));
        assert!(!is_safe_browser_url("-a"));
        // Windows `cmd /C start` injection payloads.
        assert!(!is_safe_browser_url("https://x&calc.exe"));
        assert!(!is_safe_browser_url("https://x|calc"));
        assert!(!is_safe_browser_url("https://x^y"));
        assert!(!is_safe_browser_url("https://x\"y"));
        assert!(!is_safe_browser_url("https://x\x1by"));
        // Missing host.
        assert!(!is_safe_browser_url("https://"));
        assert!(!is_safe_browser_url("https:///path"));
    }

    #[test]
    fn open_out_serializes_correctly() {
        let out = OpenOut {
            target: "pr".into(),
            url: "https://bitbucket.org/ws/r/pull-requests/1".into(),
            opened: true,
        };
        let json = serde_json::to_value(out).unwrap();
        assert_eq!(json.get("target").and_then(|v| v.as_str()), Some("pr"));
        assert!(json.get("opened").and_then(|v| v.as_bool()).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn opener_uses_xdg_open_on_linux() {
        #[cfg(not(target_os = "macos"))]
        {
            let cmd = opener_command("https://example.com");
            let prog = cmd.get_program().to_str().unwrap().to_string();
            assert!(prog.contains("xdg-open"), "expected xdg-open, got {prog}");
        }
    }

    #[test]
    fn open_out_defaults_opened_false_in_json_mode() {
        let out = OpenOut {
            target: "repo".into(),
            url: "https://bitbucket.org/ws/r".into(),
            opened: false,
        };
        let json = serde_json::to_string_pretty(&out).unwrap();
        assert!(json.contains("\"opened\": false"));
    }
}
