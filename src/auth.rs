//! Credential resolution for `bb`.
//!
//! Order of precedence:
//! 1. Environment variables (`BITBUCKET_USERNAME`, `BITBUCKET_TOKEN`,
//!    `BITBUCKET_APP_PASSWORD`).
//! 2. Config file at [`crate::config::credentials_path`].
//! 3. System keyring (v0.3; not yet implemented).

use serde::{Deserialize, Serialize};

use crate::config::{load_credentials, CredentialProfile};
use crate::error::{BitbucketError, Result};

/// Environment variable names.
pub const ENV_USERNAME: &str = "BITBUCKET_USERNAME";
pub const ENV_TOKEN: &str = "BITBUCKET_TOKEN";
pub const ENV_APP_PASSWORD: &str = "BITBUCKET_APP_PASSWORD";

/// Resolved credentials ready to attach to HTTP requests.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub username: String,
    /// The bearer-style secret (PAT or app password). PATs are sent as
    /// `Bearer <token>`; app passwords use HTTP Basic.
    pub secret: String,
    pub kind: CredentialKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// Bitbucket Personal Access Token (Bearer auth).
    Pat,
    /// Legacy app password (Basic auth).
    AppPassword,
    /// Atlassian API token from id.atlassian.com (Basic auth, same as app password).
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
    if let Ok(token) = std::env::var(ENV_TOKEN) {
        if !token.is_empty() {
            let kind = if token.starts_with("ATATT") {
                CredentialKind::ApiToken
            } else {
                CredentialKind::Pat
            };
            return Some(Credentials {
                username,
                secret: token,
                kind,
            });
        }
    }
    if let Ok(pw) = std::env::var(ENV_APP_PASSWORD) {
        if !pw.is_empty() {
            return Some(Credentials {
                username,
                secret: pw,
                kind: CredentialKind::AppPassword,
            });
        }
    }
    None
}

fn from_config() -> Result<Option<Credentials>> {
    let Some(file) = load_credentials()? else {
        return Ok(None);
    };
    let p: &CredentialProfile = &file.default;
    let Some(secret) = p.secret() else {
        return Ok(None);
    };
    if p.username.is_empty() {
        return Ok(None);
    }
    let kind = if p.is_pat() {
        if p.is_atlassian_api_token() {
            CredentialKind::ApiToken
        } else {
            CredentialKind::Pat
        }
    } else {
        CredentialKind::AppPassword
    };
    Ok(Some(Credentials {
        username: p.username.clone(),
        secret: secret.to_string(),
        kind,
    }))
}

impl Credentials {
    /// Build a `reqwest` client pre-configured with the right auth header.
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
    fn env_pat_wins_over_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(ENV_USERNAME, "u");
        std::env::set_var(ENV_TOKEN, "tok");
        std::env::remove_var(ENV_APP_PASSWORD);
        let c = from_env().unwrap();
        assert_eq!(c.kind, CredentialKind::Pat);
        std::env::remove_var(ENV_TOKEN);
        std::env::remove_var(ENV_USERNAME);
    }

    #[test]
    fn env_api_token_is_detected() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(ENV_USERNAME, "u");
        std::env::set_var(ENV_TOKEN, "ATATT-example");
        std::env::remove_var(ENV_APP_PASSWORD);
        let c = from_env().unwrap();
        assert_eq!(c.kind, CredentialKind::ApiToken);
        std::env::remove_var(ENV_TOKEN);
        std::env::remove_var(ENV_USERNAME);
    }

    #[test]
    fn env_app_password_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(ENV_USERNAME, "u");
        std::env::remove_var(ENV_TOKEN);
        std::env::set_var(ENV_APP_PASSWORD, "pw");
        let c = from_env().unwrap();
        assert_eq!(c.kind, CredentialKind::AppPassword);
        std::env::remove_var(ENV_APP_PASSWORD);
        std::env::remove_var(ENV_USERNAME);
    }
}
