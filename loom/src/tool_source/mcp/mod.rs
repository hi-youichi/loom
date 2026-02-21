//! MCP ToolSource: connects to an MCP server via stdio or Streamable HTTP, implements ToolSource.
//!
//! Uses `McpSession` (stdio) or `McpHttpSession` (HTTP); maps MCP tools/list and
//! tools/call to `ToolSpec` and `ToolCallContent`. For Exa, HTTP is preferred when
//! the server URL is http(s).

mod session;
mod session_http;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::task;

use mcp_core::ResultMessage;

use crate::tool_source::{ToolCallContent, ToolSource, ToolSourceError, ToolSpec};

pub use session::{McpSession, McpSessionError};
pub use session_http::McpHttpSession;

/// Transport kind: stdio (spawn process) or HTTP (POST to URL).
/// HTTP variant uses `Arc` so we can release the mutex before awaiting.
enum McpSessionKind {
    Stdio(McpSession),
    Http(Arc<McpHttpSession>),
}

/// Tool source backed by an MCP server over stdio or HTTP.
///
/// Use `new` / `new_with_env` for stdio (spawn process). Use `new_http` when the
/// server URL is http(s) (e.g. Exa at https://mcp.exa.ai/mcp) so tools use HTTP
/// directly without mcp-remote. Implements `ToolSource` via `tools/list` and
/// `tools/call`. Used by ReAct's ActNode and by LLM `with_tools`.
///
/// **Interaction**: Implements `ToolSource`; used by ActNode and by examples
/// that pass tools to ChatOpenAI. Holds session behind Mutex for interior mutability.
pub struct McpToolSource {
    session: Mutex<McpSessionKind>,
}

impl McpToolSource {
    /// Creates a new McpToolSource by spawning the MCP server and initializing.
    /// Returns `Err` if spawn or initialize fails. Child process inherits only
    /// default env (HOME, PATH, etc.); no extra vars.
    /// When `stderr_verbose` is false, child stderr is discarded (quiet UX).
    ///
    /// **Interaction**: Caller provides `command` (e.g. `cargo`) and `args`
    /// (e.g. `["run", "-p", "mcp-filesystem-server", "--quiet"]`).
    pub fn new(
        command: impl Into<String>,
        args: Vec<String>,
        stderr_verbose: bool,
    ) -> Result<Self, McpSessionError> {
        let session =
            McpSession::new(command, args, None::<Vec<(String, String)>>, stderr_verbose)?;
        Ok(Self {
            session: Mutex::new(McpSessionKind::Stdio(session)),
        })
    }

    /// Like `new`, but passes the given env vars to the MCP server process.
    /// Use for servers that need credentials (e.g. GITLAB_TOKEN for GitLab MCP).
    /// When `stderr_verbose` is false, child stderr is discarded (quiet default UX).
    ///
    /// **Interaction**: Caller provides `command`, `args`, env key-value pairs,
    /// and `stderr_verbose` (e.g. from CLI `--verbose`).
    pub fn new_with_env(
        command: impl Into<String>,
        args: Vec<String>,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        stderr_verbose: bool,
    ) -> Result<Self, McpSessionError> {
        let session = McpSession::new(command, args, Some(env), stderr_verbose)?;
        Ok(Self {
            session: Mutex::new(McpSessionKind::Stdio(session)),
        })
    }

