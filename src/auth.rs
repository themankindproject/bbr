//! Credential resolution for `bb`.
//!
//! Order of precedence:
//! 1. Environment variables (`BITBUCKET_USERNAME`, `BITBUCKET_TOKEN`).
//! 2. Config file at [`crate::config::credentials_path`].
//!
//! All credentials use Atlassian API tokens (from id.atlassian.com) with
//! HTTP Basic authentication — no legacy PAT or AppPassword support.

use serde::{Deserialize, Serialize};

use crate::config::load_credentials;
use crate::error::{BitbucketError, Result};

/// Environment variable names.
pub const ENV_USERNAME: &str = "BITBUCKET_USERNAME";
pub const ENV_TOKEN: &str = "BITBUCKET_TOKEN";

/// Resolved credentials ready to attach to HTTP requests.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    pub secret: String,
    pub kind: CredentialKind,
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
    if token.is_empty() {
        return None;
    }
    Some(Credentials {
        username,
        secret: token,
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
    if p.username.is_empty() {
        return Ok(None);
    }
    Ok(Some(Credentials {
        username: p.username.clone(),
        secret: secret.to_string(),
        kind: CredentialKind::ApiToken,
    }))
}

impl Credentials {
    /// Build a `reqwest` client pre-configured with Basic auth.
    pub fn into_client(self, base_url: &str) -> Result<crate::api::BitbucketClient> {
        crate::api::BitbucketClient::new(base_url, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_token_resolves_to_api_token() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENV_USERNAME);
        std::env::set_var(ENV_TOKEN, "tok");
        assert!(from_env().is_none());
        std::env::remove_var(ENV_TOKEN);
    }

    #[test]
    fn env_returns_none_with_empty_token() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(ENV_USERNAME, "u");
        std::env::set_var(ENV_TOKEN, "");
        assert!(from_env().is_none());
        std::env::remove_var(ENV_USERNAME);
        std::env::remove_var(ENV_TOKEN);
    }
}
