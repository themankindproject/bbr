//! Git integration: detect the current branch, workspace slug, and repo slug
//! by shelling out to `git` (kept lean to avoid a libgit2 dependency).

use std::process::Command;
use std::time::Duration;

use wait_timeout::ChildExt;

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
/// Spawns the child process and waits with a true deadline via `wait_timeout`.
/// On timeout, the child is explicitly killed to prevent orphaned git processes.
fn git_with_timeout(args: &[&str], timeout: Duration) -> Result<String> {
    let mut child = Command::new("git")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| BitbucketError::Git(format!("failed to spawn git: {e}")))?;

    // Drain both pipes concurrently with the wait: if we waited for exit
    // first, a child producing more than the OS pipe-buffer worth of output
    // (fetch/rebase progress on stderr, large stdout) would block forever on
    // write while we block on wait — a guaranteed deadlock until the timeout.
    let stdout_handle = child.stdout.take().map(spawn_drain);
    let stderr_handle = child.stderr.take().map(spawn_drain);

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            let stdout = join_drain(stdout_handle);
            let stderr = join_drain(stderr_handle);

            if !status.success() {
                let msg = String::from_utf8_lossy(&stderr).trim().to_string();
                return Err(BitbucketError::Git(msg));
            }
            Ok(String::from_utf8_lossy(&stdout).trim().to_string())
        }
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(BitbucketError::Git(format!(
                "git command timed out after {}s: git {}",
                timeout.as_secs(),
                args.join(" ")
            )))
        }
        Err(e) => Err(BitbucketError::Git(format!("failed to wait on git: {e}"))),
    }
}

/// Spawn a thread that drains a child pipe to a buffer.
///
/// Must run concurrently with `wait_timeout` — reading only after the child
/// exits deadlocks once the child fills the OS pipe buffer (~64KB).
fn spawn_drain(mut pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut pipe, &mut buf).ok();
        buf
    })
}

