//! Centralized error type and exit-code mapping for `bbr`.

use std::io::Write;

use thiserror::Error;

use crate::output::theme::Theme;

/// Numeric exit codes used by `bbr`.
///
/// These are stable and part of the public contract (documented in the README),
/// so CI scripts and coding agents can branch on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    Generic = 1,
    Auth = 2,
    NotFound = 3,
    RateLimit = 4,
    PipelineFailed = 5,
}

impl ExitCode {
    /// Convert to a [`std::process::ExitCode`].
    pub fn as_process(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self as u8)
    }
}

/// All errors emitted by `bbr` collapse into [`BitbucketError`].
#[derive(Debug, Error)]
pub enum BitbucketError {
    #[error("no Bitbucket credentials found; run `bbr auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN")]
    NoCredentials,

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("Bitbucket API rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("git error: {0}")]
    Git(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("pipeline{} failed{}",
        build_number.map(|n| format!(" #{n}")).unwrap_or_default(),
        branch.as_ref().map(|b| format!(" on {b}")).unwrap_or_default()
    )]
    PipelineFailed {
        build_number: Option<u64>,
        branch: Option<String>,
    },

    #[error("{0}")]
    Other(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    /// A 5xx server error, carrying the HTTP status so retry logic can
    /// branch on the code structurally instead of string-matching message
    /// text (which breaks silently if error formatting ever changes).
    #[error("HTTP {status}: server error")]
    Server {
        status: reqwest::StatusCode,
        #[source]
        source: Box<BitbucketError>,
    },
}

impl BitbucketError {
    /// Map an error to its stable process [`ExitCode`].
    pub fn exit_code(&self) -> ExitCode {
        match self {
            BitbucketError::NoCredentials | BitbucketError::AuthFailed(_) => ExitCode::Auth,
            BitbucketError::NotFound(_) => ExitCode::NotFound,
            BitbucketError::RateLimit(_) => ExitCode::RateLimit,
            BitbucketError::PipelineFailed { .. } => ExitCode::PipelineFailed,
            BitbucketError::Server { source, .. } => source.exit_code(),
            _ => ExitCode::Generic,
        }
    }

    /// Stable machine-readable error kind (used by `--json` mode).
    pub fn kind(&self) -> &'static str {
        match self {
            BitbucketError::NoCredentials | BitbucketError::AuthFailed(_) => "auth",
            BitbucketError::NotFound(_) => "not_found",
            BitbucketError::RateLimit(_) => "rate_limit",
            BitbucketError::Http(_) => "http",
            BitbucketError::Json(_) => "json",
            BitbucketError::Config(_) => "config",
            BitbucketError::Git(_) => "git",
            BitbucketError::Io(_) => "io",
            BitbucketError::PipelineFailed { .. } => "pipeline_failed",
            BitbucketError::Other(_) => "generic",
            BitbucketError::BadRequest(_) => "bad_request",
            BitbucketError::Server { .. } => "server",
        }
    }
}

pub type Result<T, E = BitbucketError> = std::result::Result<T, E>;

/// Hints to show beneath an error, phrased as actions the user can take.
fn hints(e: &BitbucketError) -> Vec<String> {
    let mut out = Vec::new();
    match e {
        BitbucketError::NoCredentials => {
            out.push("run `bbr auth setup`, or set BITBUCKET_USERNAME + BITBUCKET_TOKEN".into());
        }
        BitbucketError::AuthFailed(_) => {
            out.push("verify your token is valid and has the required scopes.".into());
            out.push(
                "create a new token at https://id.atlassian.com/manage-profile/security/api-tokens"
                    .into(),
            );
            out.push(
                "required scopes include at minimum: read:user:bitbucket, \
                 read:repository:bitbucket, read:pullrequest:bitbucket, \
                 read:pipeline:bitbucket"
                    .into(),
            );
        }
        BitbucketError::RateLimit(_) => {
            out.push("wait a few minutes or lower your request frequency.".into());
        }
        BitbucketError::NotFound(_) => {
            out.push("double-check the ID / name, or the workspace and repo slug.".into());
        }
        BitbucketError::Git(msg) => {
            if msg.contains("no git remote") {
                out.push("run bbr from inside a Bitbucket repo, or pass".into());
                out.push("--workspace <ws> --slug <slug> to point at a repo.".into());
            } else if msg.contains("not a git repository") {
                out.push("run bbr from inside a Bitbucket git repository.".into());
            } else if msg.contains("HEAD is detached") {
                out.push("check out a branch first: `git switch <branch>`.".into());
            }
        }
        BitbucketError::Config(_) => {
            out.push("check `bbr config show` and `bbr config path` for the active file.".into());
        }
        BitbucketError::Http(e) if e.is_timeout() => {
            out.push(
                "the request timed out (default: 30s). Try again or check your network.".into(),
            );
            out.push("adjust with --timeout <secs> or BBR_TIMEOUT.".into());
        }
        BitbucketError::Http(_) => {
            out.push("the request failed at the network layer. Check your connection.".into());
        }
        BitbucketError::PipelineFailed {
            build_number,
            branch,
        } => {
            if let Some(bn) = build_number {
                out.push(format!("pipeline build #{bn} failed."));
            }
            if let Some(br) = branch {
                out.push(format!("branch: {br}"));
            }
            out.push("run `bbr ci logs` to see the failure output.".into());
        }
        BitbucketError::BadRequest(_) => {
            out.push("check the arguments you passed — the API rejected the request.".into());
        }
        BitbucketError::Server { .. } => {
            out.push("the Bitbucket API had a server-side error; retrying usually helps.".into());
        }
        _ => {}
    }
    out
}

/// Render a `BitbucketError` for a human, as a single block of `stderr` text
/// (no trailing newline). Honors the active [`Theme`] for color / unicode.
pub fn display_error(e: &BitbucketError) -> String {
    let theme = Theme::current();
    let mut s = format!("bbr: {}", theme.error(&e.to_string()));
    for h in hints(e) {
        s.push_str(&format!("\n  {} {h}", theme.dim("hint:")));
    }
    s
}

/// Print a human-readable error to stderr and return the mapped exit code.
/// Honors the active theme; safe to call in non-TTY and `--no-color` contexts.
pub fn report(e: &BitbucketError) -> std::process::ExitCode {
    eprintln!("{}", display_error(e));
    e.exit_code().as_process()
}

/// Emit a stable machine-readable error object to stderr and return the
/// mapped exit code. Used when `--json` is set so scripts can parse failures.
pub fn report_json(e: &BitbucketError) -> std::process::ExitCode {
    let code = e.exit_code() as u8;
    let body = serde_json::json!({
        "error": {
            "kind": e.kind(),
            "exit_code": code,
            "message": e.to_string(),
        }
    });
    let mut err = std::io::stderr().lock();
    let _ = serde_json::to_writer_pretty(&mut err, &body);
    let _ = err.write_all(b"\n");
    code.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_credentials_maps_to_auth_exit() {
        let e = BitbucketError::NoCredentials;
        assert_eq!(e.exit_code(), ExitCode::Auth);
    }

    #[test]
    fn not_found_gives_notfound_exit() {
        let e = BitbucketError::NotFound("missing".into());
        assert_eq!(e.exit_code(), ExitCode::NotFound);
    }

    #[test]
    fn generic_other_is_exit_code_1() {
        let e = BitbucketError::Other("something went wrong".into());
        assert_eq!(e.exit_code(), ExitCode::Generic);
    }

    #[test]
    fn rate_limit_maps_correctly() {
        let e = BitbucketError::RateLimit("".into());
        assert_eq!(e.exit_code(), ExitCode::RateLimit);
    }

    #[test]
    fn rate_limit_display_separates_context() {
        let e = BitbucketError::RateLimit("HTTP 429: retry later".into());
        assert_eq!(
            format!("{e}"),
            "Bitbucket API rate limit exceeded: HTTP 429: retry later"
        );
    }

    #[test]
    fn pipeline_failed_maps_correctly() {
        let e = BitbucketError::PipelineFailed {
            build_number: Some(42),
            branch: Some("main".into()),
        };
        assert_eq!(e.exit_code(), ExitCode::PipelineFailed);
    }

    #[test]
    fn auth_failed_maps_to_auth() {
        let e = BitbucketError::AuthFailed("bad token".into());
        assert_eq!(e.exit_code(), ExitCode::Auth);
    }

    #[test]
    fn full_display_includes_cause() {
        let e = BitbucketError::Other("disk full".into());
        let msg = format!("{e}");
        assert!(msg.contains("disk full"));
    }

    #[test]
    fn kind_covers_all_variants() {
        assert_eq!(BitbucketError::NoCredentials.kind(), "auth");
        assert_eq!(BitbucketError::AuthFailed("x".into()).kind(), "auth");
        assert_eq!(BitbucketError::NotFound("x".into()).kind(), "not_found");
        assert_eq!(BitbucketError::RateLimit("x".into()).kind(), "rate_limit");
        assert_eq!(BitbucketError::Config("x".into()).kind(), "config");
        assert_eq!(BitbucketError::Git("x".into()).kind(), "git");
        assert_eq!(BitbucketError::Io(std::io::Error::other("x")).kind(), "io");
        assert_eq!(
            BitbucketError::PipelineFailed {
                build_number: None,
                branch: None,
            }
            .kind(),
            "pipeline_failed"
        );
        assert_eq!(BitbucketError::Other("x".into()).kind(), "generic");
        assert_eq!(BitbucketError::BadRequest("x".into()).kind(), "bad_request");
        assert_eq!(
            BitbucketError::Server {
                status: reqwest::StatusCode::BAD_GATEWAY,
                source: Box::new(BitbucketError::Other("boom".into())),
            }
            .kind(),
            "server"
        );
    }

    #[test]
    fn hints_exist_for_common_errors() {
        assert!(!hints(&BitbucketError::NoCredentials).is_empty());
        assert!(!hints(&BitbucketError::AuthFailed("x".into())).is_empty());
        assert!(!hints(&BitbucketError::RateLimit("x".into())).is_empty());
        assert!(!hints(&BitbucketError::NotFound("x".into())).is_empty());
        assert!(!hints(&BitbucketError::Config("x".into())).is_empty());
        assert!(!hints(&BitbucketError::BadRequest("x".into())).is_empty());
        assert!(!hints(&BitbucketError::Git("no git remote found".into())).is_empty());
        assert!(!hints(&BitbucketError::Git("HEAD is detached".into())).is_empty());
        assert!(hints(&BitbucketError::Other("x".into())).is_empty());
    }

    #[test]
    fn display_error_includes_hints() {
        let e = BitbucketError::NoCredentials;
        let s = display_error(&e);
        assert!(s.contains("bbr:"));
        assert!(s.contains("hint:"));
        assert!(s.contains("auth setup"));
    }
}
