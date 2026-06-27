//! `bb open` — open Bitbucket pages in the user's browser.

use serde::Serialize;

use crate::cli::{GlobalArgs, OpenAction};
use crate::commands::{client, current_repo};
use crate::error::{BitbucketError, Result};
use crate::git;
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct OpenOut {
    pub target: String,
    pub url: String,
    pub opened: bool,
}

pub async fn run(g: &GlobalArgs, action: Option<OpenAction>) -> Result<()> {
    let action = action.unwrap_or(OpenAction::Repo);
    let (target, url) = match action {
        OpenAction::Repo => repo_url(g).await?,
        OpenAction::Pr { id } => pr_url(g, id).await?,
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
            let head = git::head()?;
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
        None => git::current_branch()?,
    };
    let client = client(g)?;
    let pipeline = client
        .latest_pipeline(&repo.workspace, &repo.slug, Some(&branch))
        .await?
        .ok_or_else(|| BitbucketError::NotFound(format!("no pipeline for branch '{branch}'")))?;
    let url = pipeline.links.html.href.ok_or_else(|| {
        BitbucketError::NotFound(format!(
            "pipeline {} does not include an HTML URL",
            pipeline.uuid
        ))
    })?;
    Ok(("ci".into(), url))
}

fn open_url(url: &str) -> Result<bool> {
    let status = opener_command(url)
        .status()
        .map_err(|e| BitbucketError::Other(format!("opening browser: {e}")))?;
    if status.success() {
        Ok(true)
    } else {
        Err(BitbucketError::Other(format!(
            "browser opener exited with {status}"
        )))
    }
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
