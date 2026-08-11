//! Session CRUD and run-control calls for loom-server.
//!
//! Wraps the HTTP session endpoints:
//!
//! - [`SessionClient::create_session`] — `POST /session`
//! - [`SessionClient::get_session`] — `GET /session/:id`
//! - [`SessionClient::list_sessions`] — `GET /session`
//! - [`SessionClient::delete_session`] — `DELETE /session/:id`
//! - [`SessionClient::prompt`] — `POST /session/:id/prompt`
//! - [`SessionClient::prompt_async`] — `POST /session/:id/prompt_async`
//! - [`SessionClient::abort`] — `POST /session/:id/abort`
//! - [`SessionClient::patch_session`] — `PATCH /session/:id`

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{TransportError, TransportResult};
use super::HttpTransport;

/// The session metadata shape returned by loom-server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub slug: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub directory: String,
    pub title: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<SessionPath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<SessionTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<ShareInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<SessionModel>,
    pub time: SessionTime,
    #[serde(default)]
    pub metadata: Value,
    #[serde(flatten)]
    pub extras: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPath {
    pub cwd: String,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub additions: i64,
    pub deletions: i64,
    pub files: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTokens {
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache: TokensCache,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokensCache {
    pub read: i64,
    pub write: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareInfo {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionModel {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTime {
    pub created: i64,
    pub updated: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacting: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<i64>,
}

/// Request body for `POST /session` — create a new session.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "parentID", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

/// Request body for `POST /session/:id/prompt`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    pub parts: Vec<PartInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_ref: Option<ModelRef>,
}

impl PromptRequest {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![PartInput {
                part_type: "text".to_string(),
                text: Some(text.into()),
                content: None,
                id: None,
            }],
            agent: None,
            model: None,
            model_ref: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartInput {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

/// Response from a prompt call.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    pub info: AssistantMessageInfo,
    pub parts: Vec<Value>,
    /// Present when the server returned HTTP 500. Contains the structured
    /// error reason (e.g. provider failure, model not found).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<PromptResponseError>,
}

/// The error object embedded in a 500 prompt response body.
#[derive(Debug, Clone, Deserialize)]
pub struct PromptResponseError {
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub role: String,
    pub agent: String,
    pub time: TimeValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Value>,
    #[serde(flatten)]
    pub extras: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TimeValue {
    Timestamp(i64),
    Object(TimeObject),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimeObject {
    pub created: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortResponse {
    pub ok: bool,
    #[serde(default)]
    pub cancelled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AsyncResponse {
    pub ok: bool,
}

/// Session CRUD + run-control client backed by [`HttpTransport`].
#[derive(Clone)]
pub struct SessionClient {
    http: HttpTransport,
}

impl SessionClient {
    pub fn new(http: HttpTransport) -> Self {
        Self { http }
    }

    pub async fn create_session(&self, req: &SessionCreateRequest) -> TransportResult<SessionInfo> {
        self.http.post("/session", req).await
    }

    pub async fn get_session(&self, id: &str) -> TransportResult<SessionInfo> {
        self.http
            .get(&format!("/session/{id}"))
            .await
            .map_err(|e| self.map_not_found(id, e))
    }

    pub async fn list_sessions(&self) -> TransportResult<Vec<SessionInfo>> {
        self.http.get("/session").await
    }

    pub async fn patch_session(
        &self,
        id: &str,
        patch: &SessionPatch,
    ) -> TransportResult<SessionInfo> {
        self.http
            .patch(&format!("/session/{id}"), patch)
            .await
            .map_err(|e| self.map_not_found(id, e))
    }

    pub async fn delete_session(&self, id: &str) -> TransportResult<()> {
        self.http
            .delete(&format!("/session/{id}"))
            .await
            .map_err(|e| self.map_not_found(id, e))
    }

    pub async fn prompt(
        &self,
        session_id: &str,
        req: &PromptRequest,
    ) -> TransportResult<PromptResponse> {
        self.http
            .post(&format!("/session/{session_id}/prompt"), req)
            .await
            .map_err(|e| self.map_session_error(session_id, e))
    }

    pub async fn prompt_async(
        &self,
        session_id: &str,
        req: &PromptRequest,
    ) -> TransportResult<AsyncResponse> {
        self.http
            .post(&format!("/session/{session_id}/prompt_async"), req)
            .await
    }

    pub async fn abort(&self, session_id: &str) -> TransportResult<AbortResponse> {
        self.http
            .post(
                &format!("/session/{session_id}/abort"),
                &serde_json::json!({}),
            )
            .await
    }

    /// v2 alias: `POST /api/session/:id/agent`.
    pub async fn agent_prompt(
        &self,
        session_id: &str,
        req: &PromptRequest,
    ) -> TransportResult<PromptResponse> {
        self.http
            .post(&format!("/api/session/{session_id}/agent"), req)
            .await
            .map_err(|e| self.map_session_error(session_id, e))
    }

    /// v2 alias: `POST /api/session/:id/interrupt`.
    pub async fn interrupt(&self, session_id: &str) -> TransportResult<AbortResponse> {
        self.http
            .post(
                &format!("/api/session/{session_id}/interrupt"),
                &serde_json::json!({}),
            )
            .await
    }

    fn map_not_found(&self, id: &str, err: TransportError) -> TransportError {
        if matches!(err, TransportError::HttpError { status: 404, .. }) {
            return TransportError::SessionNotFound(id.to_string());
        }
        err
    }

    fn map_session_error(&self, session_id: &str, err: TransportError) -> TransportError {
        match &err {
            TransportError::HttpError { status: 404, .. } => {
                TransportError::SessionNotFound(session_id.to_string())
            }
            TransportError::HttpError {
                status: 400, body, ..
            } => {
                if let Ok(v) = serde_json::from_str::<Value>(body) {
                    if let Some(msg) = v.get("error").and_then(|e| e.as_str()) {
                        return TransportError::InvalidSessionState {
                            reason: msg.to_string(),
                        };
                    }
                }
                TransportError::InvalidSessionState {
                    reason: format!("HTTP 400: {}", body),
                }
            }
            TransportError::HttpError {
                status: 500, body, ..
            } => {
                // Try to extract the structured {"error":{"message":"..."}} from the body.
                // Fall back to the raw body if it doesn't parse.
                let message = if let Ok(v) = serde_json::from_str::<Value>(body) {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| body.clone())
                } else {
                    body.clone()
                };
                TransportError::ServerError { message }
            }
            _ => err,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(rename = "workspaceID", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_request_text() {
        let req = PromptRequest::text("Hello, world!");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("Hello, world!"));
    }

    #[test]
    fn test_session_info_deserialize() {
        let json = r#"{
          "id": "sess_abc123",
          "slug": "sess_abc123",
          "projectID": "proj_1",
          "directory": "/tmp",
          "title": "Test",
          "version": "1.0",
          "time": {"created": 1000, "updated": 2000}
        }"#;
        let session: SessionInfo = serde_json::from_str(json).unwrap();
        assert_eq!(session.id, "sess_abc123");
        assert_eq!(session.title, "Test");
    }

    #[test]
    fn test_time_value_timestamp() {
        let json = r#"1000"#;
        let tv: TimeValue = serde_json::from_str(json).unwrap();
        assert!(matches!(tv, TimeValue::Timestamp(1000)));
    }

    #[test]
    fn test_time_value_object() {
        let json = r#"{"created": 1000, "completed": 2000}"#;
        let tv: TimeValue = serde_json::from_str(json).unwrap();
        assert!(matches!(
            tv,
            TimeValue::Object(TimeObject {
                created: 1000,
                completed: Some(2000)
            })
        ));
    }

    // ─── PromptResponse error field ─────────────────────────────────────────────

    #[test]
    fn test_prompt_response_with_error() {
        let json = r#"{
          "info": {
            "id": "msg_1",
            "sessionID": "sess_1",
            "role": "assistant",
            "agent": "build",
            "time": 1000,
            "finish": "error"
          },
          "parts": [],
          "error": { "message": "model not found: gpt-fake" }
        }"#;
        let resp: PromptResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.info.finish.as_deref(), Some("error"));
        let err = resp.error.expect("error field should be present");
        assert!(err.message.contains("model not found"));
    }

    #[test]
    fn test_prompt_response_without_error() {
        let json = r#"{
          "info": {
            "id": "msg_1",
            "sessionID": "sess_1",
            "role": "assistant",
            "agent": "build",
            "time": 1000,
            "finish": "stop"
          },
          "parts": []
        }"#;
        let resp: PromptResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_none());
    }

    // ─── Structured 500 error extraction ─────────────────────────────────────

    #[test]
    fn test_map_session_error_extracts_structured_message() {
        let client = SessionClient::new(
            HttpTransport::builder("http://127.0.0.1:3030")
                .build()
                .unwrap(),
        );
        let raw = r#"{"info":{},"parts":[],"error":{"message":"provider connection refused"}}"#;
        let transport_err = TransportError::HttpError {
            status: 500,
            base_url: "http://127.0.0.1:3030".to_string(),
            path: "/session/sess_1/prompt".to_string(),
            body: raw.to_string(),
        };
        let mapped = client.map_session_error("sess_1", transport_err);
        match mapped {
            TransportError::ServerError { message } => {
                assert!(
                    message.contains("provider connection refused"),
                    "got: {}",
                    message
                );
                assert!(
                    !message.contains("sess_1"),
                    "should not include session id in the error message"
                );
            }
            other => panic!("expected ServerError, got: {:?}", other),
        }
    }

    #[test]
    fn test_map_session_error_falls_back_to_raw_body() {
        let client = SessionClient::new(
            HttpTransport::builder("http://127.0.0.1:3030")
                .build()
                .unwrap(),
        );
        let raw = "internal server error — oops";
        let transport_err = TransportError::HttpError {
            status: 500,
            base_url: "http://127.0.0.1:3030".to_string(),
            path: "/session/sess_1/prompt".to_string(),
            body: raw.to_string(),
        };
        let mapped = client.map_session_error("sess_1", transport_err);
        match mapped {
            TransportError::ServerError { message } => {
                assert!(message.contains("internal server error"));
            }
            other => panic!("expected ServerError, got: {other:?}"),
        }
    }
}