    /// Creates an MCP tool source over Streamable HTTP (no subprocess).
    /// Prefer this when the server URL is http(s) (e.g. Exa at https://mcp.exa.ai/mcp).
    /// `headers` are sent on every request (e.g. `[("EXA_API_KEY", api_key)]`).
    ///
    /// **Interaction**: Caller provides `url` and optional headers; used by
    /// `register_exa_mcp` when `mcp_exa_url` starts with `http://` or `https://`.
    pub async fn new_http(
        url: impl Into<String>,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Result<Self, ToolSourceError> {
        let session = McpHttpSession::new(url, headers).await?;
        Ok(Self {
            session: Mutex::new(McpSessionKind::Http(Arc::new(session))),
        })
    }

    /// Sends one JSON-RPC request and returns the result (stdio only; HTTP path uses async in `list_tools`/`call_tool`).
    fn request(
        &self,
        id: &str,
        method: &str,
        params: Value,
    ) -> Result<Option<ResultMessage>, ToolSourceError> {
        let mut kind = self
            .session
            .lock()
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        match &mut *kind {
            McpSessionKind::Stdio(s) => {
                s.send_request(id, method, params)
                    .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
                s.wait_for_result(id, std::time::Duration::from_secs(30))
                    .map_err(|e| ToolSourceError::Transport(e.to_string()))
            }
            McpSessionKind::Http(_) => unreachable!("HTTP session uses async request path"),
        }
    }

    /// Lists tools by sending `tools/list` and mapping result to `Vec<ToolSpec>`.
    fn list_tools_sync(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        let id = "loom-tools-list";
        let result = self.request(id, "tools/list", Value::Object(serde_json::Map::new()))?;
        let result = result
            .ok_or_else(|| ToolSourceError::Transport("timeout waiting for tools/list".into()))?;
        parse_list_tools_result(result)
    }

    /// Calls a tool by sending `tools/call` and extracting text from content.
    fn call_tool_sync(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let id = format!("loom-call-{}", name);
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let result = self
            .request(&id, "tools/call", params)?
            .ok_or_else(|| ToolSourceError::Transport("timeout waiting for tools/call".into()))?;
        parse_call_tool_result(result)
    }
}

/// Parses a `tools/list` JSON-RPC result into `Vec<ToolSpec>`.
fn parse_list_tools_result(result: ResultMessage) -> Result<Vec<ToolSpec>, ToolSourceError> {
    if let Some(err) = result.error {
        return Err(ToolSourceError::JsonRpc(err.message));
    }
    let tools_value = result
        .result
        .and_then(|r| r.get("tools").cloned())
        .ok_or_else(|| ToolSourceError::Transport("no tools in response".into()))?;
    let tools_array = tools_value
        .as_array()
        .ok_or_else(|| ToolSourceError::Transport("tools not an array".into()))?;
    let mut specs = Vec::with_capacity(tools_array.len());
    for t in tools_array {
        let obj = t
            .as_object()
            .ok_or_else(|| ToolSourceError::Transport("tool item not an object".into()))?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let input_schema = obj
            .get("inputSchema")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));
        specs.push(ToolSpec {
            name,
            description,
            input_schema,
        });
    }
    Ok(specs)
}

/// Parses a `tools/call` JSON-RPC result into `ToolCallContent`.
fn parse_call_tool_result(result: ResultMessage) -> Result<ToolCallContent, ToolSourceError> {
    if let Some(err) = result.error {
        return Err(ToolSourceError::JsonRpc(err.message));
    }
    let result_value = result
        .result
        .ok_or_else(|| ToolSourceError::Transport("no result in tools/call response".into()))?;
    if result_value
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let msg = result_value
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|b| b.get("text").and_then(|t| t.as_str()))
            .unwrap_or("tool returned error")
            .to_string();
        return Err(ToolSourceError::Transport(msg));
    }
    let mut text_parts = Vec::new();
    if let Some(content_array) = result_value.get("content").and_then(|c| c.as_array()) {
        for block in content_array {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t);
                }
            }
        }
    }
    let mut text = text_parts.join("\n").trim().to_string();
    if text.is_empty() {
        if let Some(structured) = result_value.get("structuredContent") {
            text = serde_json::to_string(structured).unwrap_or_default();
        }
    }
    if text.is_empty() {
        return Err(ToolSourceError::Transport(
            "no text or structuredContent in tools/call response".into(),
        ));
    }
    Ok(ToolCallContent { text })
}

