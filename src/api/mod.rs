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
use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, ETAG, IF_NONE_MATCH, VARY};
use reqwest::{Client, Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::auth::Credentials;
use crate::error::{BitbucketError, Result};

/// Warn when remaining API quota drops below this threshold.
const RATE_LIMIT_WARN_THRESHOLD: u64 = 50;

/// Upper bound on server-provided `Retry-After` waits (seconds). A
/// misbehaving proxy could otherwise stall the CLI for hours mid-command.
const MAX_RETRY_AFTER_SECS: u64 = 60;

/// Bound traversal even when an endpoint emits endless unique, empty pages.
const MAX_PAGINATION_PAGES: usize = 10_000;

/// Upper bound on the in-process ETag cache. Long-running watch loops can
/// touch many paths; when the cap is hit the cache is dropped wholesale
/// (worst case: one extra full fetch per path).
const MAX_ETAG_CACHE_ENTRIES: usize = 256;

/// Total bytes of cached response bodies we are willing to hold.
const MAX_ETAG_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct CachedResponse {
    etag: String,
    body: std::sync::Arc<str>,
}

#[derive(Default)]
struct EtagCache {
    generation: u64,
    entries: std::collections::HashMap<(String, String), CachedResponse>,
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

impl<T> Default for Paginated<T> {
    fn default() -> Self {
        Self {
            size: 0,
            page: 0,
            pagelen: 0,
            next: None,
            previous: None,
            values: Vec::new(),
        }
    }
}

/// Maximum response body we will buffer into memory (32 MiB).
///
/// Pipeline logs, diffs and `bbr api --paginate` all stream remote-controlled
/// bodies; without a cap a huge (or hostile) response OOM-kills the process.
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Validate an API base URL before credentials are attached to it.
///
/// The `Authorization` header is sent to whatever host this points at, so a
/// non-TLS base is a token-exfiltration path: anything able to set
/// `BITBUCKET_API_BASE` in the environment (a CI job definition, a `.env`
/// loader, a wrapper script, a container env) could redirect the token to a
/// plaintext endpoint. Plain `http` is therefore allowed only for loopback
/// hosts, which is what integration tests and a local proxy use.
fn validate_api_base(base_url: &str) -> Result<()> {
    let trimmed = base_url.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(BitbucketError::Usage(format!(
            "invalid API base URL '{base_url}': expected an https:// URL"
        )));
    };

    // Authority is everything up to the first path/query/fragment separator.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Strip any userinfo (`user:pass@host`) and port.
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(h) = host_port.strip_prefix('[') {
        // IPv6 literal: `[::1]:8080`
        h.split(']').next().unwrap_or_default().to_string()
    } else {
        host_port.split(':').next().unwrap_or_default().to_string()
    };

    match scheme.to_ascii_lowercase().as_str() {
        "https" if !host.is_empty() => Ok(()),
        "https" => Err(BitbucketError::Usage(format!(
            "invalid API base URL '{base_url}': missing host"
        ))),
        "http" if is_loopback_host(&host) => Ok(()),
        "http" => Err(BitbucketError::Usage(format!(
            "refusing to send credentials over plaintext http to '{host}'.\n\
             Use an https:// API base URL."
        ))),
        other => Err(BitbucketError::Usage(format!(
            "unsupported API base scheme '{other}://': expected https://"
        ))),
    }
}

/// Whether `host` refers to the local machine.
fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1" | "0.0.0.0"
    )
}

/// Read a response body into a `String`, refusing to buffer more than
/// [`MAX_RESPONSE_BYTES`].
///
/// `Response::text()` has no size limit, so a multi-gigabyte pipeline log or a
/// hostile endpoint would OOM the process. This checks the declared
/// `Content-Length` up front and then enforces the cap again on every streamed
/// chunk, so a chunked response with no declared length is still bounded.
async fn read_body_capped(resp: reqwest::Response, path: &str) -> Result<String> {
    if let Some(len) = resp.content_length() {
        if len > MAX_RESPONSE_BYTES as u64 {
            return Err(oversized_body_err(len as usize, path));
        }
    }
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BitbucketError::Http)?;
        if buf.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(oversized_body_err(buf.len() + chunk.len(), path));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|e| {
        BitbucketError::Other(format!(
            "response body for [{path}] is not valid UTF-8: {e}"
        ))
    })
}

