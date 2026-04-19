//! E2E test infrastructure for loom-acp
//!
//! Spawns the loom-acp binary as a subprocess and communicates via JSON-RPC over stdin/stdout.
//!
//! # Mock LLM support
//!
//! [`AcpChild::spawn_with_mock`] starts a local HTTP mock server (via `wiremock`) that simulates
//! an OpenAI-compatible API. A temporary `LOOM_HOME` directory with `config.toml` is created so
//! loom-acp routes all LLM calls to the mock server — no real API keys needed.
//!
//! ## Environment variable handling
//!
//! When spawning with a mock, we override `LOOM_HOME` to point at a temp config **and** explicitly
//! set `OPENAI_BASE_URL` / `OPENAI_API_KEY` in the child process so that existing env vars in the
//! parent shell do not take precedence (the config priority is: existing env > config.toml).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Mock LLM server
// ---------------------------------------------------------------------------

/// A mock OpenAI-compatible API server for e2e tests.
///
/// Wraps [`wiremock::MockServer`] and provides helpers to mount canned responses.
/// The server listens on a random port and is cleaned up on drop.
///
/// All response helpers mount **streaming (SSE)** responses because the Loom OpenAI client
/// always calls `invoke_stream` / `invoke_stream_with_tool_delta` which sets `stream: true`.
pub struct MockLlmServer {
    server: wiremock::MockServer,
}

/// Build an SSE body string for a chat completion stream.
fn build_sse_body(content: &str) -> String {
    let chunk_id = "chatcmpl-mock";
    let model = "mock-model";

    let role_chunk = serde_json::json!({
        "id": chunk_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{ "index": 0, "delta": { "role": "assistant" }, "finish_reason": null }],
    });
    let content_chunk = serde_json::json!({
        "id": chunk_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{ "index": 0, "delta": { "content": content }, "finish_reason": null }],
    });
    let done_chunk = serde_json::json!({
        "id": chunk_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 },
    });

    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        serde_json::to_string(&role_chunk).unwrap(),
        serde_json::to_string(&content_chunk).unwrap(),
        serde_json::to_string(&done_chunk).unwrap(),
    )
}

/// Represents a tool call response from the mock LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    pub tool_name: String,
    pub parameters: serde_json::Value,
}

impl MockLlmServer {
    /// Start a new mock server on a random port.
    pub async fn start() -> Self {
        let server = wiremock::MockServer::start().await;
        Self { server }
    }

    /// The base URL of the mock server (e.g. `http://127.0.0.1:42137`).
    pub fn url(&self) -> String {
        self.server.uri()
    }

    /// The `/v1` prefixed URL for OpenAI-compatible API base.
    pub fn v1_url(&self) -> String {
        format!("{}/v1", self.url())
    }

    /// Mount a simple text completion response (SSE format).
    ///
    /// Any `POST /v1/chat/completions` request will receive the given `content` as an SSE
    /// stream with `finish_reason: "stop"`.
    ///
    /// Uses `set_body_raw` with `text/event-stream` MIME type because `set_body_string`
    /// defaults to `text/plain` which breaks the SSE parser.
    pub async fn mount_simple_response(&self, content: &str) {
        use wiremock::{Mock, ResponseTemplate};
        use wiremock::matchers::{method, path};

        let sse_body = build_sse_body(content);

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(sse_body, "text/event-stream"),
            )
            .mount(&self.server)
            .await;
    }

    /// Mount a tool call response (SSE format).
    ///
    /// Any `POST /v1/chat/completions` request will receive the given `tool_calls` as an SSE
    /// stream with `finish_reason: "tool_calls"`.
    ///
    /// Uses `up_to_n_times(1)` so the mock only matches the **first** LLM call.
    /// You should also mount a simple text response (via `mount_simple_response`) for
    /// subsequent LLM calls so the ReAct loop can terminate.
    pub async fn mount_tool_call_response(&self, tool_calls: &[ToolCallResponse]) {
        use wiremock::{Mock, ResponseTemplate};
        use wiremock::matchers::{method, path};

        let sse_body = build_tool_call_sse_body(tool_calls);

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(sse_body, "text/event-stream"),
            )
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&self.server)
            .await;
    }
}

