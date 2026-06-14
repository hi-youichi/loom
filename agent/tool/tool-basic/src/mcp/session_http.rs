//! MCP session over Streamable HTTP using rmcp client.
//!
//! Used when the server URL is http(s) so tools use HTTP directly.
//! rmcp handles initialize handshake, session management, and SSE parsing.

use std::collections::HashMap;

use rmcp::{
    ServiceExt,
    transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig},
};
use serde_json::Value;

use tool_core::ToolSourceError;

/// MCP session over Streamable HTTP.
///
/// Performs initialize handshake via rmcp, then supports `list_tools`
/// and `call_tool`. Uses rmcp's built-in reqwest backend.
pub struct McpHttpSession {
    client: rmcp::service::RunningService<rmcp::service::RoleClient, ()>,
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
        let mut header_map = HashMap::new();
        for (k, v) in headers {
            let name = reqwest::header::HeaderName::from_bytes(k.into().as_bytes())
                .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
            let value = reqwest::header::HeaderValue::from_str(&v.into())
                .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
            header_map.insert(name, value);
        }
        let config = StreamableHttpClientTransportConfig::with_uri(url)
            .custom_headers(header_map);
        let transport = StreamableHttpClientTransport::from_config(config);
        let client = ()
            .serve(transport)
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))?;
        Ok(Self { client })
    }

    /// Lists tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<rmcp::model::Tool>, ToolSourceError> {
        self.client
            .list_tools(Default::default())
            .await
            .map(|r| r.tools)
            .map_err(|e| ToolSourceError::Transport(e.to_string()))
    }

    /// Calls a tool on the MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, ToolSourceError> {
        let arguments = arguments.as_object().cloned();
        self.client
            .call_tool(rmcp::model::CallToolRequestParams {
                name: name.to_string().into(),
                arguments,
                meta: Default::default(),
                task: None,
            })
            .await
            .map_err(|e| ToolSourceError::Transport(e.to_string()))
    }
}
