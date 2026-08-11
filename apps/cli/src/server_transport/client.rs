//! High-level client for loom-server that combines HTTP and SSE.
//!
//! Provides [`LoomServerClient`] which owns an [`HttpTransport`] and
//! [`SessionClient`], and can produce [`SseStream`] instances for
//! real-time event consumption.

use std::fmt;
use std::time::Duration;

use super::error::TransportResult;
use super::http::{HttpTransport, HttpTransportBuilder};
use super::session::{
    AbortResponse, AsyncResponse, PromptRequest, PromptResponse, SessionClient,
    SessionCreateRequest, SessionInfo, SessionPatch,
};
use super::sse::{SseChannelKind, SseStream};

/// High-level client for communicating with a running loom-server instance.
///
/// Combines HTTP session management and SSE event subscription in a single
/// struct with a shared `HttpTransport` underneath.
#[derive(Clone)]
pub struct LoomServerClient {
    http: HttpTransport,
    session: SessionClient,
}

impl fmt::Debug for LoomServerClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoomServerClient")
            .field("base_url", &self.http.base_url())
            .field("has_auth", &self.http.has_auth())
            .finish()
    }
}

impl LoomServerClient {
    /// Start building a client with the given base URL.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let client = LoomServerClient::builder("http://127.0.0.1:3030")
    ///     .with_auth_token("my-token")
    ///     .timeout(Duration::from_secs(60))
    ///     .build();
    /// ```
    pub fn builder(base_url: impl AsRef<str>) -> LoomServerClientBuilder {
        LoomServerClientBuilder {
            http: HttpTransport::builder(base_url),
        }
    }

    /// Create a new session with the server.
    pub async fn create_session(&self, req: &SessionCreateRequest) -> TransportResult<SessionInfo> {
        self.session.create_session(req).await
    }

    /// Get a session by id.
    pub async fn get_session(&self, id: &str) -> TransportResult<SessionInfo> {
        self.session.get_session(id).await
    }

    /// List all active sessions.
    pub async fn list_sessions(&self) -> TransportResult<Vec<SessionInfo>> {
        self.session.list_sessions().await
    }

    /// Update session fields (title, agent, workspaceID, etc.).
    pub async fn patch_session(
        &self,
        id: &str,
        patch: &SessionPatch,
    ) -> TransportResult<SessionInfo> {
        self.session.patch_session(id, patch).await
    }

    /// Delete a session.
    pub async fn delete_session(&self, id: &str) -> TransportResult<()> {
        self.session.delete_session(id).await
    }

    /// Send a synchronous prompt and wait for the full response.
    ///
    /// Use this for short prompts that complete quickly. For long-running
    /// prompts, prefer `prompt_async` + `subscribe` to stream results.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::SessionNotFound`] on 404,
    /// [`TransportError::InvalidSessionState`] on 400, or
    /// [`TransportError::ServerError`] on 500.
    pub async fn prompt(
        &self,
        session_id: &str,
        req: &PromptRequest,
    ) -> TransportResult<PromptResponse> {
        self.session.prompt(session_id, req).await
    }

    /// Send a prompt without waiting (fire-and-forget).
    ///
    /// Callers should subscribe to SSE events to observe results.
    pub async fn prompt_async(
        &self,
        session_id: &str,
        req: &PromptRequest,
    ) -> TransportResult<AsyncResponse> {
        self.session.prompt_async(session_id, req).await
    }

    /// Abort the currently running prompt for a session.
    pub async fn abort(&self, session_id: &str) -> TransportResult<AbortResponse> {
        self.session.abort(session_id).await
    }

    /// v2 API: send a prompt via `POST /api/session/:id/agent`.
    pub async fn agent_prompt(
        &self,
        session_id: &str,
        req: &PromptRequest,
    ) -> TransportResult<PromptResponse> {
        self.session.agent_prompt(session_id, req).await
    }

    /// v2 API: interrupt a session via `POST /api/session/:id/interrupt`.
    pub async fn interrupt(&self, session_id: &str) -> TransportResult<AbortResponse> {
        self.session.interrupt(session_id).await
    }

    /// Subscribe to the global SSE event channel.
    ///
    /// Returns an async `Stream` of [`SseEvent`] values. The stream manages
    /// its own connection lifecycle including reconnection.
    pub fn subscribe(&self, channel: SseChannelKind) -> SseStream {
        SseStream::builder(self.http.clone(), channel).build()
    }

    /// Subscribe with custom SSE stream options.
    pub fn subscribe_with(
        &self,
        channel: SseChannelKind,
        f: impl FnOnce(SseStreamBuilder) -> SseStreamBuilder,
    ) -> SseStream {
        f(SseStream::builder(self.http.clone(), channel)).build()
    }

    /// Returns a reference to the underlying HTTP transport.
    pub fn http(&self) -> &HttpTransport {
        &self.http
    }

    /// Returns a reference to the session client.
    pub fn session(&self) -> &SessionClient {
        &self.session
    }
}

/// Builder for [`LoomServerClient`].
#[derive(Debug)]
pub struct LoomServerClientBuilder {
    http: HttpTransportBuilder,
}

impl LoomServerClientBuilder {
    /// Set the base URL of loom-server.
    pub fn base_url(mut self, url: impl AsRef<str>) -> Self {
        self.http = self.http.base_url(url);
        self
    }

    /// Set the bearer auth token.
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.http = self.http.with_auth_token(token);
        self
    }

    /// Set the per-request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.http = self.http.timeout(timeout);
        self
    }

    /// Set the maximum response body size in bytes.
    pub fn max_body_bytes(mut self, bytes: usize) -> Self {
        self.http = self.http.max_body_bytes(bytes);
        self
    }

    /// Build the [`LoomServerClient`].
    ///
    /// # Errors
    ///
    /// Propagates errors from [`HttpTransportBuilder::build`]:
    /// - Missing base URL
    /// - Invalid URL scheme (only `http://` and `https://` accepted)
    pub fn build(self) -> TransportResult<LoomServerClient> {
        let http = self.http.build()?;
        let session = SessionClient::new(http.clone());
        Ok(LoomServerClient { http, session })
    }
}

/// Re-export the SSE stream builder so callers can configure it fluently.
pub type SseStreamBuilder = super::sse::SseStreamBuilder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_debug() {
        let client = LoomServerClient::builder("http://127.0.0.1:3030")
            .with_auth_token("secret")
            .build()
            .unwrap();

        let debug = format!("{client:?}");
        assert!(debug.contains("127.0.0.1:3030"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn test_builder_chain() {
        let client = LoomServerClient::builder("http://localhost:3030")
            .with_auth_token("tok")
            .timeout(Duration::from_secs(60))
            .max_body_bytes(1024 * 1024)
            .build()
            .unwrap();

        assert!(client.http.has_auth());
        assert_eq!(client.http.base_url(), "http://localhost:3030/");
    }

    #[test]
    fn test_builder_fails_with_bad_scheme() {
        let result = LoomServerClient::builder("ftp://example.com")
            .with_auth_token("tok")
            .build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("http or https"));
    }
}