/// Build an SSE body string for a tool call completion stream.
fn build_tool_call_sse_body(tool_calls: &[ToolCallResponse]) -> String {
    let chunk_id = "chatcmpl-mock";
    let model = "mock-model";

    let role_chunk = serde_json::json!({
        "id": chunk_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{ "index": 0, "delta": { "role": "assistant" }, "finish_reason": null }],
    });

    let mut chunks = vec![serde_json::to_string(&role_chunk).unwrap()];

    for (i, tool_call) in tool_calls.iter().enumerate() {
        let tool_call_chunk = serde_json::json!({
            "id": chunk_id,
            "object": "chat.completion.chunk",
            "created": 0,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": i,
                        "id": format!("call_{}", i),
                        "type": "function",
                        "function": {
                            "name": tool_call.tool_name,
                            "arguments": serde_json::to_string(&tool_call.parameters).unwrap()
                        }
                    }]
                },
                "finish_reason": null
            }],
        });
        chunks.push(serde_json::to_string(&tool_call_chunk).unwrap());
    }

    let done_chunk = serde_json::json!({
        "id": chunk_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 },
    });

    chunks.push(serde_json::to_string(&done_chunk).unwrap());

    format!(
        "{}\ndata: [DONE]\n\n",
        chunks.iter().map(|c| format!("data: {}\n\n", c)).collect::<String>()
    )
}

// ---------------------------------------------------------------------------
// Temporary LOOM_HOME with config
// ---------------------------------------------------------------------------

/// A temporary directory that acts as `LOOM_HOME`, containing a `config.toml` that points
/// to a mock LLM server.
///
/// The directory and its contents are cleaned up on drop.
pub struct TempLoomHome {
    dir: tempfile::TempDir,
}

impl TempLoomHome {
    /// Create a temp directory and write `config.toml` pointing at `mock_v1_url`
    /// (e.g. `http://127.0.0.1:42137/v1`).
    pub fn new(mock_v1_url: &str) -> std::io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let config_content = format!(
            r#"
[default]
provider = "mock"

[[providers]]
name = "mock"
api_key = "mock-key"
base_url = "{mock_v1_url}"
model = "mock-model"
type = "openai"
"#
        );
        std::fs::write(dir.path().join("config.toml"), config_content)?;
        Ok(Self { dir })
    }

    /// Path to the temporary `LOOM_HOME` directory.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

// ---------------------------------------------------------------------------
// AcpChild
// ---------------------------------------------------------------------------

/// E2E test helper that spawns loom-acp as a subprocess and communicates via JSON-RPC
pub struct AcpChild {
    child: Child,
    stdin: ChildStdin,
    stdout_reader: BufReader<ChildStdout>,
    next_id: u64,
    /// Kept alive so the temp dir is not cleaned up before the subprocess exits.
    _temp_loom_home: Option<TempLoomHome>,
}

#[derive(Debug, Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

impl AcpChild {
    // -- spawning ----------------------------------------------------------

