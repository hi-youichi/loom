//! MCP tool adapter: wraps each MCP tool as `dyn Tool` for a single registry.
//!
//! Each MCP tool is represented by an `McpToolAdapter` that implements `Tool`;
//! `call` delegates to the shared `McpToolSource`. Use `register_mcp_tools`
//! to list MCP tools and register one adapter per tool into an `AggregateToolSource`.

use std::sync::Arc;

use async_trait::async_trait;

use crate::tool_source::McpToolSource;
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSource, ToolSourceError, ToolSpec};
use crate::tools::Tool;

/// Adapter that makes one MCP tool implement the `Tool` trait.
///
/// Holds the tool name, cached spec from MCP `tools/list`, and a shared
/// `Arc<McpToolSource>` so `call` can delegate to `tools/call`. Used to put
/// MCP tools into the same `ToolRegistry` as local tools (e.g. memory tools).
///
/// **Interaction**: Created by `register_mcp_tools`; registered with
/// `AggregateToolSource::register_sync`. Implements `Tool`; `call` ignores
/// `ToolCallContext` and forwards to MCP.
pub struct McpToolAdapter {
    name: String,
    spec: ToolSpec,
    source: Arc<McpToolSource>,
}

impl McpToolAdapter {
    /// Creates an adapter for one MCP tool.
    ///
    /// **Interaction**: Used by `register_mcp_tools`; not typically called directly.
    pub fn new(name: String, spec: ToolSpec, source: Arc<McpToolSource>) -> Self {
        Self { name, spec, source }
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        self.source.call_tool(self.name.as_str(), args).await
    }
}

/// Registers all tools from the MCP server into the given aggregate.
///
/// Calls `mcp.list_tools().await`, then for each tool creates an `McpToolAdapter`
/// and registers it with `aggregate.register_sync`. Use when building a single
/// tool set that includes both local tools (e.g. memory) and MCP tools (e.g. Exa).
///
/// **Interaction**: Call after registering local tools (if any). Requires
/// `exa_api_key` (or equivalent) to have been used to create `mcp`; do not call
/// when MCP is not configured.
pub async fn register_mcp_tools(
    aggregate: &super::AggregateToolSource,
    mcp: Arc<McpToolSource>,
) -> Result<(), ToolSourceError> {
    let specs = mcp.list_tools().await?;
    for spec in specs {
        let name = spec.name.clone();
        let adapter = McpToolAdapter::new(name, spec, Arc::clone(&mcp));
        aggregate.register_async(Box::new(adapter)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use crate::tool_source::ToolSource;
    use crate::tools::AggregateToolSource;

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = stream.read(&mut tmp).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = pos + 4;
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
                return String::from_utf8_lossy(&body[..content_length]).to_string();
            }
        }
        String::new()
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: &str,
        content_type: Option<&str>,
        body: &str,
    ) {
        let mut resp = format!("HTTP/1.1 {}\r\nConnection: close\r\n", status);
        if let Some(ct) = content_type {
            resp.push_str(&format!("Content-Type: {}\r\n", ct));
        }
        resp.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
        stream.write_all(resp.as_bytes()).await.unwrap();
    }

    #[tokio::test]
    async fn register_mcp_tools_adds_adapters_and_can_call_registered_tool() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let body = read_http_request(&mut stream).await;
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                let method = json.get("method").and_then(|m| m.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-mcp-initialize",
                            "result":{"protocolVersion":"2025-11-25"}
                        })
                        .to_string();
                        write_http_response(&mut stream, "200 OK", Some("application/json"), &body).await;
                    }
                    "notifications/initialized" => {
                        write_http_response(&mut stream, "202 Accepted", None, "").await;
                    }
                    "tools/list" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-tools-list",
                            "result":{"tools":[{"name":"demo_mcp","description":"demo tool","inputSchema":{"type":"object"}}]}
                        })
                        .to_string();
                        write_http_response(&mut stream, "200 OK", Some("application/json"), &body).await;
                    }
                    "tools/call" => {
                        let body = serde_json::json!({
                            "jsonrpc":"2.0",
                            "id":"loom-call-demo_mcp",
                            "result":{"content":[{"type":"text","text":"adapter-ok"}]}
                        })
                        .to_string();
                        write_http_response(&mut stream, "200 OK", Some("application/json"), &body).await;
                    }
                    _ => panic!("unexpected method: {}", method),
                }
            }
        });

        let mcp = Arc::new(
            McpToolSource::new_http(
                format!("http://{}", addr),
                std::iter::empty::<(String, String)>(),
            )
            .await
            .unwrap(),
        );
        let aggregate = AggregateToolSource::new();
        register_mcp_tools(&aggregate, Arc::clone(&mcp))
            .await
            .unwrap();

        let tools = aggregate.list_tools().await.unwrap();
        assert!(tools.iter().any(|t| t.name == "demo_mcp"));

        let out = aggregate
            .call_tool("demo_mcp", serde_json::json!({"x":1}))
            .await
            .unwrap();
        assert_eq!(out.text, "adapter-ok");

        let first_spec = tools.into_iter().find(|t| t.name == "demo_mcp").unwrap();
        let adapter = McpToolAdapter::new("demo_mcp".to_string(), first_spec.clone(), mcp);
        assert_eq!(adapter.name(), "demo_mcp");
        assert_eq!(adapter.spec().name, "demo_mcp");

        server.await.unwrap();
    }
}
