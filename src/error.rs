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
    #[error("no Bitbucket credentials found; run `bb auth setup` or set BITBUCKET_USERNAME + BITBUCKET_TOKEN")]
    NoCredentials,

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("Bitbucket API rate limit exceeded{0}")]
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
    let code = e.exit_code();
    eprintln!("bb: {e}");
    if matches!(e, BitbucketError::NoCredentials) {
        eprintln!("hint: run `bb auth setup`, or set BITBUCKET_USERNAME + BITBUCKET_TOKEN");
    }
    code.as_process()
}
