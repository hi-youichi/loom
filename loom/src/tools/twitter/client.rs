//! HTTP client for GetXAPI (getxagent.com) — the upstream Twitter/X API.
//!
//! All read requests use `X-API-Key` header. Write operations additionally
//! require `login_cookies` in the request body and optional proxy.
//!
//! Base URL defaults to `https://api.getxagent.com`, overridable via
//! `TWITTER_API_BASE_URL`.

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::Value;

/// Default API base URL.
const DEFAULT_BASE_URL: &str = "https://api.getxagent.com";

/// Max retries for 429/5xx.
const RETRY_MAX: u32 = 3;
/// Initial backoff (seconds).
const RETRY_INITIAL_SECS: u64 = 1;
/// Max backoff cap (seconds).
const RETRY_MAX_SECS: u64 = 10;
/// HTTP request timeout (seconds).
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Shared HTTP client for GetXAPI requests.
///
/// Holds a `reqwest::Client` (connection-pooled) and the API key.
/// All Twitter tools share one instance via `Arc<TwitterClient>`.
#[derive(Clone)]
pub struct TwitterClient {
    client: Client,
    base_url: String,
    api_key: Arc<str>,
}

impl TwitterClient {
    /// Creates a new client from the given API key.
    ///
    /// Base URL comes from `TWITTER_API_BASE_URL` env var or defaults to
    /// `https://api.getxagent.com`.
    pub fn new(api_key: impl Into<Arc<str>>) -> Self {
        let base_url = std::env::var("TWITTER_API_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(api_key, base_url)
    }

    /// Creates a client with an explicit base URL (useful for tests).
    pub fn with_base_url(api_key: impl Into<Arc<str>>, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        let hv = reqwest::header::HeaderValue::from_str(self.api_key.as_ref())
            .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static(""));
        h.insert(
            reqwest::header::HeaderName::from_static("x-api-key"),
            hv.clone(),
        );
        let bearer = format!("Bearer {}", self.api_key);
        if let Ok(bv) = reqwest::header::HeaderValue::from_str(&bearer) {
            h.insert(reqwest::header::AUTHORIZATION, bv);
        }
        h.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        h
    }

    /// GET request with query params. Retries on 429/5xx.
    pub async fn get(&self, path_and_query: &str) -> Result<Value, TwitterClientError> {
        self.request(reqwest::Method::GET, path_and_query, None).await
    }

    /// POST request with JSON body. Retries on 429/5xx.
    pub async fn post(
        &self,
        path: &str,
        body: Value,
    ) -> Result<Value, TwitterClientError> {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }

    /// DELETE request with JSON body. Retries on 429/5xx.
    pub async fn delete(
        &self,
        path: &str,
        body: Value,
    ) -> Result<Value, TwitterClientError> {
        self.request(reqwest::Method::DELETE, path, Some(body)).await
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, TwitterClientError> {
        let url = self.url(path);
        let mut attempt: u32 = 0;
        loop {
            let mut req = self
                .client
                .request(method.clone(), &url)
                .headers(self.headers());
            if let Some(ref b) = body {
                req = req.json(b);
            }
            let resp = req.send().await.map_err(TwitterClientError::Request)?;
            let status = resp.status();
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| TwitterClientError::Request(e.into()))?;
            let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

            if status.is_success() {
                if let Some((code, msg)) = business_error_from_body(&v) {
                    return Err(TwitterClientError::Api { code, msg });
                }
                return Ok(v);
            }

            let code = status.as_str().to_string();
            let msg = v
                .get("message")
                .or_else(|| v.get("error"))
                .and_then(|e| e.as_str())
                .unwrap_or_else(|| status.canonical_reason().unwrap_or("unknown"))
                .to_string();

            let is_retryable = code == "429" || code.starts_with('5');
            if is_retryable && attempt < RETRY_MAX {
                attempt += 1;
                let secs = (RETRY_INITIAL_SECS << (attempt - 1)).min(RETRY_MAX_SECS);
                tracing::warn!(
                    path = path,
                    code = %code,
                    attempt = attempt,
                    retry_after_secs = secs,
                    "API error, retrying"
                );
                tokio::time::sleep(Duration::from_secs(secs)).await;
                continue;
            }
            return Err(TwitterClientError::Api { code, msg });
        }
    }
}

/// Check for `status: "error"` in a 2xx response body.
fn business_error_from_body(v: &Value) -> Option<(String, String)> {
    let obj = v.as_object()?;
    let status = obj.get("status").and_then(Value::as_str)?;
    if !status.eq_ignore_ascii_case("error") {
        return None;
    }
    let msg = obj
        .get("msg")
        .or_else(|| obj.get("message"))
        .or_else(|| obj.get("error"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Upstream returned status=error")
        .to_string();
    let code = if msg.to_ascii_lowercase().contains("subscription") {
        "403"
    } else {
        "400"
    };
    Some((code.to_string(), msg))
}

/// Errors from the Twitter API client.
#[derive(thiserror::Error, Debug)]
pub enum TwitterClientError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("API error ({code}): {msg}")]
    Api { code: String, msg: String },
}

impl TwitterClientError {
    /// Convenience constructor for API errors with just a message (code defaults to 400).
    #[allow(dead_code)]
    fn api_msg(msg: impl Into<String>) -> Self {
        Self::Api {
            code: "400".to_string(),
            msg: msg.into(),
        }
    }
}