    /// Spawn loom-acp as a subprocess (no mock LLM).
    pub fn spawn(log_file: Option<&Path>) -> std::io::Result<Self> {
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--bin", "loom-acp", "--"]);

        if let Some(log_file) = log_file {
            cmd.args(["--log-file", log_file.to_str().unwrap()]);
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        Self::from_command(cmd, None)
    }

    /// Spawn loom-acp with a mock LLM server.
    ///
    /// Returns `(AcpChild, MockLlmServer)` so the caller can mount additional responses or
    /// inspect received requests. The mock server is kept alive for the lifetime of the child.
    ///
    /// This method:
    /// 1. Starts a `wiremock` HTTP server on a random port.
    /// 2. Writes a temp `config.toml` with the mock server URL.
    /// 3. Spawns loom-acp with `LOOM_HOME` pointing at the temp dir **and** explicitly sets
    ///    `OPENAI_BASE_URL` / `OPENAI_API_KEY` so that existing env vars in the parent shell
    ///    are overridden in the child process.
    pub async fn spawn_with_mock() -> std::io::Result<(Self, MockLlmServer)> {
        let mock = MockLlmServer::start().await;
        mock.mount_simple_response("Hello from mock LLM!").await;

        let temp_home = TempLoomHome::new(&mock.v1_url())?;

        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--bin", "loom-acp", "--"]);

        // Override LOOM_HOME so config.toml is read from our temp dir.
        cmd.env("LOOM_HOME", temp_home.path());

        // Explicitly override OpenAI env vars so that any real API keys in the parent
        // shell environment do not take precedence (config priority: existing env > config.toml).
        cmd.env("OPENAI_BASE_URL", mock.v1_url());
        cmd.env("OPENAI_API_KEY", "mock-key");
        cmd.env("MODEL", "mock-model");

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let acp = Self::from_command(cmd, Some(temp_home))?;

        Ok((acp, mock))
    }

    fn from_command(
        mut cmd: Command,
        temp_loom_home: Option<TempLoomHome>,
    ) -> std::io::Result<Self> {
        let mut child = cmd.spawn()?;

        let stdin = child.stdin.take().expect("stdin should be available");
        let stdout = child.stdout.take().expect("stdout should be available");
        let stdout_reader = BufReader::new(stdout);

        Ok(Self {
            child,
            stdin,
            stdout_reader,
            next_id: 1,
            _temp_loom_home: temp_loom_home,
        })
    }

    // -- JSON-RPC helpers --------------------------------------------------

    /// Send JSON-RPC request and return the request id
    pub fn send_request(&mut self, method: &str, params: Value) -> std::io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;

        let request = RpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&request)?;
        writeln!(self.stdin, "{}", json)?;
        self.stdin.flush()?;

