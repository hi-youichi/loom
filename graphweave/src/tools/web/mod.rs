use async_trait::async_trait;

use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for the web fetcher operation.
pub const TOOL_WEB_FETCHER: &str = "web_fetcher";

/// Tool for HTTP requests to URLs (GET or POST).
///
/// Wraps reqwest::Client and exposes it as a tool for the LLM.
/// Supports GET (default) and POST with optional body and headers.
///
/// # Examples
///
/// ```no_run
/// use graphweave::tools::{Tool, WebFetcherTool};
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() {
/// let tool = WebFetcherTool::new();
///
/// // GET (default)
/// let args = json!({ "url": "https://example.com/api/data" });
/// let result = tool.call(args, None).await.unwrap();
/// assert!(!result.text.is_empty());
///
/// // POST with JSON body
/// let args = json!({
///     "url": "https://example.com/api",
///     "method": "POST",
///     "body": { "key": "value" }
/// });
/// let result = tool.call(args, None).await.unwrap();
/// # }
/// ```
///
/// # Interaction
///
/// - **reqwest::Client**: Performs HTTP GET or POST
/// - **ToolRegistry**: Registers this tool by name "web_fetcher"
/// - **AggregateToolSource**: Uses this tool via ToolRegistry
/// - **ToolSourceError**: Maps HTTP errors to tool error types
pub struct WebFetcherTool {
    client: reqwest::Client,
}

impl Default for WebFetcherTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetcherTool {
    /// Creates a new WebFetcherTool with a default HTTP client.
    ///
    /// Uses reqwest::Client::new() to create a client with default settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::web::WebFetcherTool;
    ///
    /// let tool = WebFetcherTool::new();
    /// ```
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Creates a new WebFetcherTool with a custom HTTP client.
    ///
    /// # Parameters
    ///
    /// - `client`: Custom reqwest::Client for configuring timeouts, proxies, etc.
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::web::WebFetcherTool;
    /// use std::time::Duration;
    ///
    /// let client = reqwest::Client::builder()
    ///     .timeout(Duration::from_secs(30))
    ///     .build()
    ///     .unwrap();
    /// let tool = WebFetcherTool::with_client(client);
    /// ```
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for WebFetcherTool {
    /// Returns the unique name of this tool.
    ///
    /// Returns "web_fetcher" as the tool identifier.
    fn name(&self) -> &str {
        TOOL_WEB_FETCHER
    }

    /// Returns the specification for this tool.
    ///
    /// Includes tool name, description (for the LLM), and JSON schema for arguments.
    /// The spec describes the required "url" parameter.
    ///
    /// # Interaction
    ///
    /// - Called by ToolRegistry::list() to build Vec<ToolSpec>
    /// - Spec fields are aligned with MCP tools/list result
    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_WEB_FETCHER.to_string(),
            description: Some(
                "Fetch or send content to a URL. Use this tool to retrieve web pages (GET), call \
                 APIs with a body (POST), or other HTTP-accessible content. Optional: method (default \
                 GET), body (string or JSON object), headers (object). Returns the response body as text.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to request. Must be a valid HTTP/HTTPS URL."
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method. One of: GET, POST. Default is GET.",
                        "enum": ["GET", "POST"]
                    },
                    "body": {
                        "description": "Request body for POST. May be a string (sent as-is with Content-Type: text/plain) or a JSON object (sent as application/json)."
                    },
                    "headers": {
                        "type": "object",
                        "description": "Optional HTTP headers as key-value pairs (string keys and values).",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["url"]
            }),
        }
    }

    /// Executes the tool by performing an HTTP request to the specified URL.
    ///
    /// # Parameters
    ///
    /// - `args`: JSON with required "url"; optional "method" (GET|POST), "body", "headers"
    /// - `_ctx`: Optional per-call context (not used by this tool)
    ///
    /// # Returns
    ///
    /// The HTTP response body as text content.
    ///
    /// # Errors
    ///
    /// Returns ToolSourceError for:
    /// - Missing or invalid "url" (InvalidInput)
    /// - Unsupported "method" (InvalidInput)
    /// - HTTP request failures (Transport)
    /// - Non-success HTTP status codes (Transport)
    /// - Response read failures (Transport)
    ///
    /// # Interaction
    ///
    /// - Called by ToolRegistry::call(); uses reqwest::Client for GET or POST
    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing url".to_string()))?;

        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();
        if method != "GET" && method != "POST" {
            return Err(ToolSourceError::InvalidInput(format!(
                "unsupported method: {} (use GET or POST)",
                method
            )));
        }

        let mut request = match method.as_str() {
            "GET" => self.client.get(url),
            _ => self.client.post(url),
        };

        if let Some(h) = args.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in h {
                if let Some(v_str) = v.as_str() {
                    request = request.header(k.as_str(), v_str);
                }
            }
        }

        if method == "POST" {
            if let Some(body) = args.get("body") {
                if body.is_object() {
                    request = request.json(body);
                } else if let Some(s) = body.as_str() {
                    request = request
                        .body(s.to_string())
                        .header("Content-Type", "text/plain; charset=utf-8");
                } else if !body.is_null() {
                    request = request.json(body);
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolSourceError::Transport(format!(
                "request failed with status: {}",
                response.status()
            )));
        }

        let content = response
            .text()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("failed to read response: {}", e)))?;

        Ok(ToolCallContent { text: content })
    }
}
