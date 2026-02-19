//! Twitter advanced search tool via twitterapi.io.
//!
//! Calls the Twitter Advanced Search API to search tweets by query.
//! Uses [`reqwest::Client`] for HTTP. Interacts with [`Tool`](crate::tools::Tool).
//! API: https://docs.twitterapi.io/api-reference/endpoint/tweet_advanced_search
//!
//! Requires `TWITTER_API_KEY` in environment or passed via constructor.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for Twitter search.
pub const TOOL_TWITTER_SEARCH: &str = "twitter_search";

const TWITTER_API_BASE: &str = "https://api.twitterapi.io";

/// Tool that searches Twitter tweets via twitterapi.io Advanced Search API.
///
/// Accepts `query` (required), optional `query_type` (Latest or Top, default Latest),
/// optional `cursor` for pagination. Returns up to 20 tweets per page.
pub struct TwitterSearchTool {
    /// API key for twitterapi.io (x-api-key header).
    api_key: Arc<str>,
    /// HTTP client for requests.
    client: reqwest::Client,
}

impl TwitterSearchTool {
    /// Creates a new TwitterSearchTool with the given API key.
    ///
    /// # Examples
    ///
    /// ```
    /// use loom::tools::twitter::TwitterSearchTool;
    ///
    /// let tool = TwitterSearchTool::new("your_api_key");
    /// ```
    pub fn new(api_key: impl Into<Arc<str>>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Creates a new TwitterSearchTool with a custom HTTP client.
    ///
    /// # Parameters
    ///
    /// - `api_key`: API key for twitterapi.io.
    /// - `client`: Custom reqwest::Client for configuring timeouts, etc.
    pub fn with_client(api_key: impl Into<Arc<str>>, client: reqwest::Client) -> Self {
        Self {
            api_key: api_key.into(),
            client,
        }
    }
}

#[async_trait]
impl Tool for TwitterSearchTool {
    fn name(&self) -> &str {
        TOOL_TWITTER_SEARCH
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_TWITTER_SEARCH.to_string(),
            description: Some(
                "Search Twitter tweets via advanced search. Returns JSON with tweets (up to 20/page), \
                 has_next_page, next_cursor. Query syntax: keywords, from:user, since:YYYY-MM-DD, \
                 filter:images, lang:en, min_faves:N, #hashtag. Use next_cursor for pagination.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query. Examples: AI, from:elonmusk, lang:en since:2024-01-01"
                    },
                    "query_type": {
                        "type": "string",
                        "description": "Sort: Latest (recent) or Top (by engagement). Default Latest.",
                        "enum": ["Latest", "Top"]
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Pagination cursor from next_cursor. Omit for first page."
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("missing 'query'".to_string()))?;

        if query.trim().is_empty() {
            return Err(ToolSourceError::InvalidInput(
                "query cannot be empty".to_string(),
            ));
        }

        let query_type = args
            .get("query_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Latest");

        if query_type != "Latest" && query_type != "Top" {
            return Err(ToolSourceError::InvalidInput(
                "query_type must be 'Latest' or 'Top'".to_string(),
            ));
        }

        let cursor = args.get("cursor").and_then(|v| v.as_str()).unwrap_or("");

        let url = format!("{}/twitter/tweet/advanced_search", TWITTER_API_BASE);
        let mut req = self.client.get(&url).header("x-api-key", self.api_key.as_ref());

        let params: Vec<(&str, &str)> = vec![
            ("query", query),
            ("queryType", query_type),
            ("cursor", cursor),
        ];
        req = req.query(&params);

        let response = req
            .send()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "".to_string());
            return Err(ToolSourceError::Transport(format!(
                "API error {}: {}",
                status, body
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| ToolSourceError::Transport(format!("failed to read response: {}", e)))?;

        Ok(ToolCallContent { text: body })
    }
}