fn oversized_body_err(size: usize, path: &str) -> BitbucketError {
    BitbucketError::Other(format!(
        "response body for [{path}] is {size} bytes, over the {} MiB cap; \
         narrow the query or lower --limit",
        MAX_RESPONSE_BYTES / (1024 * 1024)
    ))
}

/// Bitbucket Cloud REST API v2 wrapper.
#[derive(Clone)]
pub struct BitbucketClient {
    base_url: String,
    inner: Client,
    creds: Credentials,
    /// `Basic base64(username:token)` — zeroized on drop via `SecretString`.
    auth_header: SecretString,
    /// Conditional GET cache keyed by request path and Accept representation.
    etag_cache: std::sync::Arc<Mutex<EtagCache>>,
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
    /// Preferred entry point: build a client from resolved credentials.
    pub fn from_credentials(base_url: &str, creds: Credentials) -> Result<Self> {
        Self::new(base_url, creds)
    }

    /// Preferred entry point with an explicit request timeout (seconds).
    pub fn from_credentials_with_timeout(
        base_url: &str,
        creds: Credentials,
        timeout_secs: u64,
    ) -> Result<Self> {
        Self::with_timeout(base_url, creds, timeout_secs)
    }

    /// Construct a new client. Uses rustls and a configurable timeout (default 30s).
    /// Auth is always HTTP Basic for Atlassian API tokens.
    pub fn new(base_url: &str, creds: Credentials) -> Result<Self> {
        Self::with_timeout(base_url, creds, 30)
    }