#[async_trait]
impl ToolSource for McpToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        let arc = {
            let guard = self
                .session
                .lock()
                .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
            match &*guard {
                McpSessionKind::Stdio(_) => {
                    drop(guard);
                    return task::block_in_place(|| self.list_tools_sync());
                }
                McpSessionKind::Http(h) => Arc::clone(h),
            }
        };
        let result = arc
            .request(
                "loom-tools-list",
                "tools/list",
                Value::Object(serde_json::Map::new()),
            )
            .await?;
        parse_list_tools_result(result)
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let (arc, params) = {
            let guard = self
                .session
                .lock()
                .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
            match &*guard {
                McpSessionKind::Stdio(_) => {
                    drop(guard);
                    return task::block_in_place(|| self.call_tool_sync(name, arguments));
                }
                McpSessionKind::Http(h) => {
                    let params = serde_json::json!({ "name": name, "arguments": arguments });
                    (Arc::clone(h), params)
                }
            }
        };
        let id = format!("loom-call-{}", name);
        let result = arc.request(&id, "tools/call", params).await?;
        parse_call_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ErrorObject;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn read_http_request(stream: &mut TcpStream) -> (String, String) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        let header_end;
        loop {
            let n = stream.read(&mut tmp).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos + 4;
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = buf[header_end..].to_vec();
                while body.len() < content_length {
                    let m = stream.read(&mut tmp).await.unwrap();
                    if m == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..m]);
                }
                let body = String::from_utf8_lossy(&body[..content_length]).to_string();
                return (headers, body);
            }
        }
        (String::new(), String::new())
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: &str,
        content_type: Option<&str>,
        extra_headers: &[(&str, &str)],
        body: &str,
    ) {
        let mut resp = format!("HTTP/1.1 {}\r\nConnection: close\r\n", status);
        if let Some(ct) = content_type {
            resp.push_str(&format!("Content-Type: {}\r\n", ct));
        }
        for (k, v) in extra_headers {
            resp.push_str(&format!("{}: {}\r\n", k, v));
        }
        resp.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
        stream.write_all(resp.as_bytes()).await.unwrap();
    }

    /// **Scenario**: When command does not exist, McpToolSource::new returns an error.
    #[test]
    fn mcp_tool_source_new_invalid_command_returns_error() {
        let result = McpToolSource::new(
            "_nonexistent_command_that_does_not_exist_xyz_",
            vec![],
            false,
        );
        assert!(result.is_err(), "expected Err for nonexistent command");
    }

    #[test]
    fn parse_list_tools_result_success_maps_fields() {
        let result = ResultMessage::success(
            "1",
            serde_json::json!({
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read file content",
                        "inputSchema": {"type":"object","properties":{"path":{"type":"string"}}}
                    }
                ]
            }),
        );
        let tools = parse_list_tools_result(result).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[0].description.as_deref(), Some("Read file content"));
        assert_eq!(tools[0].input_schema["type"], "object");
    }

    #[test]
    fn parse_list_tools_result_errors_for_missing_or_invalid_tools() {
        let missing_tools = ResultMessage::success("1", serde_json::json!({}));
        assert!(matches!(
            parse_list_tools_result(missing_tools),
            Err(ToolSourceError::Transport(_))
        ));

        let non_array = ResultMessage::success("1", serde_json::json!({"tools": {}}));
        assert!(matches!(
            parse_list_tools_result(non_array),
            Err(ToolSourceError::Transport(_))
        ));
    }

    #[test]
    fn parse_list_tools_result_propagates_jsonrpc_error() {
        let err = ResultMessage::failure("1", ErrorObject::new(-32000, "rpc failed", None));
        assert!(matches!(
            parse_list_tools_result(err),
            Err(ToolSourceError::JsonRpc(msg)) if msg == "rpc failed"
        ));
    }

    #[test]
    fn parse_call_tool_result_joins_text_blocks() {
        let result = ResultMessage::success(
            "1",
            serde_json::json!({
                "content": [
                    {"type":"text","text":"line1"},
                    {"type":"image","text":"ignored"},
                    {"type":"text","text":"line2"}
                ]
            }),
        );
        let out = parse_call_tool_result(result).unwrap();
        assert_eq!(out.text, "line1\nline2");
    }

    #[test]
    fn parse_call_tool_result_uses_structured_content_fallback() {
        let result = ResultMessage::success(
            "1",
            serde_json::json!({
                "structuredContent": {"ok": true, "count": 2}
            }),
        );
        let out = parse_call_tool_result(result).unwrap();
        assert!(out.text.contains("\"ok\":true"));
    }

    #[test]
    fn parse_call_tool_result_errors_on_is_error_or_missing_content() {
        let is_error = ResultMessage::success(
            "1",
            serde_json::json!({
                "isError": true,
                "content": [{"type":"text","text":"boom"}]
            }),
        );
        assert!(matches!(
            parse_call_tool_result(is_error),
            Err(ToolSourceError::Transport(msg)) if msg == "boom"
        ));

        let missing = ResultMessage::success("1", serde_json::json!({}));
        assert!(matches!(
            parse_call_tool_result(missing),
            Err(ToolSourceError::Transport(_))
        ));
    }

    #[test]
    fn parse_call_tool_result_propagates_jsonrpc_error() {
        let err = ResultMessage::failure("1", ErrorObject::new(-32000, "call failed", None));
        assert!(matches!(
            parse_call_tool_result(err),
            Err(ToolSourceError::JsonRpc(msg)) if msg == "call failed"
        ));
    }

    #[tokio::test]
    async fn mcp_tool_source_http_list_and_call_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let methods: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let methods_clone = Arc::clone(&methods);
        let server = tokio::spawn(async move {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (_headers, body) = read_http_request(&mut stream).await;
                let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let method = json
                    .get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                methods_clone.lock().unwrap().push(method.clone());
                match method.as_str() {
                    "initialize" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-mcp-initialize",
                            "result":{"protocolVersion":"2025-11-25"}
                        })
                        .to_string();
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            Some("application/json"),
                            &[("MCP-Session-Id", "sess-1")],
                            &body,
                        )
                        .await;
                    }
                    "notifications/initialized" => {
                        write_http_response(&mut stream, "202 Accepted", None, &[], "").await;
                    }
                    "tools/list" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-tools-list",
                            "result":{"tools":[{"name":"http_tool","description":"from http","inputSchema":{"type":"object"}}]}
                        })
                        .to_string();
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            Some("application/json"),
                            &[],
                            &body,
                        )
                        .await;
                    }
                    "tools/call" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-call-http_tool",
                            "result":{"content":[{"type":"text","text":"ok-from-http"}]}
                        })
                        .to_string();
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            Some("application/json"),
                            &[],
                            &body,
                        )
                        .await;
                    }
                    _ => panic!("unexpected method: {}", method),
                }
            }
        });

        let url = format!("http://{}", addr);
        let source = McpToolSource::new_http(url, [("X-Test", "1")])
            .await
            .unwrap();
        let tools = source.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "http_tool");
        let out = source
            .call_tool("http_tool", serde_json::json!({"q":"x"}))
            .await
            .unwrap();
        assert_eq!(out.text, "ok-from-http");
        let called = methods.lock().unwrap().clone();
        assert_eq!(
            called,
            vec![
                "initialize".to_string(),
                "notifications/initialized".to_string(),
                "tools/list".to_string(),
                "tools/call".to_string()
            ]
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn mcp_tool_source_http_supports_sse_jsonrpc_responses() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (_, body) = read_http_request(&mut stream).await;
                let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let method = json.get("method").and_then(|m| m.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        write_http_response(&mut stream, "202 Accepted", None, &[], "").await;
                    }
                    "tools/list" => {
                        let sse = "data: {\"jsonrpc\":\"2.0\",\"id\":\"loom-tools-list\",\"result\":{\"tools\":[{\"name\":\"sse_tool\",\"description\":\"sse\",\"inputSchema\":{\"type\":\"object\"}}]}}\n\n";
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            Some("text/event-stream"),
                            &[],
                            sse,
                        )
                        .await;
                    }
                    _ => panic!("unexpected method: {}", method),
                }
            }
        });

        let source = McpToolSource::new_http(
            format!("http://{}", addr),
            std::iter::empty::<(String, String)>(),
        )
        .await
        .unwrap();
        let tools = source.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "sse_tool");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn mcp_tool_source_http_maps_jsonrpc_error_from_tools_call() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (_, body) = read_http_request(&mut stream).await;
                let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let method = json.get("method").and_then(|m| m.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        write_http_response(&mut stream, "202 Accepted", None, &[], "").await;
                    }
                    "tools/call" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-call-bad_tool",
                            "error":{"code":-32000,"message":"call failed"}
                        })
                        .to_string();
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            Some("application/json"),
                            &[],
                            &body,
                        )
                        .await;
                    }
                    _ => panic!("unexpected method: {}", method),
                }
            }
        });

        let source = McpToolSource::new_http(
            format!("http://{}", addr),
            std::iter::empty::<(String, String)>(),
        )
        .await
        .unwrap();
        let err = source
            .call_tool("bad_tool", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolSourceError::JsonRpc(msg) if msg == "call failed"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn mcp_tool_source_http_initialize_http_error_surfaces_transport_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_http_request(&mut stream).await;
            write_http_response(
                &mut stream,
                "500 Internal Server Error",
                Some("text/plain"),
                &[],
                "boom",
            )
            .await;
        });

        let err = match McpToolSource::new_http(
            format!("http://{}", addr),
            std::iter::empty::<(String, String)>(),
        )
        .await
        {
            Ok(_) => panic!("expected initialization to fail"),
            Err(e) => e,
        };
        assert!(matches!(err, ToolSourceError::Transport(msg) if msg.contains("initialize HTTP")));
        server.await.unwrap();
    }
}
