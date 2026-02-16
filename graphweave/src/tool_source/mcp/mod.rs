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
        let id = "graphweave-tools-list";
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
        let id = format!("graphweave-call-{}", name);
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
                "graphweave-tools-list",
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
        let id = format!("graphweave-call-{}", name);
        let result = arc.request(&id, "tools/call", params).await?;
        parse_call_tool_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