    /// Construct a new client with a specific timeout in seconds.
    pub fn with_timeout(base_url: &str, creds: Credentials, timeout_secs: u64) -> Result<Self> {
        validate_api_base(base_url)?;
        let raw = format!("{}:{}", creds.username, creds.secret.expose_secret());
        let encoded = base64_encode(raw.as_bytes());
        let auth_header = SecretString::from(format!("Basic {encoded}"));
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
            etag_cache: std::sync::Arc::new(Mutex::new(EtagCache::default())),
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
                    crate::log_warn!(
                        "Bitbucket API rate limit low: {n} requests remaining (threshold {RATE_LIMIT_WARN_THRESHOLD})"
                    );
                }
            }
        }
    }

    /// Snapshot the validator and its exact body together. The request retains
    /// the body even if another request replaces/evicts the cache entry.
    fn cached_response(&self, path: &str, accept: &str) -> (u64, Option<CachedResponse>) {
        self.etag_cache.lock().map_or((0, None), |cache| {
            (
                cache.generation,
                cache
                    .entries
                    .get(&(path.to_string(), accept.to_string()))
                    .cloned(),
            )
        })
    }

    fn invalidate_cache(&self) {
        if let Ok(mut cache) = self.etag_cache.lock() {
            cache.generation = cache.generation.wrapping_add(1);
            cache.entries.clear();
        }
    }

    fn store_etag(
        &self,
        path: &str,
        accept: &str,
        generation: u64,
        headers: &reqwest::header::HeaderMap,
        body: &str,
    ) {
        let Ok(mut cache) = self.etag_cache.lock() else {
            return;
        };
        // A GET started before a mutation must not repopulate its old cache.
        if cache.generation != generation {
            return;
        }
        let key = (path.to_string(), accept.to_string());
        cache.entries.remove(&key);
        let prohibited = headers
            .get_all(CACHE_CONTROL)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|directive| {
                directive
                    .trim()
                    .split('=')
                    .next()
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case("no-store"))
            })
            || headers
                .get_all(VARY)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(','))
                // We only model Accept variance; other variants are not cached.
                .any(|name| !name.trim().is_empty() && !name.trim().eq_ignore_ascii_case("accept"));
        let Some(etag) = headers.get(ETAG).and_then(|v| v.to_str().ok()) else {
            return;
        };
        if prohibited || etag.is_empty() || body.len() > MAX_ETAG_CACHE_BYTES {
            return;
        }
        let bytes: usize = cache.entries.values().map(|entry| entry.body.len()).sum();
        if cache.entries.len() >= MAX_ETAG_CACHE_ENTRIES
            || bytes + body.len() > MAX_ETAG_CACHE_BYTES
        {
            cache.entries.clear();
        }
        cache.entries.insert(
            key,
            CachedResponse {
                etag: etag.to_string(),
                body: body.into(),
            },
        );
    }

    /// Compute the retry wait duration from a Retry-After header or backoff+jitter.
    fn retry_wait(attempt: u8, retry_after_secs: Option<u64>) -> std::time::Duration {
        if let Some(ra) = retry_after_secs {
            // Cap server-provided waits: a misbehaving proxy could send an
            // enormous Retry-After and stall the CLI for hours mid-command.
            return std::time::Duration::from_secs(ra.min(MAX_RETRY_AFTER_SECS));
        }
        let base = u64::from(attempt) * 5;
        let jitter = rand_jitter();
        std::time::Duration::from_secs(base + jitter)
    }

    /// Determine whether an error/status is retryable for a given HTTP method.
    ///
    /// 429 is safe to retry for any method — the server rejected the request
    /// before processing it. 5xx is only retried for idempotent methods: a
    /// 5xx response can mean the server *did* process the request before
    /// failing, so replaying a POST (merge, approve, comment, create) could
    /// double-apply the mutation.
    ///
    /// 5xx detection is structural via [`BitbucketError::Server`], which
    /// carries the status code — deliberately not a string match on the
    /// formatted message, which would silently break if error wording
    /// ever changed.
    fn is_retryable_error(err: &BitbucketError, method: &Method) -> bool {
        match err {
            BitbucketError::RateLimit(_) => true,
            BitbucketError::Server { status, .. } => {
                status.is_server_error() && method != Method::POST
            }
            _ => false,
        }
    }

    fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<u64> {
        headers
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, time::OffsetDateTime::now_utc()))
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
                    crate::log_warn!("retrying in {wait:?} (attempt {attempt}) for {path}");
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
                let cache = if method == Method::GET && body.is_none() {
                    Some(self.cached_response(&path, "application/json"))
                } else {
                    None
                };
                if let Some((_, Some(cached))) = &cache {
                    req = req.header(IF_NONE_MATCH, &cached.etag);
                }
                if let Some(b) = body {
                    req = req
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(b);
                }
                let resp = self.send_request(req, &method).await?;
                self.update_rate_limit(resp.headers());
                let retry_after = Self::retry_after_secs(resp.headers());
                match self.decode(resp, &path, cache).await {
                    Ok(v) => Ok(RetryOutcome::Done(Ok(v))),
                    Err(e) if Self::is_retryable_error(&e, &method) => Ok(RetryOutcome::Retry {
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
                let cache = if method == Method::GET && body.is_none() {
                    Some(self.cached_response(&path, "application/json"))
                } else {
                    None
                };
                if let Some((_, Some(cached))) = &cache {
                    req = req.header(IF_NONE_MATCH, &cached.etag);
                }
                if let Some(b) = body {
                    req = req
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(b);
                }
                let resp = self.send_request(req, &method).await?;
                let status = resp.status();
                self.update_rate_limit(resp.headers());
                let retry_after = Self::retry_after_secs(resp.headers());

                if status.is_success()
                    || (status == StatusCode::NOT_MODIFIED && matches!(cache, Some((_, Some(_)))))
                {
                    return Ok(RetryOutcome::Done(Ok(())));
                }

                let text = read_body_capped(resp, &path).await?;
                let err = map_error(status, &text, &path);
                if Self::is_retryable_error(&err, &method) {
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

    /// Fetch up to `limit` values, following `next` even if the server caps page size.
    pub async fn fetch_paginated<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        self.fetch_all_pages(path, limit).await
    }

    pub async fn fetch_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
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
        all.truncate(limit);
        if all.len() >= limit || first_page.next.is_none() {
            return Ok(all);
        }

        // `next` is opaque: even page=N can be a cursor, and size/pagelen may
        // be absent or stale. Do not invent URLs or stop on an empty page.
        let mut seen = std::collections::HashSet::new();
        seen.insert(strip_base(&self.url(path), &self.base_url)?);
        let mut next = first_page.next;
        let mut pages = 1;
        while let Some(next_url) = next {
            if pages >= MAX_PAGINATION_PAGES {
                return Err(BitbucketError::Other(format!(
                    "pagination exceeded the {MAX_PAGINATION_PAGES}-page safety limit; narrow the query or lower --limit"
                )));
            }
            let next_path = strip_base(&next_url, &self.base_url)?;
            if !seen.insert(next_path.clone()) {
                return Err(BitbucketError::Other(
                    "pagination cycle detected; the API repeated a next link".into(),
                ));
            }
            let page: Paginated<T> = self.send(Method::GET, &next_path, None).await?;
            pages += 1;
            all.extend(page.values.into_iter().take(limit - all.len()));
            if all.len() >= limit {
                break;
            }
            next = page.next;
        }
        Ok(all)
    }

    /// Issue a GET request with a `Range: bytes={start}-` header and return
    /// the raw text body. Returns an empty string when the server responds
    /// with 416 (Range Not Satisfiable), meaning we've already fetched all
    /// available content. ETag caching is bypassed — each range request may
    /// return different content at the same path.
    ///
    /// The response status is distinguished internally: a `206 Partial
    /// Content` body is exactly the bytes after `start_byte`; a plain `200`
    /// means the server (or an intermediary) ignored the Range header and
    /// sent the *whole* representation, which callers must not treat as a
    /// suffix. Use [`Self::send_raw_range_checked`] when that distinction
    /// matters.
    pub async fn send_raw_range(&self, path: &str, start_byte: u64) -> Result<String> {
        self.send_raw_range_checked(path, start_byte)
            .await
            .map(|(body, _)| body)
    }

    /// Like [`Self::send_raw_range`] but also reports whether the server
    /// honored the range (`true` = HTTP 206 partial content) or ignored it
    /// and returned the full body (`false` = HTTP 200).
    pub async fn send_raw_range_checked(
        &self,
        path: &str,
        start_byte: u64,
    ) -> Result<(String, bool)> {
        let path = path.to_string();
        let range_hdr = format!("bytes={start_byte}-");
        self.with_retries(&path, || {
            let path = path.clone();
            let range_hdr = range_hdr.clone();
            async move {
                let url = self.url(&path);
                let resp = self
                    .inner
                    .request(Method::GET, &url)
                    .header(AUTHORIZATION, self.auth_header_value())
                    .header(ACCEPT, "*/*")
                    .header("Range", range_hdr.as_str())
                    .send()
                    .await
                    .map_err(BitbucketError::Http)?;
                let status = resp.status();
                let headers = resp.headers().clone();
                self.update_rate_limit(&headers);
                let retry_after = Self::retry_after_secs(&headers);
                if status == StatusCode::RANGE_NOT_SATISFIABLE {
                    return Ok(RetryOutcome::Done(Ok((String::new(), true))));
                }
                let body = read_body_capped(resp, &path).await?;
                if status == StatusCode::PARTIAL_CONTENT {
                    return Ok(RetryOutcome::Done(Ok((body, true))));
                }
                if status.is_success() {
                    // Full-body 200: report `honored = false` so callers know
                    // this is the entire representation, not the requested
                    // byte window.
                    return Ok(RetryOutcome::Done(Ok((body, false))));
                }
                let err = map_error(status, &body, &path);
                if Self::is_retryable_error(&err, &Method::GET) {
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
                let cache = if method == Method::GET {
                    Some(self.cached_response(&path, &accept))
                } else {
                    None
                };
                if let Some((_, Some(cached))) = &cache {
                    req = req.header(IF_NONE_MATCH, &cached.etag);
                }
                let resp = self.send_request(req, &method).await?;
                let status = resp.status();
                let headers = resp.headers().clone();
                self.update_rate_limit(&headers);
                let retry_after = Self::retry_after_secs(&headers);

                if status == StatusCode::NOT_MODIFIED {
                    if let Some((_, Some(cached))) = &cache {
                        return Ok(RetryOutcome::Done(Ok(cached.body.to_string())));
                    }
                    return Ok(RetryOutcome::Done(Err(BitbucketError::Other(format!(
                        "HTTP 304 Not Modified with empty cache [{path}]"
                    )))));
                }

                let body = read_body_capped(resp, &path).await?;
                if status.is_success() {
                    if let Some((generation, _)) = cache {
                        self.store_etag(&path, &accept, generation, &headers, &body);
                    }
                    return Ok(RetryOutcome::Done(Ok(body)));
                }
                let err = map_error(status, &body, &path);
                if Self::is_retryable_error(&err, &method) {
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

    /// Conservatively invalidate around every mutation, even transport failures:
    /// the server may have applied a write before the connection failed.
    async fn send_request(
        &self,
        request: reqwest::RequestBuilder,
        method: &Method,
    ) -> Result<reqwest::Response> {
        let mutation = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
        if mutation {
            self.invalidate_cache();
        }
        let response = request.send().await;
        if mutation {
            self.invalidate_cache();
        }
        response.map_err(BitbucketError::Http)
    }

    async fn decode<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
        path: &str,
        cache: Option<(u64, Option<CachedResponse>)>,
    ) -> Result<T> {
        let status = resp.status();
        let headers = resp.headers().clone();

        if status == StatusCode::NOT_MODIFIED {
            if let Some((_, Some(cached))) = &cache {
                return deserialize_body(&cached.body, path);
            }
            return Err(BitbucketError::Other(format!(
                "HTTP 304 Not Modified with empty cache [{path}]"
            )));
        }

        let text = read_body_capped(resp, path).await?;

        if status.is_success() {
            if let Some((generation, _)) = cache {
                self.store_etag(path, "application/json", generation, &headers, &text);
            }
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
    let diagnostic_path = path.split(['?', '#']).next().unwrap_or(path);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return serde_json::from_str("null")
            .or_else(|_| serde_json::from_str("{}"))
            .map_err(|e| {
                crate::log_debug!("JSON decode failed for empty body ({diagnostic_path})");
                BitbucketError::Json(e)
            });
    }
    serde_json::from_str(trimmed).map_err(|e| {
        crate::log_debug!(
            "JSON decode failed ({diagnostic_path}): {} bytes, line {}, column {}",
            text.len(),
            e.line(),
            e.column()
        );
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
                                all.push((s, "granted"));
                            }
                        }
                    }
                    // Plain ASCII, no theme/unicode glyphs: this text flows
                    // into error messages that also surface in `--json`
                    // output and must stay stable across environments.
                    if !all.is_empty() {
                        if !full.is_empty() {
                            full.push('\n');
                        }
                        let max_w = all.iter().map(|(n, _)| n.len()).max().unwrap_or(0).max(6);
                        full.push_str(&format!("\n  {:<width$}  Status", "Scope", width = max_w));
                        full.push_str(&format!("\n  {}", "-".repeat(max_w + 8)));
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
        s if s.is_server_error() => BitbucketError::Server {
            status: s,
            source: Box::new(BitbucketError::Other(format!("HTTP {s}: {full} [{path}]"))),
        },
        _ => BitbucketError::Other(format!("HTTP {status}: {full} [{path}]")),
    }
}

/// Validate an absolute next URL, then produce the path expected by `url()`.
/// Never include the remote URL in errors: its query/userinfo may hold secrets.
fn strip_base(url: &str, base: &str) -> Result<String> {
    let invalid =
        || BitbucketError::Other("pagination next URL is invalid or outside the API base".into());
    if url.chars().any(|ch| ch.is_control() || ch.is_whitespace()) || url.contains('\\') {
        return Err(invalid());
    }
    let base = reqwest::Url::parse(base).map_err(|_| invalid())?;
    let next = reqwest::Url::parse(url).map_err(|_| invalid())?;
    if base.origin() != next.origin()
        || !next.username().is_empty()
        || next.password().is_some()
        || next.fragment().is_some()
    {
        return Err(invalid());
    }
    let base_path = base.path().trim_end_matches('/');
    let relative = next.path().strip_prefix(base_path).ok_or_else(invalid)?;
    if !relative.starts_with('/') || relative.starts_with("//") {
        return Err(invalid());
    }
    // Use the parsed query unchanged; decoding/re-encoding can alter opaque cursors.
    Ok(match next.query() {
        Some(query) => format!("{relative}?{query}"),
        None => relative.to_string(),
    })
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

/// Parse a `Retry-After` value: delay-seconds or an HTTP-date (RFC 9110
/// IMF-fixdate, e.g. `Wed, 21 Oct 2026 07:28:00 GMT`), relative to `now`.
/// A date in the past means "retry now" (0 seconds).
fn parse_retry_after(value: &str, now: time::OffsetDateTime) -> Option<u64> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    // IMF-fixdate always uses the literal zone "GMT"; RFC 2822 parsing wants a
    // numeric offset, so normalize it first.
    let normalized = value
        .strip_suffix(" GMT")
        .or_else(|| value.strip_suffix(" UTC"))
        .map(|rest| format!("{rest} +0000"))?;
    let at =
        time::OffsetDateTime::parse(&normalized, &time::format_description::well_known::Rfc2822)
            .ok()?;
    let delta = (at - now).whole_seconds();
    Some(u64::try_from(delta).unwrap_or(0))
}

/// Simple jitter based on system time nanos mixed with a per-process counter
/// to avoid thundering herd. Wall-clock nanos alone would be identical for
/// concurrent retries within the same process; the atomic counter decorrelates
/// them.
fn rand_jitter() -> u64 {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Spread across 0-4 seconds
    ((nanos ^ count.wrapping_mul(0x9E3779B97F4A7C15)).wrapping_mul(6364136223846793005) >> 33) % 5
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
    fn retry_after_accepts_seconds_and_http_dates() {
        let now = time::OffsetDateTime::parse(
            "Wed, 21 Oct 2026 07:28:00 +0000",
            &time::format_description::well_known::Rfc2822,
        )
        .unwrap();
        assert_eq!(parse_retry_after("120", now), Some(120));
        assert_eq!(parse_retry_after(" 7 ", now), Some(7));
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2026 07:28:45 GMT", now),
            Some(45)
        );
        // A date in the past means "now", not a parse failure.
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2026 07:00:00 GMT", now),
            Some(0)
        );
        assert_eq!(parse_retry_after("soon", now), None);
        assert_eq!(parse_retry_after("", now), None);
        // The wait is still capped by `retry_wait`.
        assert_eq!(
            BitbucketClient::retry_wait(1, parse_retry_after("Wed, 21 Oct 2026 09:00:00 GMT", now)),
            std::time::Duration::from_secs(MAX_RETRY_AFTER_SECS)
        );
    }

    fn cache_test_client() -> BitbucketClient {
        BitbucketClient::new(
            "https://api.bitbucket.org/2.0",
            Credentials {
                username: "test".into(),
                secret: "fake-token".into(),
            },
        )
        .unwrap()
    }

    fn cache_test_headers() -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(ETAG, "\"cached\"".parse().unwrap());
        headers
    }

    #[test]
    fn cache_budget_applies_when_replacing_an_existing_entry() {
        let c = cache_test_client();
        let headers = cache_test_headers();
        let medium = "x".repeat(3 * 1024 * 1024);
        c.store_etag("/a", "text/plain", 0, &headers, &medium);
        c.store_etag("/b", "text/plain", 0, &headers, &medium);
        c.store_etag(
            "/a",
            "text/plain",
            0,
            &headers,
            &"x".repeat(6 * 1024 * 1024),
        );
        let cache = c.etag_cache.lock().unwrap();
        assert!(
            cache
                .entries
                .values()
                .map(|entry| entry.body.len())
                .sum::<usize>()
                <= MAX_ETAG_CACHE_BYTES
        );
    }

    #[test]
    fn in_flight_snapshot_survives_eviction_with_its_exact_body() {
        let c = cache_test_client();
        let headers = cache_test_headers();
        c.store_etag("/a", "text/plain", 0, &headers, "original");
        let (_, snapshot) = c.cached_response("/a", "text/plain");
        for n in 0..MAX_ETAG_CACHE_ENTRIES {
            c.store_etag(&format!("/{n}"), "text/plain", 0, &headers, "other");
        }
        assert!(c.cached_response("/a", "text/plain").1.is_none());
        assert_eq!(&*snapshot.unwrap().body, "original");
    }

    #[test]
    fn pre_mutation_get_cannot_repopulate_invalidated_cache() {
        let c = cache_test_client();
        let (generation, _) = c.cached_response("/a", "text/plain");
        c.clone().invalidate_cache();
        c.store_etag(
            "/a",
            "text/plain",
            generation,
            &cache_test_headers(),
            "stale",
        );
        assert!(c.cached_response("/a", "text/plain").1.is_none());
        let (current, _) = c.cached_response("/a", "text/plain");
        c.store_etag("/a", "text/plain", current, &cache_test_headers(), "fresh");
        assert_eq!(
            &*c.cached_response("/a", "text/plain").1.unwrap().body,
            "fresh"
        );
    }

    #[test]
    fn oversized_replacement_removes_the_previous_validator() {
        let c = cache_test_client();
        let headers = cache_test_headers();
        c.store_etag("/a", "text/plain", 0, &headers, "old");
        c.store_etag(
            "/a",
            "text/plain",
            0,
            &headers,
            &"x".repeat(MAX_ETAG_CACHE_BYTES + 1),
        );
        assert!(c.cached_response("/a", "text/plain").1.is_none());
    }

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
            },
            auth_header: SecretString::from("Basic dTpz".to_string()),
            etag_cache: std::sync::Arc::new(Mutex::new(EtagCache::default())),
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
        let body = "teapot";
        // 418 is neither 4xx-special-cased nor 5xx, so it lands in Other.
        let err = map_error(StatusCode::IM_A_TEAPOT, body, "/test");
        assert!(matches!(err, BitbucketError::Other(_)));
    }

    #[test]
    fn map_error_5xx_becomes_structured_server_variant() {
        for status in [500, 502, 503, 504] {
            let err = map_error(StatusCode::from_u16(status).unwrap(), "boom", "/pipelines");
            match &err {
                BitbucketError::Server { status: s, .. } => {
                    assert_eq!(s.as_u16(), status);
                }
                other => panic!("expected Server variant for {status}, got {other:?}"),
            }
        }
    }

    #[test]
    fn map_error_includes_scope_table() {
        let body = r#"{"error":{"message":"insufficient permissions","detail":{"required":["repo:write"],"granted":["repo:read"]}}}"#;
        let err = map_error(StatusCode::FORBIDDEN, body, "/repos");
        let msg = format!("{err}");
        assert!(msg.contains("repo:write"));
        assert!(msg.contains("MISSING"));
        assert!(msg.contains("repo:read"));
        // Scope table is plain ASCII (stable across themes / --json output).
        assert!(
            msg.contains("granted"),
            "granted mark is the word, not a glyph"
        );
        assert!(msg.contains("Status"));
        assert!(!msg.contains('\u{2713}'), "no theme glyphs in error text");
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
    fn map_error_500_is_retryable_for_get() {
        let err = map_error(StatusCode::INTERNAL_SERVER_ERROR, "boom", "/pipelines");
        assert!(BitbucketClient::is_retryable_error(&err, &Method::GET));
    }

    #[test]
    fn map_error_503_is_retryable_for_get() {
        let err = map_error(StatusCode::SERVICE_UNAVAILABLE, "down", "/pipelines");
        assert!(BitbucketClient::is_retryable_error(&err, &Method::GET));
    }

    #[test]
    fn map_error_5xx_is_not_retryable_for_post() {
        // A 5xx response to a POST may mean the mutation was applied before
        // the server failed — replaying it could double-apply (double merge,
        // duplicate comment). Only 429 is safe to retry for POSTs.
        let err = map_error(StatusCode::INTERNAL_SERVER_ERROR, "boom", "/merge");
        assert!(!BitbucketClient::is_retryable_error(&err, &Method::POST));
        let err = map_error(StatusCode::SERVICE_UNAVAILABLE, "down", "/merge");
        assert!(!BitbucketClient::is_retryable_error(&err, &Method::POST));
    }

    #[test]
    fn map_error_429_is_retryable_for_post() {
        // 429 rejects the request before processing — always safe to retry.
        let err = map_error(StatusCode::TOO_MANY_REQUESTS, "slow down", "/merge");
        assert!(BitbucketClient::is_retryable_error(&err, &Method::POST));
    }

    #[test]
    fn map_error_400_is_not_retryable() {
        let err = map_error(StatusCode::BAD_REQUEST, "bad", "/pullrequests");
        assert!(!BitbucketClient::is_retryable_error(&err, &Method::GET));
    }

    #[test]
    fn retry_after_is_capped() {
        // A misbehaving proxy sending Retry-After: 86400 must not stall the
        // CLI for a day.
        let wait = BitbucketClient::retry_wait(1, Some(86400));
        assert_eq!(wait, std::time::Duration::from_secs(60));
        // Small values pass through unchanged.
        let wait = BitbucketClient::retry_wait(1, Some(3));
        assert_eq!(wait, std::time::Duration::from_secs(3));
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
