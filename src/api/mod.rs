//! Bitbucket Cloud REST API client and typed endpoint modules.

pub mod deploy;
pub mod issue;
pub mod pipeline;
pub mod pr;
pub mod repo;
pub mod source;
pub mod status;
pub mod webhook;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures::{StreamExt, TryStreamExt};
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
#[derive(Clone)]
pub struct BitbucketClient {
    base_url: String,
    inner: Client,
    creds: Credentials,
    auth_header: String,
}

impl std::fmt::Debug for BitbucketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BitbucketClient")
            .field("base_url", &self.base_url)
            .field("creds", &self.creds)
            .finish()
    }
}

impl BitbucketClient {
    /// Construct a new client. Uses rustls and a configurable timeout (default 30s).
    /// Auth is always HTTP Basic for Atlassian API tokens.
    pub fn new(base_url: &str, creds: Credentials) -> Result<Self> {
        Self::with_timeout(base_url, creds, 30)
    }

    /// Construct a new client with a specific timeout in seconds.
    pub fn with_timeout(base_url: &str, creds: Credentials, timeout_secs: u64) -> Result<Self> {
        let auth_header = match creds.kind {
            CredentialKind::ApiToken => {
                let raw = format!("{}:{}", creds.username, creds.secret);
                let encoded = base64_encode(raw.as_bytes());
                format!("Basic {encoded}")
            }
        };
        let inner = Client::builder()
            .user_agent(concat!("bbr/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(BitbucketError::Http)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            inner,
            creds,
            auth_header,
        })
    }

