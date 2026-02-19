//! MCP session over Streamable HTTP: POST JSON-RPC to a URL, parse JSON response.
//!
//! Used when `MCP_EXA_URL` is an http(s) URL so Exa tools use HTTP directly
//! instead of spawning mcp-remote. Implements MCP Streamable HTTP transport:
//! POST single JSON-RPC message, Accept: application/json and text/event-stream,
//! optional MCP-Session-Id and MCP-Protocol-Version headers.
//!
//! **Interaction**: Created by `McpToolSource::new_http`; used for `initialize`,
//! `tools/list`, and `tools/call` when the server URL is http(s).
//! Uses async reqwest; safe to create and use from async/tokio context.

use std::sync::Mutex;

use mcp_core::{ErrorObject, MessageId, NotificationMessage, RequestMessage, ResultMessage};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool_source::ToolSourceError;

/// MCP protocol version for HTTP header.
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
/// Request id for initialize.
const INITIALIZE_REQUEST_ID: &str = "loom-mcp-initialize";

/// JSON-RPC error object in response body.
#[derive(Debug, Deserialize)]
struct JsonRpcErrorBody {
    code: i64,
    message: String,
}

/// JSON-RPC response body (id + result or error).
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    id: Option<MessageId>,
    result: Option<Value>,
    error: Option<JsonRpcErrorBody>,
}

/// Parses JSON-RPC response from HTTP body. Supports both application/json (single
/// JSON object) and text/event-stream (SSE: data lines with JSON-RPC messages).
/// Returns the first JSON-RPC response (has result or error) found in the body.
fn parse_json_rpc_from_body(
    body: &str,
    content_type: Option<&reqwest::header::HeaderValue>,
) -> Result<JsonRpcResponse, ToolSourceError> {
    let is_sse = content_type
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    if is_sse {
        let mut data_buffer = String::new();
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" || data.is_empty() {
                    if !data_buffer.is_empty() {
                        if let Ok(r) = serde_json::from_str::<JsonRpcResponse>(&data_buffer) {
                            if r.result.is_some() || r.error.is_some() {
                                return Ok(r);
                            }
                        }
                        data_buffer.clear();
                    }
                    continue;
                }
                if data_buffer.is_empty() {
                    data_buffer = data.to_string();
                } else {
                    data_buffer.push('\n');
                    data_buffer.push_str(data);
                }
                if let Ok(r) = serde_json::from_str::<JsonRpcResponse>(&data_buffer) {
                    if r.result.is_some() || r.error.is_some() {
                        return Ok(r);
                    }
                }
            } else if line.trim().is_empty() {
                if !data_buffer.is_empty() {
                    if let Ok(r) = serde_json::from_str::<JsonRpcResponse>(&data_buffer) {
                        if r.result.is_some() || r.error.is_some() {
                            return Ok(r);
                        }
                    }
                    data_buffer.clear();
                }
            }
        }
        if !data_buffer.is_empty() {
            if let Ok(r) = serde_json::from_str::<JsonRpcResponse>(&data_buffer) {
                if r.result.is_some() || r.error.is_some() {
                    return Ok(r);
                }
            }
        }
        Err(ToolSourceError::Transport(
            "SSE stream: no JSON-RPC response (result/error) found".into(),
        ))
    } else {
        serde_json::from_str(body)
            .map_err(|e| ToolSourceError::Transport(format!("response json: {}", e)))
    }
}

/// MCP session over Streamable HTTP.
///
/// Performs initialize handshake via POST, then supports request/response
/// for tools/list and tools/call. Uses async reqwest; safe to create and drop
/// from async/tokio context (no nested runtime).
pub struct McpHttpSession {
    client: Client,
    url: String,
    /// Extra headers (e.g. EXA_API_KEY) sent on every request.
    headers: Vec<(String, String)>,
    /// Session id from server MCP-Session-Id header; sent on subsequent requests.
    session_id: Mutex<Option<String>>,
}