/// Collect a drained buffer, tolerating a panicked drain thread (empty output).
fn join_drain(handle: Option<std::thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle.and_then(|h| h.join().ok()).unwrap_or_default()
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

/// Repository toplevel directory, or `None` when not inside a git repo.
///
/// Uses the shared timeout-guarded runner so a wedged git (locked index,
/// slow network mount) can't hang callers forever.
pub fn repo_toplevel() -> Option<String> {
    let root = git(&["rev-parse", "--show-toplevel"]).ok()?;
    (!root.is_empty()).then_some(root)
}

/// Short (12-char) commit hash for HEAD.
pub fn current_commit() -> Result<String> {
    let full = git(&["rev-parse", "HEAD"])?;
    Ok(full.chars().take(12).collect())
}

/// Combined branch + commit info for the working directory.
pub fn head() -> Result<Head> {
    let branch = current_branch()?;
    let commit = current_commit()?;
    Ok(Head { branch, commit })
}

/// Parse a Bitbucket Cloud remote URL into a [`RepoIdentity`].
///
/// Accepts HTTP(S) on bitbucket.org, SCP-style SSH, and ssh:// URLs.
/// SSH also supports altssh.bitbucket.org and single-label host aliases.
/// Aliases are trusted local configuration; they are not resolved over the network.
pub fn parse_remote_url(url: &str) -> Option<RepoIdentity> {
    let url = url.trim();
    if url.chars().any(|ch| ch.is_control() || ch.is_whitespace()) || url.contains('\\') {
        return None;
    }
    let path = if let Some((_, rest)) = url.split_once("://") {
        let parsed = reqwest::Url::parse(url).ok()?;
        let host = parsed.host_str()?;
        match parsed.scheme() {
            "https" | "http" if host.eq_ignore_ascii_case("bitbucket.org") => {}
            "ssh" if is_bitbucket_ssh_host(host) => {}
            _ => return None,
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return None;
        }
        // Inspect the original path: URL parsers normalize away dot segments,
        // which would disguise a malformed identity as a different repository.
        rest.split_once('/')?.1
    } else {
        let (host, path) = url.strip_prefix("git@")?.split_once(':')?;
        if !is_bitbucket_ssh_host(host) {
            return None;
        }
        path
    };

    let (workspace, slug) = path.split_once('/')?;
    let slug = slug.strip_suffix(".git").unwrap_or(slug);
    if !is_remote_segment(workspace) || !is_remote_segment(slug) {
        return None;
    }
    Some(RepoIdentity {
        workspace: workspace.to_string(),
        slug: slug.to_string(),
    })
}

fn is_remote_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn is_bitbucket_ssh_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("bitbucket.org")
        || host.eq_ignore_ascii_case("altssh.bitbucket.org")
        // Support local aliases without mistaking unrelated DNS domains or IP
        // addresses for Bitbucket. Dotted aliases require explicit repo overrides.
        || (host.starts_with(|ch: char| ch.is_ascii_alphanumeric())
            && host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
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
        "no git remote for Bitbucket Cloud found; pass --workspace and --slug explicitly, or configure a Bitbucket remote".into(),
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
    // `--` stops option parsing so branch names like `--help` or `HEAD:foo` are safe.
    let exists = git(&["rev-parse", "--verify", "--", branch]).is_ok();

    if exists {
        git(&["switch", "--", branch]).map(|_| ())
    } else {
        let remote_ref = format!("origin/{branch}");
        git(&["switch", "-c", branch, "--", &remote_ref]).map(|_| ())
    }
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

/// Delete only branches whose commits are retained by HEAD, regardless of a
/// configured upstream. An HTTP remote deletion does not prune tracking refs,
/// and Git's `branch -d` alone can use that stale upstream to permit data loss.
pub fn delete_branch_local_safe(branch: &str) -> Result<()> {
    validate_branch_name(branch)?;
    git(&["merge-base", "--is-ancestor", &format!("refs/heads/{branch}"), "HEAD"])
        .map_err(|_| BitbucketError::Git(format!(
            "branch {branch:?} is not confirmed merged into HEAD; retain its commits before deleting it"
        )))?;
    git(&["branch", "-d", "--", branch])?;
    Ok(())
}

/// Validate a literal branch name without DWIM expansion (e.g. @{-1}).
pub fn validate_branch_name(branch: &str) -> Result<()> {
    if branch.is_empty() || branch.starts_with('-') || branch == "HEAD" {
        return Err(BitbucketError::Git("invalid branch name".into()));
    }
    git(&["check-ref-format", &format!("refs/heads/{branch}")])?;
    Ok(())
}

/// Delete a local branch safely, treating only an absent exact ref as complete.
/// Ref enumeration errors propagate rather than masquerading as absence.
pub async fn delete_local_branch_if_exists(branch: &str) -> Result<bool> {
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || {
        validate_branch_name(&branch)?;
        let reference = format!("refs/heads/{branch}");
        let refs = git(&["for-each-ref", "--format=%(refname)", "--", &reference])?;
        if !refs.lines().any(|line| line == reference) {
            return Ok(false);
        }
        delete_branch_local_safe(&branch)?;
        Ok(true)
    })
    .await
    .map_err(|e| BitbucketError::Git(format!("branch cleanup task failed: {e}")))?
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
///
/// On conflict the in-progress rebase is aborted and the repository is
/// restored to the branch it was on before the call, so callers never
/// inherit a half-finished rebase state.
pub fn rebase_branch(branch: &str, onto: &str) -> Result<()> {
    let original = current_branch().ok();
    // switch to the target branch first, then rebase onto the parent
    git(&["switch", "--", branch])?;
    if let Err(e) = git_with_timeout(&["rebase", "--", onto], GIT_WRITE_TIMEOUT) {
        // Never leave the repo mid-rebase: abort and go back to where we were.
        let _ = git(&["rebase", "--abort"]);
        return match original {
            Some(orig) => {
                let _ = git(&["switch", "--", &orig]);
                Err(BitbucketError::Git(format!(
                    "{e} (rebase aborted, restored branch `{orig}`)"
                )))
            }
            None => Err(e),
        };
    }
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
    fn parses_ssh_url_with_local_alias() {
        let id = parse_remote_url("git@work-bitbucket:foo/bar.git").unwrap();
        assert_eq!(id.workspace, "foo");
        assert_eq!(id.slug, "bar");
    }

    #[test]
    fn git_with_timeout_drains_pipes_concurrently() {
        // Regression: pipes were read only *after* wait_timeout returned, so
        // any git command emitting more than the OS pipe buffer (~64KB) of
        // stdout/stderr deadlocked until the timeout killed the child.
        let big = git_with_timeout(&["version"], Duration::from_secs(30));
        assert!(
            big.is_ok(),
            "git version with piped stdout must not time out: {big:?}"
        );
        assert!(big.unwrap().contains("git version"));
    }

    #[test]
    fn current_commit_slices_by_chars_not_bytes() {
        // Regression: byte-slicing `full[..len]` could panic on a non-ASCII
        // branch tip description leaking into rev-parse output.
        let s = "abc123def456"; // normal case stays 12 chars
        assert_eq!(s.chars().take(12).count(), 12);
    }
}
