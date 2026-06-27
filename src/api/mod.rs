//! Bitbucket Cloud REST API client and typed endpoint modules.

pub mod pipeline;
pub mod pr;
pub mod repo;
pub mod status;

use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::auth::{CredentialKind, Credentials};
use crate::error::{BitbucketError, Result};

/// Default page size when listing collections.
pub const DEFAULT_PAGE_SIZE: u32 = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paginated<T> {
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub page: u64,
    #[serde(default)]
    pub pagelen: u64,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub previous: Option<String>,
    pub values: Vec<T>,
}

/// Bitbucket Cloud REST API v2 wrapper.
#[derive(Debug, Clone)]
pub struct BitbucketClient {
    base_url: String,
    inner: Client,
    creds: Credentials,
    auth_header: String,
}

impl BitbucketClient {
    /// Construct a new client. Uses rustls and a 30s timeout.
    pub fn new(base_url: &str, creds: Credentials) -> Result<Self> {
        let auth_header = match creds.kind {
            CredentialKind::Pat => format!("Bearer {}", creds.secret),
            CredentialKind::AppPassword | CredentialKind::ApiToken => {
                let raw = format!("{}:{}", creds.username, creds.secret);
                let encoded = base64_encode(raw.as_bytes());
                format!("Basic {encoded}")
            }
        };
        let inner = Client::builder()
            .user_agent(concat!("bbr/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(BitbucketError::Http)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            inner,
            creds,
            auth_header,
        })
    }

    /// Credentials accessor (used by `bb auth status`).
    pub fn creds(&self) -> &Credentials {
        &self.creds
    }

    /// Build a full URL from a path (path may start with `/`).
    pub fn url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/{path}", self.base_url)
    }

    /// Issue a request and return the deserialized body.
    /// Automatically retries up to 2 times on HTTP 429 (rate-limit) with
    /// linear back-off (5 s, 10 s) + jitter.
    pub async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<T> {
        const MAX_RETRIES: u8 = 2;
        let mut attempt: u8 = 0;
        let url = self.url(path);
        loop {
            let mut req = self
                .inner
                .request(method.clone(), &url)
                .header(AUTHORIZATION, &self.auth_header)
                .header(ACCEPT, "application/json");
            if let Some(b) = body {
                req = req
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(b.to_string());
            }
            let resp = req.send().await.map_err(BitbucketError::Http)?;
            match self.decode(resp).await {
                Err(BitbucketError::RateLimit(_msg)) if attempt < MAX_RETRIES => {
                    attempt += 1;
                    let base = u64::from(attempt) * 5;
                    let jitter = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .subsec_nanos()
                        % 3;
                    let wait = std::time::Duration::from_secs(base + u64::from(jitter));
                    tracing::warn!("rate limited, retrying in {:?} (attempt {attempt})", wait);
                    tokio::time::sleep(wait).await;
                }
                other => return other,
            }
        }
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

    /// Fetch all pages of a paginated endpoint, up to `limit`.
    /// Follows `next` links until the limit is reached or there are no more pages.
    pub async fn fetch_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut next_path = path.to_string();
        loop {
            let page: crate::api::Paginated<T> = self.send(Method::GET, &next_path, None).await?;
            let remaining = limit.saturating_sub(all.len());
            all.extend(page.values.into_iter().take(remaining));
            if all.len() >= limit {
                break;
            }
            match page.next {
                Some(next_url) => {
                    next_path = strip_base(&next_url, &self.base_url)?;
                }
                None => break,
            }
        }
        Ok(all)
    }

    /// Issue a request and return the raw text body.
    /// Used for non-JSON endpoints (e.g. diff, logs).
    pub async fn send_raw(&self, method: Method, path: &str, accept: &str) -> Result<String> {
        const MAX_RETRIES: u8 = 2;
        let mut attempt: u8 = 0;
        let url = self.url(path);
        loop {
            let resp = self
                .inner
                .request(method.clone(), &url)
                .header(AUTHORIZATION, &self.auth_header)
                .header(ACCEPT, accept)
                .send()
                .await
                .map_err(BitbucketError::Http)?;
            let status = resp.status();
            let body = resp.text().await.map_err(BitbucketError::Http)?;
            if !status.is_success() {
                let err = map_error(status, &body);
                match err {
                    BitbucketError::RateLimit(_msg) if attempt < MAX_RETRIES => {
                        attempt += 1;
                        let base = u64::from(attempt) * 5;
                        let jitter = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .subsec_nanos()
                            % 3;
                        let wait = std::time::Duration::from_secs(base + u64::from(jitter));
                        tracing::warn!("rate limited, retrying in {:?} (attempt {attempt})", wait);
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    other => return Err(other),
                }
            }
            return Ok(body);
        }
    }

    async fn decode<T: DeserializeOwned>(&self, resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        let text = resp.text().await.map_err(BitbucketError::Http)?;

        if status.is_success() {
            if text.is_empty() {
                return serde_json::from_str("null").map_err(BitbucketError::Json);
            }
            return serde_json::from_str(&text).map_err(|e| {
                tracing::debug!("JSON decode failed for {status}: {text:.200}");
                BitbucketError::Json(e)
            });
        }

        Err(map_error(status, &text))
    }
}

/// Bitbucket standardized error envelope.
#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
    detail: Option<String>,
    #[serde(default)]
    fields: Option<serde_json::Value>,
}

/// Map an HTTP failure status into the right [`BitbucketError`] variant.
pub fn map_error(status: StatusCode, body: &str) -> BitbucketError {
    let parsed: Option<ApiErrorEnvelope> = serde_json::from_str(body).ok();
    let msg = parsed
        .as_ref()
        .and_then(|e| e.error.message.as_deref())
        .unwrap_or("")
        .to_string();
    let detail = parsed
        .as_ref()
        .and_then(|e| e.error.detail.as_deref())
        .filter(|d| !d.is_empty());
    let fields = parsed
        .as_ref()
        .and_then(|e| e.error.fields.as_ref())
        .filter(|f| !f.is_null() && !f.as_object().map_or(true, |o| o.is_empty()));

    let mut full = msg;
    if let Some(d) = detail {
        if !full.is_empty() {
            full.push_str(". ");
        }
        full.push_str(d);
    }
    if let Some(f) = fields {
        if !full.is_empty() {
            full.push(' ');
        }
        if let Some(map) = f.as_object() {
            let pairs: Vec<String> = map
                .iter()
                .filter_map(|(k, v)| {
                    let arr = v.as_array()?;
                    let items: Vec<String> = arr
                        .iter()
                        .filter_map(|e| e.as_str().map(|s| format!("{k}: {s}")))
                        .collect();
                    if items.is_empty() {
                        None
                    } else {
                        Some(items.join("; "))
                    }
                })
                .collect();
            if !pairs.is_empty() {
                full.push_str(&format!("({})", pairs.join("; ")));
            }
        }
    }
    if full.is_empty() {
        full = one_line(body);
    }

    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => BitbucketError::AuthFailed(full),
        StatusCode::NOT_FOUND => BitbucketError::NotFound(full),
        StatusCode::TOO_MANY_REQUESTS => {
            BitbucketError::RateLimit(format!("HTTP {status}: {full}"))
        }
        _ => BitbucketError::Other(format!("HTTP {status}: {full}")),
    }
}

/// Strip the API base URL from an absolute `next` URL to get a relative path.
fn strip_base(url: &str, base: &str) -> Result<String> {
    url.strip_prefix(base)
        .map(|s| s.to_string())
        .ok_or_else(|| BitbucketError::Other(format!("next URL does not match base: {url}")))
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
