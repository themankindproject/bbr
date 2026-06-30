//! Git integration: detect the current branch, workspace slug, and repo slug
//! by shelling out to `git` (kept lean to avoid a libgit2 dependency).

use std::process::Command;

use crate::error::{BitbucketError, Result};

/// `{workspace}/{repo-slug}` parsed from the `bitbucket.org` remote URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    pub workspace: String,
    pub slug: String,
}

/// `{branch}` + `{short_commit}` for the current HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub branch: String,
    pub commit: String,
}

/// Run a `git` command in `cwd`, returning trimmed stdout.
fn git(args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| BitbucketError::Git(format!("failed to run git: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(BitbucketError::Git(stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Current branch name. Errors with a friendly message if HEAD is detached.
pub fn current_branch() -> Result<String> {
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        return Err(BitbucketError::Git(
            "HEAD is detached (not on any branch)".into(),
        ));
    }
    Ok(branch)
}

/// Short (12-char) commit hash for HEAD.
pub fn current_commit() -> Result<String> {
    let full = git(&["rev-parse", "HEAD"])?;
    let len = full.len().min(12);
    Ok(full[..len].to_string())
}

/// Combined branch + commit info for the working directory.
pub fn head() -> Result<Head> {
    let branch = current_branch()?;
    let commit = current_commit()?;
    Ok(Head { branch, commit })
}

/// Parse a Bitbucket Cloud remote URL into a [`RepoIdentity`].
///
/// Accepts HTTPS (`https://bitbucket.org/<ws>/<slug>.git`), SSH
/// (`git@bitbucket.org:<ws>/<slug>.git`), and SSH host alias
/// (`git@alias:<ws>/<slug>.git`) forms.
pub fn parse_remote_url(url: &str) -> Option<RepoIdentity> {
    let url = url.trim().trim_end_matches(".git");
    // strip credentials embedded in https url: https://user:pass@host/ws/slug
    let no_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .map(|rest| {
            // drop credentials embedded before the host: user:pass@host/...
            rest.split('@').next_back().unwrap_or(rest)
        });

    let path: &str = if let Some(rest) = no_scheme {
        rest.split_once('/').map(|(_, tail)| tail)?
    } else {
        url.strip_prefix("git@")
            .and_then(|rest| rest.split_once(':').map(|(_, path)| path))?
    };

    let mut parts = path.splitn(2, '/');
    let workspace = parts.next()?.trim();
    let slug = parts.next()?.trim();
    if workspace.is_empty() || slug.is_empty() {
        return None;
    }
    Some(RepoIdentity {
        workspace: workspace.to_string(),
        slug: slug.to_string(),
    })
}

/// Detect the Bitbucket repo identity from the `origin` remote (falling back
/// to scanning all remotes).
pub fn detect_repo() -> Result<RepoIdentity> {
    // Prefer `origin` explicitly before scanning all remotes.
    if let Ok(url) = git(&["remote", "get-url", "origin"]) {
        if let Some(id) = parse_remote_url(&url) {
            return Ok(id);
        }
    }
    // Fall back to scanning all remotes.
    let remotes = git(&["remote", "-v"])?;
    for line in remotes.lines() {
        let mut parts = line.split('\t');
        let _name = parts.next();
        let rest = parts.next().unwrap_or("");
        let url = rest.split_whitespace().next().unwrap_or("");
        if let Some(id) = parse_remote_url(url) {
            return Ok(id);
        }
    }
    Err(BitbucketError::Git(
        "no bitbucket.org remote found in this repository".into(),
    ))
}

/// Fetch a branch from origin.
pub fn fetch_branch(branch: &str) -> Result<()> {
    git(&["fetch", "origin", branch])?;
    Ok(())
}

/// Checkout a local branch (creating it if it doesn't exist).
pub fn checkout_branch(branch: &str) -> Result<()> {
    match git(&["switch", branch]) {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            // Only retry with -c if the branch doesn't exist locally
            if msg.contains("not a valid object")
                || msg.contains("not found")
                || msg.contains("unknown revision")
                || msg.contains("fatal: invalid reference")
            {
                git(&["switch", "-c", branch, &format!("origin/{branch}")]).map(|_| ())
            } else {
                // The original error was something else (dirty tree, conflicts, etc.)
                Err(e)
            }
        }
    }
}

/// Convenience: detect the repo identity and current HEAD together.
pub fn context() -> Result<(RepoIdentity, Head)> {
    let repo = detect_repo()?;
    let head = head()?;
    Ok((repo, head))
}

/// Run git status --porcelain to see if working tree is dirty.
pub fn git_status_porcelain() -> Result<String> {
    git(&["status", "--porcelain"])
}

/// Check if working tree has any modifications, untracked files, etc.
pub fn is_working_tree_clean() -> Result<bool> {
    let status = git_status_porcelain()?;
    Ok(status.is_empty())
}

/// Git push a branch to origin.
pub fn push_branch(branch: &str) -> Result<()> {
    git(&["push", "origin", branch])?;
    Ok(())
}

/// Git push --force-with-lease to origin.
pub fn push_force_with_lease(branch: &str) -> Result<()> {
    git(&["push", "--force-with-lease", "origin", branch])?;
    Ok(())
}

/// Delete a branch locally.
pub fn delete_branch_local(branch: &str) -> Result<()> {
    git(&["branch", "-D", branch])?;
    Ok(())
}

/// Delete a remote branch on origin.
pub fn delete_branch_remote(branch: &str) -> Result<()> {
    git(&["push", "origin", "--delete", branch])?;
    Ok(())
}

/// Rebase branch onto another branch.
pub fn rebase_branch(branch: &str, onto: &str) -> Result<()> {
    // switch to the target branch first, then rebase onto the parent
    git(&["switch", branch])?;
    git(&["rebase", onto])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_remote() {
        let id = parse_remote_url("git@bitbucket.org:sdadev/bvrm-backend.git").unwrap();
        assert_eq!(id.workspace, "sdadev");
        assert_eq!(id.slug, "bvrm-backend");
    }

    #[test]
    fn parses_https_remote_with_creds() {
        let id =
            parse_remote_url("https://user:pass@bitbucket.org/sdadev/bvrm-backend.git").unwrap();
        assert_eq!(id.workspace, "sdadev");
        assert_eq!(id.slug, "bvrm-backend");
    }

    #[test]
    fn parses_ssh_url_with_any_host() {
        let id = parse_remote_url("git@github.com:foo/bar.git").unwrap();
        assert_eq!(id.workspace, "foo");
        assert_eq!(id.slug, "bar");
    }
}
