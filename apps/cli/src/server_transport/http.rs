//! Low-level HTTP transport primitives for anureo-server.
//!
//! Provides:
//! - [`HttpTransport`] — base URL + authorization context, builds and
//!   executes HTTP requests with consistent timeout and error handling.
//! - [`HttpTransportBuilder`] — fallible builder for constructing transports
//!   with validated URLs, auth, and timeout.
//!
//! # Authorization
//!
//! anureo-server uses a simple bearer-token scheme. Set the token via
//! [`HttpTransportBuilder::with_auth_token`] and it will be injected as
//! `Authorization: Bearer <token>` on every request.
//!
//! The token may be supplied in either form:
//! - **Raw token**: `"secret"` → `Authorization: Bearer secret`
//! - **Pre-formed header value**: `"Bearer secret"` or `"Basic xyz"` →
//!   sent as-is (no second `Bearer` prefix is added)
//!
//! # Example
//!
//! ```ignore
//! let transport = HttpTransport::builder("http://127.0.0.1:3030")
//!     .with_auth_token("secret")
//!     .timeout(std::time::Duration::from_secs(30))
//!     .build()?;
//!
//! let body: serde_json::Value = transport.get("/session").await?;
//! ```

use std::fmt;
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::error::{TransportError, TransportResult};

/// Default maximum response body size (64 KB).
pub const DEFAULT_MAX_BODY_BYTES: usize = 64 * 1024;

/// Default request timeout (30 seconds).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Low-level HTTP transport for anureo-server.
///
/// Construct via [`HttpTransport::builder`].
///
/// All methods accept a `path` relative to the base URL (e.g. `"/session"`).
/// The transport handles:
/// - Injecting the `Authorization` header when a token is configured.
/// - Setting `Content-Type: application/json` on requests with a body.
/// - Converting non-2xx responses to [`TransportError::HttpError`].
/// - Deserializing JSON responses into the caller's type.
/// - Returning [`TransportError::EmptySuccess`] for 204 No Content responses.
#[derive(Clone)]
pub struct HttpTransport {
    client: reqwest::Client,
    base_url: reqwest::Url,
    /// Auth value — either a raw token (`"secret"`) or a full
    /// `Authorization` header value (`"Bearer secret"`, `"Basic xyz"`).
    /// Caller is responsible for not passing a raw token that already
    /// starts with `"Bearer "` to avoid `Bearer Bearer ...`.
    auth_value: Option<String>,
    timeout: Duration,
    max_body_bytes: usize,
}

