//! Credential resolution for `bbr`.
//!
//! Order of precedence:
//! 1. Environment variables (`BITBUCKET_USERNAME`, `BITBUCKET_TOKEN`).
//! 2. Config file at [`crate::config::credentials_path`].
//!
//! All credentials use Atlassian API tokens (from id.atlassian.com) with
//! HTTP Basic authentication — no legacy PAT or AppPassword support.

use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::config::load_credentials;
use crate::error::{BitbucketError, Result};

/// Environment variable names.
pub const ENV_USERNAME: &str = "BITBUCKET_USERNAME";
pub const ENV_TOKEN: &str = "BITBUCKET_TOKEN";

/// Resolved credentials ready to attach to HTTP requests.
/// The `secret` field is zeroized on drop to prevent credential leakage
/// in memory dumps or core files.
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub secret: SecretString,
    pub kind: CredentialKind,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("secret", &"[REDACTED]")
            .field("kind", &self.kind)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// Atlassian API token from id.atlassian.com (Basic auth).
    ApiToken,
}

/// Resolve credentials from the environment first, then the config file.
pub fn resolve() -> Result<Credentials> {
    if let Some(c) = from_env() {
        return Ok(c);
    }
    if let Some(c) = from_config()? {
        return Ok(c);
    }
    Err(BitbucketError::NoCredentials)
}

fn from_env() -> Option<Credentials> {
    let username = std::env::var(ENV_USERNAME).ok()?;
    let token = std::env::var(ENV_TOKEN).ok()?;
    let username = username.trim().to_string();
    let token_trimmed = token.trim().to_string();
    if token_trimmed.is_empty() {
        tracing::warn!(
            "{ENV_TOKEN} is set but empty or whitespace-only; ignoring environment credentials. \
             Set a valid Atlassian API token from https://id.atlassian.com/manage-profile/security/api-tokens"
        );
        return None;
    }
    if username.is_empty() {
        tracing::warn!(
            "{ENV_USERNAME} is set but empty or whitespace-only; ignoring environment credentials. \
             Set both {ENV_USERNAME} and {ENV_TOKEN} for env-based auth."
        );
        return None;
    }
    Some(Credentials {
        username,
        secret: SecretString::from(token_trimmed),
        kind: CredentialKind::ApiToken,
    })
}

fn from_config() -> Result<Option<Credentials>> {
    let Some(file) = load_credentials()? else {
        return Ok(None);
    };
    let p = &file.default;
    let Some(secret) = p.secret() else {
        return Ok(None);
    };
    let username = p.username.trim();
    if username.is_empty() {
        return Ok(None);
    }
    Ok(Some(Credentials {
        username: username.to_string(),
        secret: SecretString::from(secret.to_string()),
        kind: CredentialKind::ApiToken,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_token_resolves_to_api_token() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(ENV_USERNAME, "u@example.com");
        std::env::set_var(ENV_TOKEN, "ATATT-example");
        let c = from_env().unwrap();
        assert_eq!(c.kind, CredentialKind::ApiToken);
        assert_eq!(c.username, "u@example.com");
        std::env::remove_var(ENV_TOKEN);
        std::env::remove_var(ENV_USERNAME);
    }

    #[test]
    fn env_returns_none_without_username() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(ENV_USERNAME);
        std::env::set_var(ENV_TOKEN, "tok");
        assert!(from_env().is_none());
        std::env::remove_var(ENV_TOKEN);
    }

    #[test]
    fn env_returns_none_with_empty_token() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(ENV_USERNAME, "u");
        std::env::set_var(ENV_TOKEN, "");
        assert!(from_env().is_none());
        std::env::remove_var(ENV_USERNAME);
        std::env::remove_var(ENV_TOKEN);
    }
}