    /// Credentials accessor (used by `bbr auth status`).
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
    /// linear back-off (5 s, 10 s) + jitter, honoring the Retry-After header
    /// when present.
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
                    .body(b.to_owned());
            }
            let resp = req.send().await.map_err(BitbucketError::Http)?;

            // Extract Retry-After header before consuming the response body
            let retry_after_secs = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());

            match self.decode(resp).await {
                Err(BitbucketError::RateLimit(_msg)) if attempt < MAX_RETRIES => {
                    attempt += 1;
                    let wait = if let Some(ra) = retry_after_secs {
                        std::time::Duration::from_secs(ra)
                    } else {
                        let base = u64::from(attempt) * 5;
                        let jitter = rand_jitter();
                        std::time::Duration::from_secs(base + jitter)
                    };
                    tracing::warn!("rate limited, retrying in {:?} (attempt {attempt})", wait);
                    tokio::time::sleep(wait).await;
                }
                other => return other,
            }
        }
    }

    /// Issue a request expecting no meaningful response body (returns `()` on success).
    /// Only checks the HTTP status code; does not attempt to deserialize the body.
    pub async fn send_empty(&self, method: Method, path: &str, body: Option<&str>) -> Result<()> {
        self.send_no_body(method, path, body).await
    }

    /// Internal method that makes a request, checks the status code, handles errors,
    /// but does not deserialize the response body.
    async fn send_no_body(&self, method: Method, path: &str, body: Option<&str>) -> Result<()> {
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
                    .body(b.to_owned());
            }
            let resp = req.send().await.map_err(BitbucketError::Http)?;
            let status = resp.status();

            // Extract Retry-After header before consuming the response body
            let retry_after_secs = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());

            if status.is_success() {
                return Ok(());
            }

            let text = resp.text().await.map_err(BitbucketError::Http)?;
            let err = map_error(status, &text);
            match err {
                BitbucketError::RateLimit(_) if attempt < MAX_RETRIES => {
                    attempt += 1;
                    let wait = if let Some(ra) = retry_after_secs {
                        std::time::Duration::from_secs(ra)
                    } else {
                        let base = u64::from(attempt) * 5;
                        let jitter = rand_jitter();
                        std::time::Duration::from_secs(base + jitter)
                    };
                    tracing::warn!("rate limited, retrying in {:?} (attempt {attempt})", wait);
                    tokio::time::sleep(wait).await;
                }
                other => return Err(other),
            }
        }
    }

    /// POST a serializable body.
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        let raw = serde_json::to_string(body)?;
        self.send(Method::POST, path, Some(&raw)).await
    }

    pub async fn fetch_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        // Fetch the first page
        let first_page: crate::api::Paginated<T> = self.send(Method::GET, path, None).await?;
        let mut all = first_page.values;

        if all.len() >= limit || first_page.next.is_none() {
            all.truncate(limit);
            return Ok(all);
        }

        let size = first_page.size as usize;
        let pagelen = if first_page.pagelen > 0 {
            first_page.pagelen as usize
        } else {
            25
        };

        if size == 0 {
            // Fall back to sequential paging
            let mut next_path = strip_base(&first_page.next.unwrap(), &self.base_url)?;
            loop {
                let page: crate::api::Paginated<T> =
                    self.send(Method::GET, &next_path, None).await?;

                // Guard: break if the page returned no values to prevent infinite loop
                if page.values.is_empty() {
                    break;
                }

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
            all.truncate(limit);
            return Ok(all);
        }

        let total_needed = limit.min(size);
        if total_needed <= all.len() {
            all.truncate(total_needed);
            return Ok(all);
        }

        let num_pages = total_needed.div_ceil(pagelen);

        let mut futures = Vec::new();
        for p in 2..=num_pages {
            let p_path = if path.contains('?') {
                format!("{path}&page={p}")
            } else {
                format!("{path}?page={p}")
            };
            futures.push(async move {
                self.send::<crate::api::Paginated<T>>(Method::GET, &p_path, None)
                    .await
            });
        }

        // Cap parallel page fetches to 10 concurrent requests
        let results: Vec<crate::api::Paginated<T>> = futures::stream::iter(futures)
            .buffer_unordered(10)
            .try_collect()
            .await?;

        for page in results {
            all.extend(page.values);
        }

        all.truncate(limit);
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

            // Extract Retry-After header before consuming the response body
            let retry_after_secs = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());

            let body = resp.text().await.map_err(BitbucketError::Http)?;
            if !status.is_success() {
                let err = map_error(status, &body);
                match err {
                    BitbucketError::RateLimit(_msg) if attempt < MAX_RETRIES => {
                        attempt += 1;
                        let wait = if let Some(ra) = retry_after_secs {
                            std::time::Duration::from_secs(ra)
                        } else {
                            let base = u64::from(attempt) * 5;
                            let jitter = rand_jitter();
                            std::time::Duration::from_secs(base + jitter)
                        };
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
    #[serde(default)]
    detail: Option<serde_json::Value>,
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
    let detail = parsed.as_ref().and_then(|e| e.error.detail.as_ref());
    let fields = parsed
        .as_ref()
        .and_then(|e| e.error.fields.as_ref())
        .filter(|f| !f.is_null() && !f.as_object().is_none_or(|o| o.is_empty()));

    let mut full = msg;
    if let Some(d) = detail {
        match d {
            serde_json::Value::String(s) if !s.is_empty() => {
                if !full.is_empty() {
                    full.push_str(". ");
                }
                full.push_str(s);
            }
            serde_json::Value::Object(map) => {
                let required = map.get("required").and_then(|v| v.as_array());
                let granted = map.get("granted").and_then(|v| v.as_array());
                if required.is_some() || granted.is_some() {
                    let mut all: Vec<(&str, &str)> = Vec::new();
                    if let Some(req) = required {
                        for s in req.iter().filter_map(|v| v.as_str()) {
                            all.push((s, "MISSING"));
                        }
                    }
                    if let Some(grant) = granted {
                        for s in grant.iter().filter_map(|v| v.as_str()) {
                            if !all.iter().any(|(n, _)| *n == s) {
                                all.push((s, "✓"));
                            }
                        }
                        for s in grant.iter().filter_map(|v| v.as_str()) {
                            if let Some(entry) = all.iter_mut().find(|(n, _)| *n == s) {
                                entry.1 = "✓";
                            }
                        }
                    }
                    if !all.is_empty() {
                        if !full.is_empty() {
                            full.push('\n');
                        }
                        let max_w = all.iter().map(|(n, _)| n.len()).max().unwrap_or(0).max(5);
                        full.push_str(&format!("\n  {:<width$}  Status", "Scope", width = max_w));
                        full.push_str(&format!("\n  {}", "─".repeat(max_w + 8)));
                        for (name, status) in &all {
                            full.push_str(&format!(
                                "\n  {:<width$}  {}",
                                name,
                                status,
                                width = max_w
                            ));
                        }
                    }
                } else if !map.is_empty() {
                    if !full.is_empty() {
                        full.push_str(". ");
                    }
                    full.push_str(&serde_json::to_string(map).unwrap_or_default());
                }
            }
            _ => {}
        }
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
        StatusCode::UNAUTHORIZED => {
            let msg = if full.is_empty() || full.starts_with("HTTP ") {
                "HTTP 401: Unauthorized. Check your credentials are valid.".to_string()
            } else {
                format!("HTTP 401 Unauthorized: {full}")
            };
            BitbucketError::AuthFailed(msg)
        }
        StatusCode::FORBIDDEN => {
            let msg = if full.is_empty() || full.starts_with("HTTP ") {
                "HTTP 403: Permission denied. Your token may lack the required scopes.".to_string()
            } else {
                format!("HTTP 403 Forbidden: {full}")
            };
            BitbucketError::AuthFailed(msg)
        }
        StatusCode::NOT_FOUND => {
            let msg = if full.is_empty() || full.starts_with("HTTP ") {
                "HTTP 404: Not found. The resource or endpoint does not exist.".to_string()
            } else {
                format!("HTTP 404 Not Found: {full}")
            };
            BitbucketError::NotFound(msg)
        }
        StatusCode::TOO_MANY_REQUESTS => {
            BitbucketError::RateLimit(format!("HTTP {status}: {full}"))
        }
        StatusCode::BAD_REQUEST => BitbucketError::BadRequest(format!("HTTP {status}: {full}")),
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

/// Percent-encode a string for use in URL query parameters.
pub(crate) fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Simple jitter based on process ID and monotonic counter to avoid thundering herd.
fn rand_jitter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Spread across 0-4 seconds using a simple hash
    let pid = std::process::id() as u64;
    (pid.wrapping_add(n).wrapping_mul(6364136223846793005) >> 33) % 5
}

/// Base64 encoder using the `base64` crate (RFC 4648 standard alphabet).
pub(crate) fn base64_encode(input: &[u8]) -> String {
    STANDARD.encode(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn base64_roundtrip_basic() {
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"bar"), "YmFy");
        assert_eq!(base64_encode(b"a"), "YQ==");
    }

    #[test]
    fn url_appends_path_to_base() {
        let client = BitbucketClient {
            base_url: "https://api.bitbucket.org/2.0".into(),
            inner: Client::builder().build().unwrap(),
            creds: crate::auth::Credentials {
                username: "u".into(),
                secret: "s".into(),
                kind: crate::auth::CredentialKind::ApiToken,
            },
            auth_header: "Basic dTpz".into(),
        };
        assert_eq!(
            client.url("/repositories/ws/slug"),
            "https://api.bitbucket.org/2.0/repositories/ws/slug"
        );
        assert_eq!(
            client.url("repositories/ws/slug"),
            "https://api.bitbucket.org/2.0/repositories/ws/slug"
        );
    }

    #[test]
    fn map_error_auth_failed() {
        let body = r#"{"error":{"message":"access denied","detail":"invalid credentials"}}"#;
        let err = map_error(StatusCode::UNAUTHORIZED, body);
        assert!(matches!(err, BitbucketError::AuthFailed(_)));

        let err = map_error(StatusCode::FORBIDDEN, body);
        assert!(matches!(err, BitbucketError::AuthFailed(_)));
    }

    #[test]
    fn map_error_not_found() {
        let body = r#"{"error":{"message":"repository not found"}}"#;
        let err = map_error(StatusCode::NOT_FOUND, body);
        assert!(matches!(err, BitbucketError::NotFound(_)));
    }

    #[test]
    fn map_error_rate_limit() {
        let body = "rate limit exceeded";
        let err = map_error(StatusCode::TOO_MANY_REQUESTS, body);
        assert!(matches!(err, BitbucketError::RateLimit(_)));
    }

    #[test]
    fn map_error_other_status() {
        let body = "internal error";
        let err = map_error(StatusCode::INTERNAL_SERVER_ERROR, body);
        assert!(matches!(err, BitbucketError::Other(_)));
    }

    #[test]
    fn map_error_includes_scope_table() {
        let body = r#"{"error":{"message":"insufficient permissions","detail":{"required":["repo:write"],"granted":["repo:read"]}}}"#;
        let err = map_error(StatusCode::FORBIDDEN, body);
        let msg = format!("{err}");
        assert!(msg.contains("repo:write"));
        assert!(msg.contains("MISSING"));
        assert!(msg.contains("repo:read"));
    }

    #[test]
    fn map_error_falls_back_to_raw_body_when_not_json() {
        let err = map_error(StatusCode::BAD_REQUEST, "not valid json");
        let msg = format!("{err}");
        assert!(msg.contains("not valid json"));
    }

    #[test]
    fn strip_base_works() {
        let result = strip_base(
            "https://api.bitbucket.org/2.0/repositories/ws/r?page=2",
            "https://api.bitbucket.org/2.0",
        )
        .unwrap();
        assert_eq!(result, "/repositories/ws/r?page=2");
    }

    #[test]
    fn strip_base_errors_on_mismatch() {
        let err =
            strip_base("https://other.com/repos", "https://api.bitbucket.org/2.0").unwrap_err();
        assert!(matches!(err, BitbucketError::Other(_)));
    }

    #[test]
    fn one_line_truncates_to_300_chars() {
        let long = "a".repeat(400);
        let result = one_line(&long);
        assert_eq!(result.len(), 300);
    }

    #[test]
    fn one_line_replaces_newlines() {
        assert_eq!(one_line("hello\nworld"), "hello world");
    }

    #[test]
    fn paginated_deserializes_basic() {
        let json = r#"{"values":[{"id":1,"state":"OPEN","title":"Fix","source":{"branch":{"name":"f"}},"destination":{"branch":{"name":"main"}}}],"pagelen":25}"#;
        let page: Paginated<super::pr::PullRequest> = serde_json::from_str(json).unwrap();
        assert_eq!(page.values.len(), 1);
        assert_eq!(page.pagelen, 25);
        assert!(page.next.is_none());
    }

    #[test]
    fn paginated_handles_missing_fields() {
        let json = r#"{"values":[{"id":1,"state":"OPEN","source":{"branch":{"name":"f"}},"destination":{"branch":{"name":"m"}}}]}"#;
        let page: Paginated<super::pr::PullRequest> = serde_json::from_str(json).unwrap();
        assert_eq!(page.values.len(), 1);
        assert_eq!(page.size, 0);
        assert_eq!(page.page, 0);
    }
}
