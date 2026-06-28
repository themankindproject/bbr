//! `bb open` — open Bitbucket pages in the user's browser.

use serde::Serialize;

use crate::cli::{GlobalArgs, OpenAction};
use crate::commands::{client, current_head, current_repo};
use crate::error::{BitbucketError, Result};
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct OpenOut {
    pub target: String,
    pub url: String,
    pub opened: bool,
}

pub async fn run(g: &GlobalArgs, action: Option<OpenAction>) -> Result<()> {
    let action = action.unwrap_or(OpenAction::Repo);
    let repo = current_repo()?;
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

    let opened = if g.json { false } else { open_url(&url)? };
    let out = OpenOut {
        target,
        url: url.clone(),
        opened,
    };
    let human = if opened { format!("Opened {url}") } else { url };
    Formatter::from_json_flag(g.json).print(&out, &human)
}

async fn repo_url(g: &GlobalArgs) -> Result<(String, String)> {
    let repo = current_repo()?;
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
    let repo = current_repo()?;
    let client = client(g)?;
    let pr = match id {
        Some(id) => client.get_pr(&repo.workspace, &repo.slug, id).await?,
        None => {
            let head = current_head()?;
            client
                .pr_for_branch(&repo.workspace, &repo.slug, &head.branch)
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
    let repo = current_repo()?;
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

fn open_url(url: &str) -> Result<bool> {
    let status = match opener_command(url).status() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("failed to launch browser opener: {e}");
            return Ok(false);
        }
    };
    Ok(status.success())
}

#[cfg(target_os = "macos")]
fn opener_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("open");
    cmd.arg(url);
    cmd
}

#[cfg(target_os = "windows")]
fn opener_command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", "", url]);
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
