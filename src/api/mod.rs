//! Bitbucket Cloud REST API client and typed endpoint modules.

pub mod pipeline;
pub mod pr;
pub mod repo;
pub mod status;

use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Serialize};

use crate::auth::{CredentialKind, Credentials};
use crate::error::{BitbucketError, Result};

/// Default page size when listing collections.
pub const DEFAULT_PAGE_SIZE: u32 = 25;

/// Bitbucket Cloud REST API v2 wrapper.
#[derive(Debug, Clone)]
pub struct BitbucketClient {
    base_url: String,
    inner: Client,
    creds: Credentials,
}

impl BitbucketClient {
    /// Construct a new client. Uses rustls and a 30s timeout.
    pub fn new(base_url: &str, creds: Credentials) -> Result<Self> {
        let inner = Client::builder()
            .user_agent(concat!("bbr/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(BitbucketError::Http)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            inner,
            creds,
        })
    }

    /// Credentials accessor (used by `bb auth status`).
    pub fn creds(&self) -> &Credentials {
        &self.creds
    }

    fn auth_header(&self) -> String {
        match self.creds.kind {
            CredentialKind::Pat => format!("Bearer {}", self.creds.secret),
            CredentialKind::AppPassword => {
                // HTTP Basic with username:app-password
                let raw = format!("{}:{}", self.creds.username, self.creds.secret);
                let encoded = base64_encode(raw.as_bytes());
                format!("Basic {encoded}")
            }
        }
    }

    /// Build a full URL from a path (path may start with `/`).
    pub fn url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/{path}", self.base_url)
    }

    /// Issue a request and return the deserialized body.
    pub async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<T> {
        let url = self.url(path);
        let mut req = self
            .inner
            .request(method, &url)
            .header(AUTHORIZATION, self.auth_header())
            .header(ACCEPT, "application/json");
        if let Some(b) = body {
            req = req
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(b.to_string());
        }
        let resp = req.send().await.map_err(BitbucketError::Http)?;
        self.decode(resp).await
    }

    /// Issue a request expecting no body (returns `()` on success).
    pub async fn send_empty(&self, method: Method, path: &str, body: Option<&str>) -> Result<()> {
        // Bitbucket returns 201/204 with empty or minimal bodies; decode as
        // serde_json::Value and ignore.
        let _: serde_json::Value = self.send(method, path, body).await?;
        Ok(())
    }

    /// POST a serializable body.
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        let raw = serde_json::to_string(body)?;
        self.send(Method::POST, path, Some(&raw)).await
    }

    async fn decode<T: DeserializeOwned>(&self, resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if status.is_success() {
            if text.is_empty() {
                return serde_json::from_str("null").map_err(BitbucketError::Json);
            }
            return serde_json::from_str(&text).map_err(BitbucketError::Json);
        }

        Err(map_error(status, &text))
    }
}

/// Map an HTTP failure status into the right [`BitbucketError`] variant.
pub fn map_error(status: StatusCode, body: &str) -> BitbucketError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            BitbucketError::AuthFailed(format!("HTTP {status}: {}", one_line(body)))
        }
        StatusCode::NOT_FOUND => BitbucketError::NotFound(one_line(body)),
        StatusCode::TOO_MANY_REQUESTS => BitbucketError::RateLimit(format!(": HTTP {status}")),
        _ => BitbucketError::Other(format!("HTTP {status}: {}", one_line(body))),
    }
}

fn one_line(s: &str) -> String {
    s.trim().replace('\n', " ").chars().take(300).collect()
}

/// Minimal base64 encoder (RFC 4648) — avoids pulling in a base64 crate just
/// for HTTP Basic auth.
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(TABLE[(b[0] >> 2) as usize] as char);
        out.push(TABLE[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b[2] & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip_basic() {
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"bar"), "YmFy");
        assert_eq!(base64_encode(b"a"), "YQ==");
    }
}
