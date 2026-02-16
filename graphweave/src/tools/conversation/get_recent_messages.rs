use async_trait::async_trait;

use serde_json::{json, Value};

use crate::message::Message;
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;

/// Tool name for the get_recent_messages operation.
pub const TOOL_GET_RECENT_MESSAGES: &str = "get_recent_messages";

/// Tool for getting recent messages from current conversation.
///
/// Uses ToolCallContext (injected by ActNode via set_call_context) to return
/// the last N messages. This is for short-term memory access during tool execution.
///
/// # Examples
///
/// ```no_run
/// # #[tokio::main]
/// # async fn main() {
/// use graphweave::tools::{GetRecentMessagesTool, Tool};
/// use graphweave::message::Message;
/// use graphweave::tool_source::ToolCallContext;
/// use serde_json::json;
///
/// let tool = GetRecentMessagesTool;
///
/// let context = ToolCallContext::new(vec![
///     Message::User("hello".to_string()),
///     Message::Assistant("hi there!".to_string()),
/// ]);
///
/// let args = json!({"limit": 2});
/// let result = tool.call(args, Some(&context)).await.unwrap();
/// assert!(result.text.contains("hello"));
/// # }
/// ```
///
/// # Interaction
///
/// - **ToolCallContext**: Provides recent messages via context.recent_messages
/// - **ActNode**: Injects context via set_call_context before tool execution
/// - **ToolRegistry**: Registers this tool by name "get_recent_messages"
/// - **ShortTermMemoryToolSource**: Uses this tool via AggregateToolSource
pub struct GetRecentMessagesTool;

impl GetRecentMessagesTool {
    /// Creates a new GetRecentMessagesTool.
    ///
    /// This tool is stateless; the context is passed via ToolCallContext.
    ///
    /// # Examples
    ///
    /// ```
    /// use graphweave::tools::GetRecentMessagesTool;
    ///
    /// let tool = GetRecentMessagesTool;
    /// ```
    pub fn new() -> Self {
        Self
    }

    /// Converts a Message to a JSON value with role and content.
    fn message_to_json(m: &Message) -> Value {
        let (role, content) = match m {
            Message::System(s) => ("system", s.as_str()),
            Message::User(s) => ("user", s.as_str()),
            Message::Assistant(s) => ("assistant", s.as_str()),
        };
        json!({ "role": role, "content": content })
    }
}

impl Default for GetRecentMessagesTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GetRecentMessagesTool {
    fn name(&self) -> &str {
        TOOL_GET_RECENT_MESSAGES
    }

    fn spec(&self) -> crate::tool_source::ToolSpec {
        crate::tool_source::ToolSpec {
            name: TOOL_GET_RECENT_MESSAGES.to_string(),
            description: Some(
                "(Optional) Get last N messages from current conversation. Use only when you need \
                 to explicitly re-read or summarize recent turns (e.g. when prompt does not include full history). \
                 Most ReAct flows can omit this tool.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max number of messages to return (optional)" }
                }
            }),
        }
    }

    async fn call(
        &self,
        args: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        let messages_vec: Vec<Message> = match ctx {
            Some(c) => c.recent_messages.clone(),
            None => vec![],
        };

        let messages = messages_vec.as_slice();
        let take = limit.unwrap_or(messages.len());
        let start = messages.len().saturating_sub(take);
        let slice = &messages[start..];

        let arr: Vec<Value> = slice.iter().map(Self::message_to_json).collect();
        let text = serde_json::to_string(&arr)
            .map_err(|e| ToolSourceError::InvalidInput(e.to_string()))?;

        Ok(ToolCallContent { text })
    }
}
