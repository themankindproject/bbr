//! Subcommand implementations.

pub mod auth;
pub mod ci;
pub mod open;
pub mod pr;
pub mod repo;
pub mod status;

use crate::api::BitbucketClient;
use crate::cli::{resolve_api_base, GlobalArgs};
use crate::error::{BitbucketError, Result};
use crate::git::{self, RepoIdentity};

/// Resolve credentials + API base into a ready-to-use client.
pub fn client(g: &GlobalArgs) -> Result<BitbucketClient> {
    let creds = crate::auth::resolve()?;
    let base = resolve_api_base(g);
    creds.into_client(&base)
}

/// Detect the current repo identity from git, falling back to an explicit
/// workspace override stored in the credentials file.
pub fn current_repo() -> Result<RepoIdentity> {
    git::detect_repo()
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
