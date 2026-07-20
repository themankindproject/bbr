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
use reqwest::header::{ACCEPT, AUTHORIZATION, ETAG, IF_NONE_MATCH};
use reqwest::{Client, Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::auth::{CredentialKind, Credentials};
use crate::error::{BitbucketError, Result};

/// Default page size when listing collections.
pub const DEFAULT_PAGE_SIZE: u32 = 25;

/// Warn when remaining API quota drops below this threshold.
const RATE_LIMIT_WARN_THRESHOLD: u64 = 50;

#[derive(Clone)]
struct CachedResponse {
    etag: String,
    body: String,
}

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
    /// `Basic base64(username:token)` — zeroized on drop via `SecretString`.
    auth_header: SecretString,
    /// In-process ETag + body cache keyed by request path, for conditional GETs.
    etag_cache: std::sync::Arc<Mutex<std::collections::HashMap<String, CachedResponse>>>,
    /// Last known rate-limit remaining (from `X-RateLimit-Remaining`).
    rate_limit_remaining: std::sync::Arc<std::sync::atomic::AtomicU64>,
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
                let raw = format!("{}:{}", creds.username, creds.secret.expose_secret());
                let encoded = base64_encode(raw.as_bytes());
                SecretString::from(format!("Basic {encoded}"))
            }
        };
        let inner = Client::builder()
            .user_agent(concat!("bbr/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_nodelay(true)
            .build()
            .map_err(BitbucketError::Http)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            inner,
            creds,
            auth_header,
            etag_cache: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            rate_limit_remaining: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX)),
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

    /// Last known rate-limit remaining (from `X-RateLimit-Remaining`).
    /// Returns `None` if no rate-limit header has been seen yet.
    pub fn rate_limit_remaining(&self) -> Option<u64> {
        let v = self
            .rate_limit_remaining
            .load(std::sync::atomic::Ordering::Relaxed);
        (v != u64::MAX).then_some(v)
    }

    fn auth_header_value(&self) -> &str {
        self.auth_header.expose_secret()
    }

    /// Extract rate-limit headers from a response and update internal state.
    fn update_rate_limit(&self, headers: &reqwest::header::HeaderMap) {
        if let Some(v) = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(n) = v.trim().parse::<u64>() {
                self.rate_limit_remaining
                    .store(n, std::sync::atomic::Ordering::Relaxed);
                if n < RATE_LIMIT_WARN_THRESHOLD {
                    tracing::warn!(
                        "Bitbucket API rate limit low: {n} requests remaining (threshold {RATE_LIMIT_WARN_THRESHOLD})"
                    );
                }
            }
        }
    }

    /// Look up a cached ETag for the given path (for conditional GETs).
    fn get_etag(&self, path: &str) -> Option<String> {
        self.etag_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(path).map(|c| c.etag.clone()))
    }

    /// Look up a cached response body for a 304 Not Modified hit.
    fn get_cached_body(&self, path: &str) -> Option<String> {
        self.etag_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(path).map(|c| c.body.clone()))
    }

    /// Store an ETag and body from a response for future conditional GETs.
    fn store_etag(&self, path: &str, headers: &reqwest::header::HeaderMap, body: &str) {
        if let Some(etag) = headers.get(ETAG).and_then(|v| v.to_str().ok()) {
            if !etag.is_empty() {
                if let Ok(mut cache) = self.etag_cache.lock() {
                    cache.insert(
                        path.to_string(),
                        CachedResponse {
                            etag: etag.to_string(),
                            body: body.to_string(),
                        },
                    );
                }
            }
        }
    }

    /// Compute the retry wait duration from a Retry-After header or backoff+jitter.
    fn retry_wait(attempt: u8, retry_after_secs: Option<u64>) -> std::time::Duration {
        if let Some(ra) = retry_after_secs {
            return std::time::Duration::from_secs(ra);
        }
        let base = u64::from(attempt) * 5;
        let jitter = rand_jitter();
        std::time::Duration::from_secs(base + jitter)
    }

    /// Determine whether an error/status is retryable (429 or 5xx).
    fn is_retryable_error(err: &BitbucketError) -> bool {
        match err {
            BitbucketError::RateLimit(_) => true,
            BitbucketError::Other(msg) => {
                msg.starts_with("HTTP 500")
                    || msg.starts_with("HTTP 502")
                    || msg.starts_with("HTTP 503")
                    || msg.starts_with("HTTP 504")
            }
            _ => false,
        }
    }

    fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
    }

    /// Shared retry loop for HTTP requests.
    async fn with_retries<T, F, Fut>(&self, path: &str, mut attempt_fn: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<RetryOutcome<T>>>,
    {
        const MAX_RETRIES: u8 = 2;
        let mut attempt: u8 = 0;
        loop {
            match attempt_fn().await? {
                RetryOutcome::Done(result) => return result,
                RetryOutcome::Retry {
                    err,
                    retry_after_secs,
                } => {
                    if attempt >= MAX_RETRIES {
                        return Err(err);
                    }
                    attempt += 1;
                    let wait = Self::retry_wait(attempt, retry_after_secs);
                    tracing::warn!("retrying in {wait:?} (attempt {attempt}) for {path}");
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    /// Issue a request and return the deserialized body.
    /// Automatically retries up to 2 times on HTTP 429 (rate-limit) and 5xx
    /// server errors with linear back-off (5s, 10s) + jitter, honoring the
    /// Retry-After header when present. Uses ETag-based conditional GETs for
    /// cacheable GET requests to reduce bandwidth in watch/poll loops.
    pub async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<T> {
        let path = path.to_string();
        let body = body.map(str::to_owned);
        self.with_retries(&path, || {
            let method = method.clone();
            let path = path.clone();
            let body = body.clone();
            async move {
                let url = self.url(&path);
                let mut req = self
                    .inner
                    .request(method.clone(), &url)
                    .header(AUTHORIZATION, self.auth_header_value())
                    .header(ACCEPT, "application/json");
                if method == Method::GET {
                    if let Some(etag) = self.get_etag(&path) {
                        req = req.header(IF_NONE_MATCH, etag);
                    }
                }
                if let Some(b) = body {
                    req = req
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(b);
                }
                let resp = req.send().await.map_err(BitbucketError::Http)?;
                self.update_rate_limit(resp.headers());
                let retry_after = Self::retry_after_secs(resp.headers());
                match self.decode(resp, &path).await {
                    Ok(v) => Ok(RetryOutcome::Done(Ok(v))),
                    Err(e) if Self::is_retryable_error(&e) => Ok(RetryOutcome::Retry {
                        err: e,
                        retry_after_secs: retry_after,
                    }),
                    Err(e) => Ok(RetryOutcome::Done(Err(e))),
                }
            }
        })
        .await
    }

    /// Issue a request expecting no meaningful response body (returns `()` on success).
    /// Only checks the HTTP status code; does not attempt to deserialize the body.
    pub async fn send_empty(&self, method: Method, path: &str, body: Option<&str>) -> Result<()> {
        self.send_no_body(method, path, body).await
    }

    /// Internal method that makes a request, checks the status code, handles errors,
    /// but does not deserialize the response body.
    async fn send_no_body(&self, method: Method, path: &str, body: Option<&str>) -> Result<()> {
        let path = path.to_string();
        let body = body.map(str::to_owned);
        self.with_retries(&path, || {
            let method = method.clone();
            let path = path.clone();
            let body = body.clone();
            async move {
                let url = self.url(&path);
                let mut req = self
                    .inner
                    .request(method.clone(), &url)
                    .header(AUTHORIZATION, self.auth_header_value())
                    .header(ACCEPT, "application/json");
                if method == Method::GET {
                    if let Some(etag) = self.get_etag(&path) {
                        req = req.header(IF_NONE_MATCH, etag);
                    }
                }
                if let Some(b) = body {
                    req = req
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(b);
                }
                let resp = req.send().await.map_err(BitbucketError::Http)?;
                let status = resp.status();
                self.update_rate_limit(resp.headers());
                let retry_after = Self::retry_after_secs(resp.headers());

                if status.is_success() || status == StatusCode::NOT_MODIFIED {
                    return Ok(RetryOutcome::Done(Ok(())));
                }

                let text = resp.text().await.map_err(BitbucketError::Http)?;
                let err = map_error(status, &text, &path);
                if Self::is_retryable_error(&err) {
                    Ok(RetryOutcome::Retry {
                        err,
                        retry_after_secs: retry_after,
                    })
                } else {
                    Ok(RetryOutcome::Done(Err(err)))
                }
            }
        })
        .await
    }

    /// POST a serializable body.
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        let raw = serde_json::to_string(body)?;
        self.send(Method::POST, path, Some(&raw)).await
    }

    /// Fetch a paginated list, automatically following pages when `limit > 100`.
    pub async fn fetch_paginated<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        if limit > 100 {
            self.fetch_all_pages(path, limit).await
        } else {
            let page: Paginated<T> = self.send(Method::GET, path, None).await?;
            Ok(page.values)
        }
    }

    pub async fn fetch_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        let first_page: Paginated<T> = self.send(Method::GET, path, None).await?;
        self.paginate_from(first_page, path, limit).await
    }

    /// Continue pagination from an already-fetched first page (avoids double-fetch).
    pub async fn paginate_from<T: DeserializeOwned>(
        &self,
        first_page: Paginated<T>,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        let mut all = first_page.values;

        if all.len() >= limit || first_page.next.is_none() {
            all.truncate(limit);
            return Ok(all);
        }

        // Determine if this endpoint supports numeric `page=N` pagination by
        // inspecting the `next` URL. If it contains `page=`, we can safely
        // fetch remaining pages in parallel. Otherwise, follow `next` links
        // sequentially (cursor-based pagination — safer for all endpoints).
        let next_url = first_page.next.as_deref().unwrap_or("");
        let supports_numeric_paging = next_url.contains("page=");
        // Parallel `page=N` needs a known total (`size`). Without it, following
        // `next` sequentially is the only safe strategy.
        let size = first_page.size as usize;

        if !supports_numeric_paging || size == 0 {
            let mut next_path = strip_base(next_url, &self.base_url)?;
            loop {
                let page: Paginated<T> = self.send(Method::GET, &next_path, None).await?;

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

        // Numeric paging: use the actual page-1 value count as the effective
        // page size (not the reported `pagelen`, which may be 0 or stale).
        let effective_pagelen = all.len().max(1);
        let total_needed = limit.min(size);

        if total_needed <= all.len() {
            all.truncate(total_needed);
            return Ok(all);
        }

        let num_pages = total_needed.div_ceil(effective_pagelen);

        let mut futures = Vec::new();
        for p in 2..=num_pages {
            let p_path = if path.contains('?') {
                format!("{path}&page={p}")
            } else {
                format!("{path}?page={p}")
            };
            futures
                .push(async move { self.send::<Paginated<T>>(Method::GET, &p_path, None).await });
        }

        let results: Vec<Paginated<T>> = futures::stream::iter(futures)
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
        let path = path.to_string();
        let accept = accept.to_string();
        self.with_retries(&path, || {
            let method = method.clone();
            let path = path.clone();
            let accept = accept.clone();
            async move {
                let url = self.url(&path);
                let mut req = self
                    .inner
                    .request(method.clone(), &url)
                    .header(AUTHORIZATION, self.auth_header_value())
                    .header(ACCEPT, accept.as_str());
                if method == Method::GET {
                    if let Some(etag) = self.get_etag(&path) {
                        req = req.header(IF_NONE_MATCH, etag);
                    }
                }
                let resp = req.send().await.map_err(BitbucketError::Http)?;
                let status = resp.status();
                let headers = resp.headers().clone();
                self.update_rate_limit(&headers);
                let retry_after = Self::retry_after_secs(&headers);

                if status == StatusCode::NOT_MODIFIED {
                    if let Some(cached) = self.get_cached_body(&path) {
                        return Ok(RetryOutcome::Done(Ok(cached)));
                    }
                    return Ok(RetryOutcome::Done(Err(BitbucketError::Other(format!(
                        "HTTP 304 Not Modified with empty cache [{path}]"
                    )))));
                }

                let body = resp.text().await.map_err(BitbucketError::Http)?;
                if status.is_success() {
                    if method == Method::GET {
                        self.store_etag(&path, &headers, &body);
                    }
                    return Ok(RetryOutcome::Done(Ok(body)));
                }
                let err = map_error(status, &body, &path);
                if Self::is_retryable_error(&err) {
                    Ok(RetryOutcome::Retry {
                        err,
                        retry_after_secs: retry_after,
                    })
                } else {
                    Ok(RetryOutcome::Done(Err(err)))
                }
            }
        })
        .await
    }

    async fn decode<T: DeserializeOwned>(&self, resp: reqwest::Response, path: &str) -> Result<T> {
        let status = resp.status();
        let headers = resp.headers().clone();

        if status == StatusCode::NOT_MODIFIED {
            if let Some(cached) = self.get_cached_body(path) {
                return deserialize_body(&cached, path);
            }
            return Err(BitbucketError::Other(format!(
                "HTTP 304 Not Modified with empty cache [{path}]"
            )));
        }

        let text = resp.text().await.map_err(BitbucketError::Http)?;

        if status.is_success() {
            self.store_etag(path, &headers, &text);
            return deserialize_body(&text, path);
        }

        Err(map_error(status, &text, path))
    }
}

enum RetryOutcome<T> {
    Done(Result<T>),
    Retry {
        err: BitbucketError,
        retry_after_secs: Option<u64>,
    },
}

/// Deserialize a JSON body, treating empty success bodies as `null` then `{}`.
fn deserialize_body<T: DeserializeOwned>(text: &str, path: &str) -> Result<T> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return serde_json::from_str("null")
            .or_else(|_| serde_json::from_str("{}"))
            .map_err(|e| {
                tracing::debug!("JSON decode failed for empty body ({path}): {e}");
                BitbucketError::Json(e)
            });
    }
    serde_json::from_str(trimmed).map_err(|e| {
        tracing::debug!("JSON decode failed ({path}): {trimmed:.200}");
        BitbucketError::Json(e)
    })
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
/// `path` is included in the error message for debuggability.
pub fn map_error(status: StatusCode, body: &str, path: &str) -> BitbucketError {
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
                format!(
                    "HTTP 401: Unauthorized. Check your credentials are valid. [{}]",
                    path
                )
            } else {
                format!("HTTP 401 Unauthorized: {full} [{path}]")
            };
            BitbucketError::AuthFailed(msg)
        }
        StatusCode::FORBIDDEN => {
            let msg = if full.is_empty() || full.starts_with("HTTP ") {
                format!(
                    "HTTP 403: Permission denied. Your token may lack the required scopes. [{}]",
                    path
                )
            } else {
                format!("HTTP 403 Forbidden: {full} [{path}]")
            };
            BitbucketError::AuthFailed(msg)
        }
        StatusCode::NOT_FOUND => {
            let msg = if full.is_empty() || full.starts_with("HTTP ") {
                format!(
                    "HTTP 404: Not found. The resource or endpoint does not exist. [{}]",
                    path
                )
            } else {
                format!("HTTP 404 Not Found: {full} [{path}]")
            };
            BitbucketError::NotFound(msg)
        }
        StatusCode::TOO_MANY_REQUESTS => {
            BitbucketError::RateLimit(format!("HTTP {status}: {full} [{path}]"))
        }
        StatusCode::BAD_REQUEST => {
            BitbucketError::BadRequest(format!("HTTP {status}: {full} [{path}]"))
        }
        _ => BitbucketError::Other(format!("HTTP {status}: {full} [{path}]")),
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

/// Simple jitter based on system time nanos to avoid thundering herd.
/// Uses SystemTime nanosecond precision for better entropy than a counter.
fn rand_jitter() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    // Spread across 0-4 seconds
    (nanos.wrapping_mul(6364136223846793005) >> 33) % 5
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
            auth_header: SecretString::from("Basic dTpz".to_string()),
            etag_cache: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            rate_limit_remaining: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX)),
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
        let err = map_error(StatusCode::UNAUTHORIZED, body, "/test/path");
        assert!(matches!(err, BitbucketError::AuthFailed(_)));

        let err = map_error(StatusCode::FORBIDDEN, body, "/test/path");
        assert!(matches!(err, BitbucketError::AuthFailed(_)));
    }

    #[test]
    fn map_error_not_found() {
        let body = r#"{"error":{"message":"repository not found"}}"#;
        let err = map_error(StatusCode::NOT_FOUND, body, "/repositories/ws/slug");
        assert!(matches!(err, BitbucketError::NotFound(_)));
    }

    #[test]
    fn map_error_rate_limit() {
        let body = "rate limit exceeded";
        let err = map_error(StatusCode::TOO_MANY_REQUESTS, body, "/pipelines/");
        assert!(matches!(err, BitbucketError::RateLimit(_)));
    }

    #[test]
    fn map_error_other_status() {
        let body = "internal error";
        let err = map_error(StatusCode::INTERNAL_SERVER_ERROR, body, "/test");
        assert!(matches!(err, BitbucketError::Other(_)));
    }

    #[test]
    fn map_error_includes_scope_table() {
        let body = r#"{"error":{"message":"insufficient permissions","detail":{"required":["repo:write"],"granted":["repo:read"]}}}"#;
        let err = map_error(StatusCode::FORBIDDEN, body, "/repos");
        let msg = format!("{err}");
        assert!(msg.contains("repo:write"));
        assert!(msg.contains("MISSING"));
        assert!(msg.contains("repo:read"));
    }

    #[test]
    fn map_error_falls_back_to_raw_body_when_not_json() {
        let err = map_error(StatusCode::BAD_REQUEST, "not valid json", "/test");
        let msg = format!("{err}");
        assert!(msg.contains("not valid json"));
    }

    #[test]
    fn map_error_includes_path() {
        let err = map_error(
            StatusCode::NOT_FOUND,
            "not found",
            "/repositories/ws/missing",
        );
        let msg = format!("{err}");
        assert!(msg.contains("/repositories/ws/missing"));
    }

    #[test]
    fn map_error_500_is_retryable() {
        let err = map_error(StatusCode::INTERNAL_SERVER_ERROR, "boom", "/pipelines");
        assert!(BitbucketClient::is_retryable_error(&err));
    }

    #[test]
    fn map_error_503_is_retryable() {
        let err = map_error(StatusCode::SERVICE_UNAVAILABLE, "down", "/pipelines");
        assert!(BitbucketClient::is_retryable_error(&err));
    }

    #[test]
    fn map_error_400_is_not_retryable() {
        let err = map_error(StatusCode::BAD_REQUEST, "bad", "/pullrequests");
        assert!(!BitbucketClient::is_retryable_error(&err));
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