        Ok(id)
    }

    /// Send JSON-RPC notification (no id, no response expected)
    pub fn send_notification(&mut self, method: &str, params: Value) -> std::io::Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let json = serde_json::to_string(&notification)?;
        writeln!(self.stdin, "{}", json)?;
        self.stdin.flush()
    }

    /// Read one JSON-RPC message (response or notification)
    pub fn read_message(&mut self) -> std::io::Result<Value> {
        let mut line = String::new();

        // Skip empty lines
        loop {
            line.clear();
            let bytes_read = self.stdout_reader.read_line(&mut line)?;
            if bytes_read > 0 && !line.trim().is_empty() {
                break;
            }
            if bytes_read == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "stdout closed"));
            }
        }

        let value: Value = serde_json::from_str(line.trim())?;
        Ok(value)
    }

    /// Wait for a response with the given id (with timeout)
    pub fn wait_for_response(&mut self, id: u64, timeout: Duration) -> std::io::Result<RpcResponse> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            let message = self.read_message()?;

            // Check if this is the response we're looking for
            if let Some(msg_id) = message.get("id").and_then(|v| v.as_u64()) {
                if msg_id == id {
                    let response: RpcResponse = serde_json::from_value(message)?;
                    return Ok(response);
                }
            }
            // Otherwise it's a notification or response to another request, skip it
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timeout waiting for response id {}", id),
        ))
    }

    /// Send a request and wait for its response (convenience method)
    pub fn send_request_and_wait(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> std::io::Result<RpcResponse> {
        let id = self.send_request(method, params)?;
        self.wait_for_response(id, timeout)
    }

    /// Wait for a notification of the given method (with timeout)
    pub fn wait_for_notification(&mut self, method: &str, timeout: Duration) -> std::io::Result<RpcNotification> {
        let start = Instant::now();

        while start.elapsed() < timeout {
            let message = self.read_message()?;

            if message.get("method").and_then(|v| v.as_str()) == Some(method) {
                let notification: RpcNotification = serde_json::from_value(message)?;
                return Ok(notification);
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timeout waiting for notification '{}'", method),
        ))
    }

    /// Perform full handshake: initialize + session/new, returns session_id.
    pub fn handshake(&mut self, timeout: Duration) -> std::io::Result<String> {
        let init = self.send_request_and_wait(
            "initialize",
            serde_json::json!({ "protocolVersion": 1 }),
            timeout,
        )?;
        if let Some(err) = init.error {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("initialize failed: {} ({})", err.message, err.code),
            ));
        }

        let sess = self.send_request_and_wait(
            "session/new",
            serde_json::json!({
                "cwd": std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).to_str().unwrap(),
                "mcpServers": [],
            }),
            timeout,
        )?;
        if let Some(err) = sess.error {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("session/new failed: {} ({})", err.message, err.code),
            ));
        }

        Ok(sess
            .result
            .and_then(|r| r.get("sessionId").cloned())
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default())
    }

    /// Wait for and collect all session/update notifications within timeout
    pub fn wait_for_session_updates(&mut self, timeout: Duration) -> std::io::Result<Vec<RpcNotification>> {
        let start = Instant::now();
        let mut updates = Vec::new();

        while start.elapsed() < timeout {
            let message = self.read_message()?;

            // Check if this is a session/update notification
            if message.get("method").and_then(|v| v.as_str()) == Some("session/update") {
                let notification: RpcNotification = serde_json::from_value(message)?;
                updates.push(notification);
            }
            
            // If we haven't received any updates for a while, assume we're done
            if !updates.is_empty() && start.elapsed() > Duration::from_secs(2) {
                break;
            }
        }

        Ok(updates)
    }

    /// Assert that a session/update contains Diff content for the expected path
    pub fn assert_diff_content(&self, update: &RpcNotification, expected_path: &str) -> std::io::Result<()> {
        if let Some(session_update) = update.params.get("sessionUpdate") {
            if let Some(tool_call_update) = session_update.get("toolCallUpdate") {
                if let Some(content) = tool_call_update.get("content") {
                    if let Some(content_array) = content.as_array() {
                        let diff_found = content_array.iter().any(|item| item.get("diff").is_some());
                        
                        if !diff_found {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Expected Diff content, got: {:?}", content_array),
                            ));
                        }

                        // Verify path if provided
                        if let Some(diff_content) = content_array.iter().find(|item| item.get("diff").is_some()) {
                            if let Some(diff) = diff_content.get("diff") {
                                if let Some(path) = diff.get("path").and_then(|p| p.as_str()) {
                                    if path != expected_path {
                                        return Err(std::io::Error::new(
                                            std::io::ErrorKind::InvalidData,
                                            format!("Expected path '{}', got '{}'", expected_path, path),
                                        ));
                                    }
                                }
                            }
                        }
                        
                        return Ok(());
                    }
                }
            }
        }
        
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Expected tool_call_update with Diff content",
        ))
    }

    /// Find and return the first session/update notification with Diff content
    pub fn find_diff_update<'a>(&self, updates: &'a [RpcNotification]) -> Option<&'a RpcNotification> {
        updates.iter().find(|notification| {
            if let Some(session_update) = notification.params.get("sessionUpdate") {
                if let Some(tool_call_update) = session_update.get("toolCallUpdate") {
                    if let Some(content) = tool_call_update.get("content") {
                        if let Some(content_array) = content.as_array() {
                            return content_array.iter().any(|item| item.get("diff").is_some());
                        }
                    }
                }
            }
            false
        })
    }

    // -- process management ------------------------------------------------

    /// Kill the child process
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    /// Wait for the child to exit
    pub fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait()
    }
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_acp() {
        let mut acp = AcpChild::spawn(None).expect("should spawn loom-acp");
        let _ = acp.kill();
    }

    #[tokio::test]
    async fn test_mock_server_starts() {
        let mock = MockLlmServer::start().await;
        assert!(!mock.url().is_empty());
    }

    #[test]
    fn test_temp_loom_home_creates_config() {
        let home = TempLoomHome::new("http://localhost:12345/v1").unwrap();
        let config_path = home.path().join("config.toml");
        assert!(config_path.exists());

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("base_url = \"http://localhost:12345/v1\""));
        assert!(content.contains("api_key = \"mock-key\""));
    }
}
