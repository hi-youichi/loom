//! MCP session: stdio transport with initialize handshake and request/response.
//!
//! Wraps `StdioClientTransport` from mcp_client; used by `McpToolSource` for
//! `tools/list` and `tools/call`. Does not handle resources or prompts.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use mcp_client::stdio::{
    JsonRpcMessage, StdioClientTransport, StdioClientTransportError, StdioServerParameters,
    StdioStream,
};
use mcp_core::{MessageId, NotificationMessage, RequestMessage, ResultMessage};
use serde_json::{json, Value};

/// Protocol version for MCP initialize.
const PROTOCOL_VERSION: &str = "2025-11-25";
/// Request id for initialize.
const INITIALIZE_REQUEST_ID: &str = "loom-mcp-initialize";

/// MCP session over stdio: spawns server process, performs initialize handshake,
/// provides `send_request` and `wait_for_result` for JSON-RPC calls.
///
/// **Interaction**: Created by `McpToolSource::new`; used internally for
/// `tools/list` and `tools/call`. Holds `StdioClientTransport` and an `mpsc`
/// receiver for incoming messages.
pub struct McpSession {
    transport: StdioClientTransport,
    receiver: mpsc::Receiver<JsonRpcMessage>,
}

impl McpSession {
    /// Creates a new MCP session by spawning the server process and completing
    /// the initialize handshake. Returns `Err` if spawn or initialize fails.
    ///
    /// **Interaction**: Called by `McpToolSource::new` / `new_with_env`. Uses
    /// `StdioClientTransport` from mcp_client; sends `initialize` then
    /// `notifications/initialized`. Optional `env` is passed to the child process
    /// (e.g. GITLAB_TOKEN for GitLab MCP server).
    /// When `stderr_verbose` is false, child stderr is discarded for quiet default UX;
    /// when true, child stderr is inherited so MCP proxy debug logs are visible.
    pub fn new(
        command: impl Into<String>,
        args: Vec<String>,
        env: Option<impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>>,
        stderr_verbose: bool,
    ) -> Result<Self, McpSessionError> {
        let (tx, rx) = mpsc::channel();

        let stderr_stream = if stderr_verbose {
            StdioStream::Inherit
        } else {
            StdioStream::Null
        };
        let mut params = StdioServerParameters::new(command)
            .args(args)
            .stderr(stderr_stream);
        if let Some(env_iter) = env {
            params = params.env(env_iter);
        }

        let mut transport = StdioClientTransport::new(params);
        transport.on_message(move |msg| {
            let _ = tx.send(msg);
        });
        transport.on_error(|e| {
            eprintln!("[McpSession] transport error: {}", e);
        });

        transport.start().map_err(McpSessionError::Transport)?;

        let mut session = Self {
            transport,
            receiver: rx,
        };
        session.initialize()?;
        Ok(session)
    }

    /// Performs MCP initialize handshake: send `initialize`, wait for result,
    /// send `notifications/initialized`. Uses empty roots for tools-only use.
    fn initialize(&mut self) -> Result<(), McpSessionError> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "clientInfo": {
                "name": "loom-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        self.send_request(INITIALIZE_REQUEST_ID, "initialize", params)?;

        match self.wait_for_result(INITIALIZE_REQUEST_ID, Duration::from_secs(20))? {
            Some(result) => {
                if result.error.is_some() {
                    return Err(McpSessionError::Initialize(
                        result
                            .error
                            .map(|e| e.message)
                            .unwrap_or_else(|| "unknown".into()),
                    ));
                }
                let notification = JsonRpcMessage::Notification(NotificationMessage::new(
                    "notifications/initialized",
                    Some(json!({})),
                ));
                self.transport
                    .send(&notification)
                    .map_err(McpSessionError::Transport)?;
            }
            None => {
                return Err(McpSessionError::Initialize(
                    "timeout waiting for initialize".into(),
                ))
            }
        }

        Ok(())
    }

    /// Sends a JSON-RPC request. Does not wait for the response.
    pub fn send_request(
        &mut self,
        id: &str,
        method: &str,
        params: Value,
    ) -> Result<(), McpSessionError> {
        let request = RequestMessage::new(id, method, params);
        self.transport
            .send(&JsonRpcMessage::Request(request))
            .map_err(McpSessionError::Transport)
    }

    /// Waits for a JSON-RPC result matching the given request id. Handles
    /// `roots/list` requests from the server by responding with empty roots.
    pub fn wait_for_result(
        &mut self,
        request_id: &str,
        timeout: Duration,
    ) -> Result<Option<ResultMessage>, McpSessionError> {
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(1));

