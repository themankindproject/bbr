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

/// Convenience: detect the repo identity and current HEAD together.
pub fn context() -> Result<(RepoIdentity, Head)> {
    let repo = detect_repo()?;
    let head = head()?;
    Ok((repo, head))
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
