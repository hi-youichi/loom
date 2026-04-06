use async_trait::async_trait;

use serde_json::{json, Value};

use crate::message::Message;
use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError};
use crate::tools::Tool;
use crate::{ToolOutputHint, ToolOutputStrategy};

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
/// use loom::tools::{GetRecentMessagesTool, Tool};
/// use loom::message::Message;
/// use loom::tool_source::ToolCallContext;
/// use serde_json::json;
///
/// let tool = GetRecentMessagesTool;
///
/// let context = ToolCallContext::new(vec![
///     Message::User(loom::message::UserContent::Text("hello".to_string())),
///     Message::assistant("hi there!"),
/// ]);
///
/// let args = json!({"limit": 2});
/// let result = tool.call(args, Some(&context)).await.unwrap();
/// assert!(result.as_text().unwrap().contains("hello"));
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
    /// use loom::tools::GetRecentMessagesTool;
    ///
    /// let tool = GetRecentMessagesTool;
    /// ```
    pub fn new() -> Self {
        Self
    }

    /// Converts a Message to a JSON value with role and content.
    fn message_to_json(m: &Message) -> Value {
        match m {
            Message::System(s) => json!({ "role": "system", "content": s }),
            Message::User(s) => json!({ "role": "user", "content": s }),
            Message::Assistant(p) if p.tool_calls.is_empty() => {
                json!({ "role": "assistant", "content": p.content })
            }
            Message::Assistant(p) => json!({
                "role": "assistant",
                "content": p.content,
                "tool_calls": p.tool_calls,
            }),
            Message::Tool {
                tool_call_id,
                content,
            } => json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": content,
            }),
        }
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
            output_hint: Some(
                ToolOutputHint::preferred(ToolOutputStrategy::SummaryOnly).safe_inline_chars(1_000),
            ),
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

        Ok(ToolCallContent::text(text))
    }
}
