//! Git integration: detect the current branch, workspace slug, and repo slug
//! by shelling out to `git` (kept lean to avoid a libgit2 dependency).

use std::process::Command;
use std::time::Duration;

use crate::error::{BitbucketError, Result};

/// Default timeout for git read operations (30 seconds).
const GIT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for git write operations (120 seconds).
const GIT_WRITE_TIMEOUT: Duration = Duration::from_secs(120);

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

/// Run a `git` command with a timeout, returning trimmed stdout.
fn git(args: &[&str]) -> Result<String> {
    git_with_timeout(args, GIT_READ_TIMEOUT)
}

/// Run a `git` command with a specific timeout, returning trimmed stdout.
///
/// Spawns the child process and waits with a timeout. On timeout, the child
/// is explicitly killed to prevent orphaned git processes from leaking.
fn git_with_timeout(args: &[&str], timeout: Duration) -> Result<String> {
    let mut child = Command::new("git")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| BitbucketError::Git(format!("failed to spawn git: {e}")))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exited — read output
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                if !status.success() {
                    let msg = String::from_utf8_lossy(&stderr).trim().to_string();
                    return Err(BitbucketError::Git(msg));
                }
                return Ok(String::from_utf8_lossy(&stdout).trim().to_string());
            }
            Ok(None) => {
                // Still running — check timeout
                if start.elapsed() >= timeout {
                    // Kill the child to prevent orphaned processes
                    let _ = child.kill();
                    let _ = child.wait(); // reap the zombie
                    return Err(BitbucketError::Git(format!(
                        "git command timed out after {}s: git {}",
                        timeout.as_secs(),
                        args.join(" ")
                    )));
                }
                // Sleep briefly to avoid busy-waiting
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(BitbucketError::Git(format!("failed to wait on git: {e}")));
            }
        }
    }
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
        "no git remote found in this repository".into(),
    ))
}

/// Fetch a branch from origin.
pub fn fetch_branch(branch: &str) -> Result<()> {
    git_with_timeout(&["fetch", "origin", "--", branch], GIT_WRITE_TIMEOUT)?;
    Ok(())
}

/// Checkout a local branch (creating it if it doesn't exist).
pub fn checkout_branch(branch: &str) -> Result<()> {
    // First check if branch exists locally (locale-independent)
    let exists = git(&["rev-parse", "--verify", branch]).is_ok();

    if exists {
        git(&["switch", "--", branch]).map(|_| ())
    } else {
        let remote_ref = format!("origin/{branch}");
        git(&["switch", "-c", branch, "--", &remote_ref]).map(|_| ())
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
    git_with_timeout(&["push", "origin", "--", branch], GIT_WRITE_TIMEOUT)?;
    Ok(())
}

/// Git push --force-with-lease to origin.
pub fn push_force_with_lease(branch: &str) -> Result<()> {
    git_with_timeout(
        &["push", "--force-with-lease", "origin", "--", branch],
        GIT_WRITE_TIMEOUT,
    )?;
    Ok(())
}

/// Delete a branch locally.
pub fn delete_branch_local(branch: &str) -> Result<()> {
    git(&["branch", "-D", "--", branch])?;
    Ok(())
}

/// Delete a branch locally, checking if it is fully merged (safe delete).
pub fn delete_branch_local_safe(branch: &str) -> Result<()> {
    git(&["branch", "-d", "--", branch])?;
    Ok(())
}

/// Delete a remote branch on origin.
pub fn delete_branch_remote(branch: &str) -> Result<()> {
    git_with_timeout(
        &["push", "origin", "--delete", "--", branch],
        GIT_WRITE_TIMEOUT,
    )?;
    Ok(())
}

/// Rebase branch onto another branch.
pub fn rebase_branch(branch: &str, onto: &str) -> Result<()> {
    // switch to the target branch first, then rebase onto the parent
    git(&["switch", "--", branch])?;
    git_with_timeout(&["rebase", "--", onto], GIT_WRITE_TIMEOUT)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Async wrappers using spawn_blocking for git write operations
// ---------------------------------------------------------------------------

/// Async version of [`fetch_branch`] — runs on the blocking thread pool.
pub async fn fetch_branch_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || fetch_branch(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`checkout_branch`] — runs on the blocking thread pool.
pub async fn checkout_branch_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || checkout_branch(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`push_branch`] — runs on the blocking thread pool.
pub async fn push_branch_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || push_branch(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`push_force_with_lease`] — runs on the blocking thread pool.
pub async fn push_force_with_lease_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || push_force_with_lease(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`delete_branch_local`] — runs on the blocking thread pool.
pub async fn delete_branch_local_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || delete_branch_local(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`delete_branch_local_safe`] — runs on the blocking thread pool.
pub async fn delete_branch_local_safe_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || delete_branch_local_safe(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`delete_branch_remote`] — runs on the blocking thread pool.
pub async fn delete_branch_remote_async(branch: &str) -> Result<()> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || delete_branch_remote(&branch))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
}

/// Async version of [`rebase_branch`] — runs on the blocking thread pool.
pub async fn rebase_branch_async(branch: &str, onto: &str) -> Result<()> {
    let branch = branch.to_string();
    let onto = onto.to_string();
    tokio::task::spawn_blocking(move || rebase_branch(&branch, &onto))
        .await
        .map_err(|e| BitbucketError::Git(format!("spawn_blocking join error: {e}")))?
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
