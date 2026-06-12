//! Twitter advanced search tool via twitterapi.io.
//!
//! Calls the Twitter Advanced Search API to search tweets by query.
//! Uses [`reqwest::Client`] for HTTP. Interacts with [`Tool`](tool_core::Tool).
//! API: https://docs.twitterapi.io/api-reference/endpoint/tweet_advanced_search
//!
//! Requires `TWITTER_API_KEY` in environment or passed via constructor.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use tool_core::{ToolCallContent, ToolCallContext, ToolSourceError, ToolOutputHint, ToolOutputStrategy};
use tool_core::Tool;

fn is_retryable_reqwest_error(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request()
}

fn retry_backoff_for_attempt(attempt: u32) -> tokio::time::Duration {
    tokio::time::Duration::from_millis(100 * (1 << attempt.min(6)))
}

const TRANSIENT_HTTP_MAX_RETRIES: u32 = 3;

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
    /// use tool_extensions::twitter::TwitterSearchTool;
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

    fn spec(&self) -> tool_core::ToolSpec {
        tool_core::ToolSpec {
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
            output_hint: Some(ToolOutputHint::preferred(
                ToolOutputStrategy::FileRefWithExcerpt,
            )),
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
        let mut req = self
            .client
            .get(&url)
            .header("x-api-key", self.api_key.as_ref());

        let params: Vec<(&str, &str)> = vec![
            ("query", query),
            ("queryType", query_type),
            ("cursor", cursor),
        ];
        req = req.query(&params);

        let mut attempt = 0;
        loop {
            let request = req.try_clone().ok_or_else(|| {
                ToolSourceError::Transport("failed to clone Twitter request for retry".to_string())
            })?;
            let response = match request.send().await {
                Ok(response) => response,
                Err(e)
                    if is_retryable_reqwest_error(&e) && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "Twitter search request failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                Err(e) => {
                    return Err(ToolSourceError::Transport(format!("request failed: {}", e)));
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_else(|_| "".to_string());
                return Err(ToolSourceError::Transport(format!(
                    "API error {}: {}",
                    status, body
                )));
            }

            match response.text().await {
                Ok(body) => return Ok(ToolCallContent::text(body)),
                Err(e)
                    if is_retryable_reqwest_error(&e) && attempt < TRANSIENT_HTTP_MAX_RETRIES =>
                {
                    let delay = retry_backoff_for_attempt(attempt);
                    tracing::warn!(
                        url = %url,
                        attempt = attempt + 1,
                        max_retries = TRANSIENT_HTTP_MAX_RETRIES,
                        delay_secs = delay.as_secs_f64(),
                        error = %e,
                        "Twitter search response read failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    return Err(ToolSourceError::Transport(format!(
                        "failed to read response: {}",
                        e
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()
            .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn twitter_search_tool_name_returns_twitter_search() {
        let tool = TwitterSearchTool::with_client("test_key", test_client());
        assert_eq!(tool.name(), TOOL_TWITTER_SEARCH);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn twitter_search_tool_spec_has_correct_properties() {
        let tool = TwitterSearchTool::with_client("test_key", test_client());
        let spec = tool.spec();
        assert_eq!(spec.name, TOOL_TWITTER_SEARCH);
        assert!(spec.description.is_some());
        let desc = spec.description.unwrap();
        let desc_lower = desc.to_lowercase();
        assert!(desc_lower.contains("search") && desc_lower.contains("query"));
        assert_eq!(spec.input_schema["properties"]["query"]["type"], "string");
        assert_eq!(
            spec.input_schema["properties"]["query_type"]["enum"],
            json!(["Latest", "Top"])
        );
        assert!(spec.input_schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("query")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn twitter_search_tool_call_missing_query_returns_error() {
        let tool = TwitterSearchTool::with_client("test_key", test_client());
        let args = json!({});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("query") || err.to_string().contains("InvalidInput"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn twitter_search_tool_call_empty_query_returns_error() {
        let tool = TwitterSearchTool::with_client("test_key", test_client());
        let args = json!({"query": "   "});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty") || err.to_string().contains("InvalidInput"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn twitter_search_tool_call_invalid_query_type_returns_error() {
        let tool = TwitterSearchTool::with_client("test_key", test_client());
        let args = json!({"query": "AI", "query_type": "Invalid"});
        let result = tool.call(args, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Latest") || err.to_string().contains("InvalidInput"));
    }
}