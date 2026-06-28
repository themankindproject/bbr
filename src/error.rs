//! Centralized error type and exit-code mapping for `bb`.

use thiserror::Error;

/// Numeric exit codes used by `bb`.
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

/// All errors emitted by `bb` collapse into [`BitbucketError`].
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

    #[error("pipeline failed")]
    PipelineFailed,

    #[error("{0}")]
    Other(String),
}

impl BitbucketError {
    /// Map an error to its stable process [`ExitCode`].
    pub fn exit_code(&self) -> ExitCode {
        match self {
            BitbucketError::NoCredentials | BitbucketError::AuthFailed(_) => ExitCode::Auth,
            BitbucketError::NotFound(_) => ExitCode::NotFound,
            BitbucketError::RateLimit(_) => ExitCode::RateLimit,
            BitbucketError::PipelineFailed => ExitCode::PipelineFailed,
            _ => ExitCode::Generic,
        }
    }
}

pub type Result<T, E = BitbucketError> = std::result::Result<T, E>;

/// Convenience for the top-level `main`: print a friendly message to stderr
/// and return the right process exit code.
pub fn report(e: &BitbucketError) -> std::process::ExitCode {
    eprintln!("bb: {e}");
    if matches!(e, BitbucketError::NoCredentials) {
        eprintln!("hint: run `bbr auth setup`, or set BITBUCKET_USERNAME + BITBUCKET_TOKEN");
    }
    e.exit_code().as_process()
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
        let e = BitbucketError::PipelineFailed;
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
}
