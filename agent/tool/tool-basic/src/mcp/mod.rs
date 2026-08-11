//! MCP ToolSource: connects to an MCP server via stdio or Streamable HTTP, implements ToolSource.

mod session;
mod session_http;

use std::time::Duration;

use serde_json::Value;

use tool_core::{ToolCallContent, ToolOutputHint, ToolOutputStrategy, ToolSourceError, ToolSpec};

pub use session::{McpSession, McpSessionError};
pub use session_http::McpHttpSession;

/// Tool source backed by an MCP server over stdio or HTTP.
///
/// Use `new` / `new_with_env` for stdio (spawn process). Use `new_http` when the
/// server URL is http(s). Implements `ToolSource` via rmcp `list_tools` and `call_tool`.
pub struct McpToolSource {
    client: McpClient,
    tool_timeout: Duration,
}

/// Default timeout for one MCP tool call.
pub const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(60);

enum McpClient {
    Stdio(McpSession),
    Http(McpHttpSession),
}

impl McpToolSource {
    /// Gracefully closes an owned MCP session. HTTP sources are connection
    /// pools owned by reqwest and do not need an explicit shutdown hook.
    pub async fn shutdown(&self) {
        if let McpClient::Stdio(session) = &self.client {
            session.shutdown().await;
        }
    }

    pub async fn is_closed(&self) -> bool {
        match &self.client {
            McpClient::Stdio(session) => session.is_closed().await,
            McpClient::Http(_) => false,
        }
    }

    /// Creates a new McpToolSource by spawning the MCP server and initializing via rmcp.
    pub async fn new(
        command: impl Into<String>,
        args: Vec<String>,
        stderr_verbose: bool,
    ) -> Result<Self, McpSessionError> {
        let session =
            McpSession::new(command, args, None::<Vec<(String, String)>>, stderr_verbose).await?;
        Ok(Self {
            client: McpClient::Stdio(session),
            tool_timeout: DEFAULT_TOOL_TIMEOUT,
        })
    }

    /// Like `new`, but passes the given env vars to the MCP server process.
    pub async fn new_with_env(
        command: impl Into<String>,
        args: Vec<String>,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        stderr_verbose: bool,
    ) -> Result<Self, McpSessionError> {
        let session = McpSession::new(command, args, Some(env), stderr_verbose).await?;
        Ok(Self {
            client: McpClient::Stdio(session),
            tool_timeout: DEFAULT_TOOL_TIMEOUT,
        })
    }

    /// Creates an MCP tool source over Streamable HTTP (no subprocess).
    pub async fn new_http(
        url: impl Into<String>,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Result<Self, ToolSourceError> {
        let session = McpHttpSession::new(url, headers).await?;
        Ok(Self {
            client: McpClient::Http(session),
            tool_timeout: DEFAULT_TOOL_TIMEOUT,
        })
    }

    /// Like [`Self::new_with_env`], with an explicit per-tool-call timeout.
    pub async fn new_with_env_and_tool_timeout(
        command: impl Into<String>,
        args: Vec<String>,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        stderr_verbose: bool,
        tool_timeout: Duration,
    ) -> Result<Self, McpSessionError> {
        let session = McpSession::new(command, args, Some(env), stderr_verbose).await?;
        Ok(Self {
            client: McpClient::Stdio(session),
            tool_timeout,
        })
    }

    /// Like [`Self::new_http`], with an explicit per-tool-call timeout.
    pub async fn new_http_with_tool_timeout(
        url: impl Into<String>,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        tool_timeout: Duration,
    ) -> Result<Self, ToolSourceError> {
        let session = McpHttpSession::new(url, headers).await?;
        Ok(Self {
            client: McpClient::Http(session),
            tool_timeout,
        })
    }

    /// Lists tools by calling rmcp `list_tools` and mapping result to `Vec<ToolSpec>`.
    pub async fn list_tools_async(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        let tools = match &self.client {
            McpClient::Stdio(s) => s.list_tools().await?,
            McpClient::Http(s) => s.list_tools().await?,
        };
        Ok(tools
            .into_iter()
            .map(|t| {
                let schema = serde_json::to_value(&t.input_schema).unwrap_or_default();
                ToolSpec {
                    name: t.name.to_string(),
                    description: t.description.map(|s| s.to_string()),
                    input_schema: schema,
                    output_hint: Some(ToolOutputHint::preferred(
                        ToolOutputStrategy::FileRefWithExcerpt,
                    )),
                }
            })
            .collect())
    }

    /// Calls a tool via rmcp `call_tool` and extracts text from content.
    pub async fn call_tool_async(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let call = async {
            match &self.client {
                McpClient::Stdio(s) => s.call_tool(name, arguments).await,
                McpClient::Http(s) => s.call_tool(name, arguments).await,
            }
        };
        let result = tokio::time::timeout(self.tool_timeout, call)
            .await
            .map_err(|_| {
                ToolSourceError::Transport(format!(
                    "MCP tool `{name}` timed out after {} seconds",
                    self.tool_timeout.as_secs_f64()
                ))
            })??;
        if result.is_error.unwrap_or(false) {
            let msg = extract_text(&result.content);
            return Err(ToolSourceError::Transport(
                if msg.is_empty() {
                    "tool returned error"
                } else {
                    &msg
                }
                .to_string(),
            ));
        }
        let text = extract_text(&result.content);
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(ToolSourceError::Transport(
                "no text in tools/call response".into(),
            ));
        }
        Ok(ToolCallContent::text(text))
    }
}

/// Extracts text from rmcp content blocks.
fn extract_text(content: &[rmcp::model::Content]) -> String {
    content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_tool_source_new_invalid_command_returns_error() {
        let result = McpToolSource::new(
            "_nonexistent_command_that_does_not_exist_xyz_",
            vec![],
            false,
        )
        .await;
        assert!(result.is_err(), "expected Err for nonexistent command");
    }
}