impl McpHttpSession {
    /// Creates a new HTTP MCP session and completes the initialize handshake.
    ///
    /// `url` must be the MCP endpoint (e.g. `https://mcp.exa.ai/mcp`).
    /// `headers` are added to every request (e.g. `[("EXA_API_KEY", key)]`).
    pub async fn new(
        url: impl Into<String>,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Result<Self, ToolSourceError> {
        let url = url.into();
        let headers: Vec<(String, String)> = headers
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let session_id = Mutex::new(None);
        let mut s = Self {
            client,
            url: url.clone(),
            headers,
            session_id,
        };
        s.initialize().await?;
        Ok(s)
    }

    /// Performs MCP initialize: POST initialize, capture MCP-Session-Id, POST notifications/initialized.
    async fn initialize(&mut self) -> Result<(), ToolSourceError> {
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "clientInfo": {
                "name": "loom-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        let request = RequestMessage::new(INITIALIZE_REQUEST_ID, "initialize", params);
        let body =
            serde_json::to_vec(&request).map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .body(body);
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let status = resp.status();
        let session_id = resp
            .headers()
            .get("MCP-Session-Id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        if let Some(ref id) = session_id {
            *self
                .session_id
                .lock()
                .map_err(|e| ToolSourceError::Transport(e.to_string()))? = Some(id.clone());
        }
        if status == reqwest::StatusCode::ACCEPTED {
            return Ok(());
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ToolSourceError::Transport(format!(
                "initialize HTTP {}: {}",
                status,
                if text.is_empty() { "no body" } else { &text }
            )));
        }
        let content_type = resp.headers().get("content-type").cloned();
        let text = resp
            .text()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("initialize response body: {}", e)))?;
        let _: JsonRpcResponse = parse_json_rpc_from_body(&text, content_type.as_ref())
            .map_err(|e| ToolSourceError::Transport(format!("initialize {}", e)))?;

        let notification = NotificationMessage::new("notifications/initialized", Some(json!({})));
        let notif_body = serde_json::to_vec(&notification)
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let mut req2 = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .body(notif_body);
        for (k, v) in &self.headers {
            req2 = req2.header(k.as_str(), v.as_str());
        }
        if let Some(ref id) = *self
            .session_id
            .lock()
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?
        {
            req2 = req2.header("MCP-Session-Id", id.as_str());
        }
        let resp2 = req2
            .send()
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let status2 = resp2.status();
        if status2 != reqwest::StatusCode::ACCEPTED && !status2.is_success() {
            let text = resp2.text().await.unwrap_or_default();
            return Err(ToolSourceError::Transport(format!(
                "notifications/initialized HTTP {}: {}",
                status2,
                if text.is_empty() { "no body" } else { &text }
            )));
        }
        Ok(())
    }

    /// Sends a JSON-RPC request and returns the parsed result (one POST, one response).
    ///
    /// Used by McpToolSource for tools/list and tools/call. Response must be
    /// Content-Type: application/json with a single JSON-RPC response.
    pub async fn request(
        &self,
        id: &str,
        method: &str,
        params: Value,
    ) -> Result<ResultMessage, ToolSourceError> {
        let request = RequestMessage::new(id, method, params);
        let body =
            serde_json::to_vec(&request).map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .body(body);
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        if let Ok(guard) = self.session_id.lock() {
            if let Some(ref sid) = *guard {
                req = req.header("MCP-Session-Id", sid.as_str());
            }
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ToolSourceError::Transport(format!(
                "{} HTTP {}: {}",
                method,
                status,
                if text.is_empty() { "no body" } else { &text }
            )));
        }
        let content_type = resp.headers().get("content-type").cloned();
        let text = resp
            .text()
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let json: JsonRpcResponse = parse_json_rpc_from_body(&text, content_type.as_ref())
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        let msg_id = json.id.unwrap_or_else(|| MessageId::from(id));
        if let Some(err) = json.error {
            let err_obj = ErrorObject::new(err.code as i32, err.message, None);
            return Ok(ResultMessage::failure(msg_id, err_obj));
        }
        Ok(ResultMessage::success(
            msg_id,
            json.result.unwrap_or(Value::Null),
        ))
    }
}
