//! `bb commit` — commit metadata and build-status operations.

use serde::Serialize;

use crate::api::status::{BuildStatus, BuildStatusRequest};
use crate::cli::GlobalArgs;
use crate::commands::{client, current_head, current_repo, make_spinner};
use crate::error::{BitbucketError, Result};
use crate::output::Formatter;

#[derive(Debug, Serialize)]
pub struct CommitStatusOut {
    pub commit: String,
    pub key: String,
    pub state: String,
    pub name: String,
    pub url: String,
    pub description: Option<String>,
    pub refname: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn set_status(
    g: &GlobalArgs,
    commit: Option<&str>,
    key: &str,
    state: &str,
    name: Option<&str>,
    url: Option<&str>,
    description: Option<&str>,
    refname: Option<&str>,
) -> Result<()> {
    let repo = current_repo()?;
    let commit = match commit {
        Some(commit) => commit.to_string(),
        None => current_head()?.commit,
    };
    let req = BuildStatusRequest {
        key: key.to_string(),
        state: normalize_state(state)?,
        name: name.map(str::to_string),
        url: url.map(str::to_string),
        description: description.map(str::to_string),
        refname: refname.map(str::to_string),
    };
    let client = client(g)?;

    let spinner = make_spinner(g.json);
    spinner.set_message("Setting commit status...");
    let status = client
        .create_commit_status(&repo.workspace, &repo.slug, &commit, &req)
        .await?;
    spinner.finish_and_clear();

    let out = status_out(&commit, &status);
    let human = format!(
        "Set status '{}' on {} to {}",
        out.key,
        short_commit(&out.commit),
        out.state
    );
    Formatter::from_json_flag(g.json).print(&out, &human)
}

fn normalize_state(state: &str) -> Result<String> {
    let normalized = state.trim().replace(['-', '_'], "").to_ascii_uppercase();
    match normalized.as_str() {
        "SUCCESSFUL" | "SUCCESS" | "PASSED" => Ok("SUCCESSFUL".into()),
        "FAILED" | "FAILURE" | "ERROR" => Ok("FAILED".into()),
        "INPROGRESS" | "RUNNING" | "PENDING" => Ok("INPROGRESS".into()),
        "STOPPED" | "CANCELLED" | "CANCELED" => Ok("STOPPED".into()),
        _ => Err(BitbucketError::Other(
            "invalid --state (expected successful|failed|inprogress|stopped)".into(),
        )),
    }
}

fn status_out(commit: &str, status: &BuildStatus) -> CommitStatusOut {
    CommitStatusOut {
        commit: commit.to_string(),
        key: status.key.clone(),
        state: status.state.clone(),
        name: status.name.clone(),
        url: status.url.clone(),
        description: status.description.clone(),
        refname: status.refname.clone(),
    }
}

fn short_commit(commit: &str) -> &str {
    commit.get(..commit.len().min(12)).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_status_state() {
        assert_eq!(normalize_state("successful").unwrap(), "SUCCESSFUL");
        assert_eq!(normalize_state("in-progress").unwrap(), "INPROGRESS");
        assert_eq!(normalize_state("cancelled").unwrap(), "STOPPED");
        assert!(normalize_state("unknown").is_err());
    }
}