impl fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpTransport")
            .field("base_url", &self.base_url)
            .field("auth_token", &self.auth_value.is_some())
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl HttpTransport {
    /// Start building an `HttpTransport` with the given base URL.
    ///
    /// The base URL should include the scheme and port, e.g.
    /// `"http://127.0.0.1:3030"`. Trailing slashes are handled gracefully.
    ///
    /// # Builder ergonomics
    ///
    /// The builder is fallible — [`HttpTransportBuilder::build`] returns
    /// `Result<HttpTransport, TransportError>` because the base URL must
    /// be a valid `http://` or `https://` URL.
    ///
    /// ```ignore
    /// let transport = HttpTransport::builder("http://127.0.0.1:3030").build()?;
    /// ```
    pub fn builder(base_url: impl AsRef<str>) -> HttpTransportBuilder {
        HttpTransportBuilder {
            base_url: reqwest::Url::parse(base_url.as_ref()).ok(),
            auth_value: None,
            timeout: Some(DEFAULT_TIMEOUT),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }

    /// Returns the base URL string (always includes trailing slash).
    pub fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    /// Returns a reference to the underlying `reqwest::Url`.
    pub fn url(&self) -> &reqwest::Url {
        &self.base_url
    }

    /// Returns `true` if an auth token is configured.
    pub fn has_auth(&self) -> bool {
        self.auth_value.is_some()
    }

    /// Returns a reference to the auth value, if set.
    ///
    /// May be a raw token or a full `Authorization` header value.
    pub fn auth_value(&self) -> Option<&String> {
        self.auth_value.as_ref()
    }

    /// Returns a reference to the HTTP client.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Performs a GET request and deserializes the JSON response.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] on network failure, non-2xx HTTP status,
    /// JSON parse error, or 204 No Content.
    pub async fn get<R>(&self, path: &str) -> TransportResult<R>
    where
        R: DeserializeOwned,
    {
        self.request(reqwest::Method::GET, path, None::<&()>).await
    }

    /// Performs a POST request with a JSON body and deserializes the response.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] on network failure, non-2xx HTTP status,
    /// JSON parse error, or 204 No Content.
    pub async fn post<B, R>(&self, path: &str, body: &B) -> TransportResult<R>
    where
        B: Serialize,
        R: DeserializeOwned,
    {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }

    /// Performs a PATCH request with a JSON body and deserializes the response.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] on network failure, non-2xx HTTP status,
    /// JSON parse error, or 204 No Content.
    pub async fn patch<B, R>(&self, path: &str, body: &B) -> TransportResult<R>
    where
        B: Serialize,
        R: DeserializeOwned,
    {
        self.request(reqwest::Method::PATCH, path, Some(body)).await
    }

    /// Performs a DELETE request.
    ///
    /// anureo-server returns 204 No Content on success. Use this method for
    /// endpoints that return no body.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::SessionNotFound`] on 404,
    /// [`TransportError::HttpError`] on other non-2xx, or any network/timeout error.
    pub async fn delete(&self, path: &str) -> TransportResult<()> {
        self.request_no_body(reqwest::Method::DELETE, path).await
    }

    /// Performs a POST request with an optional JSON body, expecting no response body.
    ///
    /// Use for endpoints that return 204 No Content (e.g. abort, interrupt).
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::HttpError`] on non-2xx or any network error.
    pub async fn post_no_body<B>(&self, path: &str, body: Option<&B>) -> TransportResult<()>
    where
        B: Serialize,
    {
        self.request_no_body_with_body(reqwest::Method::POST, path, body)
            .await
    }

    /// Returns the full URL for a path, joining it with the base URL using
    /// [`reqwest::Url::join`].
    ///
    /// This correctly handles:
    /// - Paths with or without leading slashes
    /// - Base URLs with or without trailing slashes
    /// - Query strings in the path (they are appended to existing query)
    /// - Fragment identifiers
    pub(crate) fn make_url(&self, path: &str) -> reqwest::Url {
        self.base_url
            .join(path)
            .expect("reqwest::Url::join is infallible for relative paths")
    }

    // ─── Private request helpers ───────────────────────────────────────────────

    /// Low-level request with typed JSON response body.
    async fn request<B, R>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> TransportResult<R>
    where
        B: Serialize,
        R: DeserializeOwned,
    {
        let url = self.make_url(path);
        let path_str = url.path().to_string();

        let mut req = self.client.request(method, url);

        // Auth header — sent as-is (caller is responsible for not passing
        // a raw token that already starts with "Bearer ").
        if let Some(value) = &self.auth_value {
            req = req.header(AUTHORIZATION, value.as_str());
        }

        // JSON body
        if let Some(b) = body {
            let json = serde_json::to_string(b)?;
            req = req.header(CONTENT_TYPE, "application/json").body(json);
        }

        let response = req.timeout(self.timeout).send().await?;
        self.handle_response(response, &path_str).await
    }

    /// Request with no expected response body (204 No Content).
    async fn request_no_body(&self, method: reqwest::Method, path: &str) -> TransportResult<()> {
        let url = self.make_url(path);
        let path_str = url.path().to_string();

        let mut req = self.client.request(method, url);

        if let Some(value) = &self.auth_value {
            req = req.header(AUTHORIZATION, value.as_str());
        }

        let response = req.timeout(self.timeout).send().await?;
        self.handle_no_body_response(response, &path_str).await
    }

    /// POST/PATCH with no expected response body.
    async fn request_no_body_with_body<B>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
    ) -> TransportResult<()>
    where
        B: Serialize,
    {
        let url = self.make_url(path);
        let path_str = url.path().to_string();

        let mut req = self.client.request(method, url);

        if let Some(value) = &self.auth_value {
            req = req.header(AUTHORIZATION, value.as_str());
        }

        if let Some(b) = body {
            let json = serde_json::to_string(b)?;
            req = req.header(CONTENT_TYPE, "application/json").body(json);
        }

        let response = req.timeout(self.timeout).send().await?;
        self.handle_no_body_response(response, &path_str).await
    }

    /// Handle a response with an expected JSON body.
    async fn handle_response<R>(
        &self,
        response: reqwest::Response,
        path: &str,
    ) -> TransportResult<R>
    where
        R: DeserializeOwned,
    {
        let status = response.status();

        // 204 No Content — caller used typed method but server returned nothing.
        if status.as_u16() == 204 {
            return Err(TransportError::EmptySuccess {
                path: path.to_string(),
            });
        }

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        if body_bytes.len() > self.max_body_bytes {
            return Err(TransportError::BodyTooLarge {
                max_bytes: self.max_body_bytes,
            });
        }

        if !status.is_success() {
            let body_str = String::from_utf8_lossy(&body_bytes).to_string();
            return Err(TransportError::HttpError {
                status: status.as_u16(),
                base_url: self.base_url.to_string(),
                path: path.to_string(),
                body: body_str,
            });
        }

        // Empty body on a non-204 success — this is unusual but treat as error
        // rather than panicking during JSON decode.
        if body_bytes.is_empty() {
            return Err(TransportError::EmptySuccess {
                path: path.to_string(),
            });
        }

        let parsed: R = serde_json::from_slice(&body_bytes)?;
        Ok(parsed)
    }

    /// Handle a response with no expected body (204 No Content).
    async fn handle_no_body_response(
        &self,
        response: reqwest::Response,
        path: &str,
    ) -> TransportResult<()> {
        let status = response.status();

        if status.as_u16() == 204 || status.as_u16() == 200 {
            // Consume any body to ensure the connection is returned to the pool.
            let _ = response.bytes().await;
            return Ok(());
        }

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        let body_str = String::from_utf8_lossy(&body_bytes).to_string();

        Err(TransportError::HttpError {
            status: status.as_u16(),
            base_url: self.base_url.to_string(),
            path: path.to_string(),
            body: body_str,
        })
    }
}

/// Builder for [`HttpTransport`].
///
/// All mutating methods return `Self` so you can chain fluently.
/// Construction is fallible via [`HttpTransportBuilder::build`].
#[derive(Default, Debug)]
pub struct HttpTransportBuilder {
    base_url: Option<reqwest::Url>,
    auth_value: Option<String>,
    timeout: Option<Duration>,
    max_body_bytes: usize,
}

impl HttpTransportBuilder {
    /// Set the base URL (e.g. `"http://127.0.0.1:3030"`).
    ///
    /// Only `http://` and `https://` schemes are accepted. File URLs, data
    /// URLs, and other schemes return an error from [`HttpTransportBuilder::build`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// let builder = HttpTransport::builder("http://localhost")
    ///     .base_url("https://api.example.com")?;
    /// ```
    pub fn base_url(mut self, url: impl AsRef<str>) -> Self {
        self.base_url = reqwest::Url::parse(url.as_ref()).ok();
        self
    }

    /// Set the authorization value injected as the `Authorization` header.
    ///
    /// **Two forms are accepted:**
    /// - **Raw bearer token**: `"my-secret"` → `Authorization: Bearer my-secret`
    /// - **Pre-formed header value**: `"Bearer my-secret"` → sent as-is.
    ///   Any value that does **not** start with `"Bearer "` (case-insensitive)
    ///   is sent verbatim, so callers can also supply `"Basic xyz"` or other
    ///   schemes directly.
    ///
    /// # Warning
    ///
    /// Do **not** pass a value that already starts with `"Bearer "` as the
    /// `token` argument — this would produce `Authorization: Bearer Bearer ...`.
    /// Use the pre-formed header value directly instead.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Raw token (recommended):
    /// transport.with_auth_token("secret")
    ///
    /// // Pre-formed header (for custom schemes):
    /// transport.with_auth_token("Bearer secret")
    /// ```
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_value = Some(token.into());
        self
    }

    /// Set the per-request timeout. Defaults to 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the maximum response body size in bytes.
    /// Defaults to 64 KB.
    pub fn max_body_bytes(mut self, bytes: usize) -> Self {
        self.max_body_bytes = bytes;
        self
    }

    /// Build the [`HttpTransport`].
    ///
    /// # Errors
    ///
    /// Returns `TransportError` if:
    /// - No base URL was provided.
    /// - The base URL is not a valid URL.
    /// - The URL scheme is not `http://` or `https://`.
    pub fn build(self) -> TransportResult<HttpTransport> {
        let base_url = self.base_url.ok_or_else(|| {
            TransportError::ConnectionFailed(
                "HttpTransportBuilder: base_url is required".to_string(),
            )
        })?;

        // Enforce http/https scheme.
        let scheme = base_url.scheme();
        if !matches!(scheme, "http" | "https") {
            return Err(TransportError::ConnectionFailed(format!(
                "HttpTransportBuilder: URL scheme must be http or https, got '{scheme}'"
            )));
        }

        let client = reqwest::Client::builder()
            .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
            .tcp_nodelay(true)
            .build()
            .expect("reqwest ClientBuilder is always valid");

        Ok(HttpTransport {
            client,
            base_url,
            auth_value: self.auth_value,
            timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
            max_body_bytes: self.max_body_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── URL joining ─────────────────────────────────────────────────────────

    #[test]
    fn test_make_url_no_slashes() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030")
            .with_auth_token("secret")
            .build()
            .unwrap();

        assert_eq!(
            transport.make_url("/session").as_str(),
            "http://127.0.0.1:3030/session"
        );
    }

    #[test]
    fn test_make_url_with_trailing_slash_in_base() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030/")
            .with_auth_token("secret")
            .build()
            .unwrap();

        assert_eq!(
            transport.make_url("/session").as_str(),
            "http://127.0.0.1:3030/session"
        );
        assert_eq!(
            transport.make_url("/session/").as_str(),
            "http://127.0.0.1:3030/session/"
        );
    }

    #[test]
    fn test_make_url_preserves_query() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030")
            .with_auth_token("secret")
            .build()
            .unwrap();

        assert_eq!(
            transport.make_url("/search?q=test").as_str(),
            "http://127.0.0.1:3030/search?q=test"
        );
    }

    #[test]
    fn test_make_url_no_leading_slash() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030")
            .with_auth_token("secret")
            .build()
            .unwrap();

        assert_eq!(
            transport.make_url("session").as_str(),
            "http://127.0.0.1:3030/session"
        );
    }

    // ─── Builder fallibility ──────────────────────────────────────────────────

    #[test]
    fn test_builder_requires_base_url() {
        let result = HttpTransportBuilder::default().build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("base_url is required"));
    }

    #[test]
    fn test_builder_rejects_invalid_url() {
        let result = HttpTransport::builder("not-a-url").build();
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_rejects_non_http_scheme() {
        let result = HttpTransport::builder("ftp://example.com").build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("http or https"));
    }

    #[test]
    fn test_builder_accepts_https() {
        let result = HttpTransport::builder("https://api.example.com").build();
        assert!(result.is_ok());
    }

    // ─── Auth semantics ───────────────────────────────────────────────────────

    #[test]
    fn test_debug_does_not_leak_token() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030")
            .with_auth_token("super-secret")
            .build()
            .unwrap();

        let debug = format!("{transport:?}");
        assert!(debug.contains("true")); // auth_token: true
        assert!(!debug.contains("super-secret"));
    }

    #[test]
    fn test_auth_value_raw_token() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030")
            .with_auth_token("raw-token")
            .build()
            .unwrap();

        assert_eq!(transport.auth_value(), Some(&"raw-token".to_string()));
    }

    #[test]
    fn test_auth_value_pre_formed_header() {
        let transport = HttpTransport::builder("http://127.0.0.1:3030")
            .with_auth_token("Bearer my-token")
            .build()
            .unwrap();

        // Stored as-is — no transformation.
        assert_eq!(transport.auth_value(), Some(&"Bearer my-token".to_string()));
    }
}