            match self.receiver.recv_timeout(remaining) {
                Ok(JsonRpcMessage::Result(msg)) if message_id_matches(&msg.id, request_id) => {
                    return Ok(Some(msg));
                }
                Ok(JsonRpcMessage::Request(req)) if req.method == "roots/list" => {
                    let result = ResultMessage::success(req.id.clone(), json!({ "roots": [] }));
                    self.transport
                        .send(&JsonRpcMessage::Result(result))
                        .map_err(McpSessionError::Transport)?;
                }
                Ok(JsonRpcMessage::Request(_)) | Ok(JsonRpcMessage::Result(_)) => {}
                Ok(JsonRpcMessage::Notification(_)) => {}
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(None)
    }
}

fn message_id_matches(id: &MessageId, expected: &str) -> bool {
    id.as_str() == Some(expected)
}

/// Errors from McpSession operations.
#[derive(Debug, thiserror::Error)]
pub enum McpSessionError {
    #[error("transport: {0}")]
    Transport(#[from] StdioClientTransportError),
    #[error("initialize: {0}")]
    Initialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_python_server(script: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake_mcp_server.py");
        std::fs::write(&path, script).unwrap();
        // Keep tempdir alive by leaking it for test lifetime.
        let _ = Box::leak(Box::new(dir));
        path.to_string_lossy().to_string()
    }

    #[test]
    fn message_id_matches_checks_expected_id() {
        assert!(message_id_matches(&MessageId::from("abc"), "abc"));
        assert!(!message_id_matches(&MessageId::from("abc"), "def"));
    }

    #[test]
    fn mcp_session_new_and_roundtrip_requests_with_fake_python_server() {
        let script_path = write_python_server(
            r#"
import json, sys

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":"roots-1",
            "method":"roots/list",
            "params":{}
        }), flush=True)
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":msg["id"],
            "result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}}}
        }), flush=True)
    elif method == "tools/list":
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":"other-id",
            "result":{"ignored":True}
        }), flush=True)
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":msg["id"],
            "result":{"tools":[{"name":"fake_tool","description":"demo","inputSchema":{"type":"object"}}]}
        }), flush=True)
    elif method == "tools/call":
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":msg["id"],
            "result":{"content":[{"type":"text","text":"ok-from-fake"}]}
        }), flush=True)
"#,
        );

        let mut session = McpSession::new(
            "python3",
            vec![script_path],
            None::<Vec<(String, String)>>,
            false,
        )
        .unwrap();

        session
            .send_request("tools-list-1", "tools/list", json!({}))
            .unwrap();
        let list_result = session
            .wait_for_result("tools-list-1", Duration::from_secs(2))
            .unwrap()
            .expect("tools/list response");
        assert!(list_result.error.is_none());
        let tools = list_result
            .result
            .as_ref()
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(tools.len(), 1);

        session
            .send_request(
                "tools-call-1",
                "tools/call",
                json!({"name":"fake_tool","arguments":{}}),
            )
            .unwrap();
        let call_result = session
            .wait_for_result("tools-call-1", Duration::from_secs(2))
            .unwrap()
            .expect("tools/call response");
        assert!(call_result.error.is_none());
    }

    #[test]
    fn wait_for_result_times_out_when_no_matching_response() {
        let script_path = write_python_server(
            r#"
import json, sys

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":msg["id"],
            "result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}}}
        }), flush=True)
    # Intentionally do not respond to "no_reply"
"#,
        );

        let mut session = McpSession::new(
            "python3",
            vec![script_path],
            None::<Vec<(String, String)>>,
            false,
        )
        .unwrap();
        session
            .send_request("no-reply-id", "no_reply", json!({}))
            .unwrap();
        let result = session
            .wait_for_result("no-reply-id", Duration::from_millis(200))
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn mcp_session_new_returns_initialize_error_on_rpc_error() {
        let script_path = write_python_server(
            r#"
import json, sys

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    msg = json.loads(raw)
    if msg.get("method") == "initialize":
        print(json.dumps({
            "jsonrpc":"2.0",
            "id":msg["id"],
            "error":{"code":-32000,"message":"init failed"}
        }), flush=True)
"#,
        );

        let err = match McpSession::new(
            "python3",
            vec![script_path],
            None::<Vec<(String, String)>>,
            false,
        ) {
            Ok(_) => panic!("expected initialize error"),
            Err(e) => e,
        };
        assert!(matches!(err, McpSessionError::Initialize(msg) if msg == "init failed"));
    }
}
