//! Message types for TUI session display.
//!
//! This module defines various message types that can appear in a session,
//! including text, thinking blocks, tool calls, and tool results.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for a message
pub type MessageId = String;

/// Represents a message in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique identifier for this message
    pub id: MessageId,
    /// Role of the message sender
    pub role: MessageRole,
    /// Content of the message
    pub content: MessageContent,
    /// Timestamp when the message was created
    pub timestamp: DateTime<Utc>,
    /// Whether this message is collapsed (for UI display)
    pub collapsed: bool,
}

/// Role of the message sender
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    /// Message from the user
    User,
    /// Message from the assistant
    Assistant,
    /// System message
    System,
}

/// Content of a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Plain text message
    Text(String),
    /// Thinking/reasoning block
    Thinking {
        /// The thinking content
        content: String,
        /// Whether the thinking is complete
        complete: bool,
    },
    /// Tool call request
    ToolCall {
        /// Tool call ID
        id: String,
        /// Name of the tool being called
        name: String,
        /// Arguments passed to the tool
        arguments: String,
        /// Status of the tool call
        status: ToolCallStatus,
    },
    /// Tool call result
    ToolResult {
        /// Tool call ID this result corresponds to
        tool_call_id: String,
        /// The result content
        content: String,
        /// Whether the tool call was successful
        success: bool,
    },
    /// Composite message with multiple content blocks
    Composite(Vec<MessageContentBlock>),
}

/// A single block in a composite message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContentBlock {
    /// The content type
    pub content_type: ContentBlockType,
    /// The actual content
    pub content: String,
    /// Additional metadata
    pub metadata: Option<serde_json::Value>,
}

/// Type of content block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentBlockType {
    /// Plain text
    Text,
    /// Thinking/reasoning
    Thinking,
    /// Code snippet
    Code { language: Option<String> },
    /// Tool call
    ToolCall { name: String },
    /// Tool result
    ToolResult,
}

/// Status of a tool call
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallStatus {
    /// Tool call is pending execution
    Pending,
    /// Tool is currently executing
    Executing,
    /// Tool call completed successfully
    Success,
    /// Tool call failed with an error
    Error,
}

impl Message {
    /// Create a new text message
    pub fn text(role: MessageRole, content: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role,
            content: MessageContent::Text(content),
            timestamp: Utc::now(),
            collapsed: false,
        }
    }

    /// Create a new thinking message
    pub fn thinking(content: String, complete: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: MessageContent::Thinking { content, complete },
            timestamp: Utc::now(),
            collapsed: false,
        }
    }

    /// Create a new tool call message
    pub fn tool_call(id: String, name: String, arguments: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: MessageContent::ToolCall {
                id,
                name,
                arguments,
                status: ToolCallStatus::Pending,
            },
            timestamp: Utc::now(),
            collapsed: false,
        }
    }

    /// Create a new tool result message
    pub fn tool_result(tool_call_id: String, content: String, success: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: MessageContent::ToolResult {
                tool_call_id,
                content,
                success,
            },
            timestamp: Utc::now(),
            collapsed: false,
        }
    }

    /// Check if this message is collapsible
    pub fn is_collapsible(&self) -> bool {
        matches!(
            self.content,
            MessageContent::Thinking { .. }
                | MessageContent::ToolCall { .. }
                | MessageContent::ToolResult { .. }
        )
    }

    /// Toggle collapse state
    pub fn toggle_collapse(&mut self) {
        if self.is_collapsible() {
            self.collapsed = !self.collapsed;
        }
    }

    /// Get a short preview of the message content
    pub fn preview(&self, max_len: usize) -> String {
        let content = match &self.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Thinking { content, .. } => format!("💭 {}", content),
            MessageContent::ToolCall { name, .. } => format!("🔧 Calling {}...", name),
            MessageContent::ToolResult { content, success, .. } => {
                if *success {
                    format!("✅ {}", content)
                } else {
                    format!("❌ {}", content)
                }
            }
            MessageContent::Composite(blocks) => {
                let texts: Vec<_> = blocks
                    .iter()
                    .filter_map(|b| {
                        if matches!(b.content_type, ContentBlockType::Text) {
                            Some(b.content.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                texts.join(" ")
            }
        };

        if content.len() > max_len {
            format!("{}...", &content[..max_len.saturating_sub(3)])
        } else {
            content
        }
    }
}

impl MessageRole {
    /// Get display name for the role
    pub fn display_name(&self) -> &'static str {
        match self {
            MessageRole::User => "You",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        }
    }

    /// Get icon for the role
    pub fn icon(&self) -> &'static str {
        match self {
            MessageRole::User => "👤",
            MessageRole::Assistant => "🤖",
            MessageRole::System => "⚙️",
        }
    }
}

impl ToolCallStatus {
    /// Get display icon for the status
    pub fn icon(&self) -> &'static str {
        match self {
            ToolCallStatus::Pending => "⏳",
            ToolCallStatus::Executing => "🔄",
            ToolCallStatus::Success => "✅",
            ToolCallStatus::Error => "❌",
        }
    }

    /// Get display text for the status
    pub fn display_text(&self) -> &'static str {
        match self {
            ToolCallStatus::Pending => "Pending",
            ToolCallStatus::Executing => "Executing",
            ToolCallStatus::Success => "Success",
            ToolCallStatus::Error => "Error",
        }
    }
}
