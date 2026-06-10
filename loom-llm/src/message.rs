//! Minimal message types for agent state.
//!
//! Message roles: System, User, Assistant, and Tool (tool outputs for strict chat APIs).
//! Used by `AgentState::messages` and by agents that read/append messages in `Agent::run`.

use std::borrow::Cow;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// User message content: plain text or multimodal part array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Multimodal(Vec<ContentPart>),
}

/// One content part in a multimodal user message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    ImageUrl {
        url: String,
        detail: Option<String>,
    },
    ImageBase64 {
        media_type: String,
        data: String,
    },
    AudioBase64 {
        media_type: String,
        data: String,
    },
    VideoUrl {
        url: String,
    },
    VideoBase64 {
        media_type: String,
        data: String,
    },
    PdfUrl {
        url: String,
    },
    PdfBase64 {
        data: String,
    },
    File {
        file_id: Option<String>,
        file_data: Option<String>,
        filename: Option<String>,
    },
}

/// Content block or message error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    #[error("empty content parts")]
    EmptyMessage,
}

impl UserContent {
    pub fn as_text(&self) -> Cow<'_, str> {
        match self {
            UserContent::Text(s) => Cow::Borrowed(s),
            UserContent::Multimodal(parts) => {
                let texts: Vec<_> = parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                Cow::Owned(texts.join("\n"))
            }
        }
    }

    pub fn contains(&self, pattern: &str) -> bool {
        self.as_text().contains(pattern)
    }

    pub fn starts_with(&self, pattern: &str) -> bool {
        self.as_text().starts_with(pattern)
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn multimodal(parts: Vec<ContentPart>) -> Result<Self, ContentError> {
        if parts.is_empty() {
            return Err(ContentError::EmptyMessage);
        }
        Ok(Self::Multimodal(parts))
    }

    /// Returns the list of modalities used in this content.
    /// Note: Requires model_spec_core - use a wrapper in loom that calls this.
    pub fn modalities_fallback(&self) -> Vec<String> {
        match self {
            UserContent::Text(_) => vec!["Text".to_string()],
            UserContent::Multimodal(parts) => parts.iter().map(|p| p.modality_string()).collect(),
        }
    }
}

impl From<String> for UserContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for UserContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl std::fmt::Display for UserContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_text())
    }
}

impl From<UserContent> for String {
    fn from(c: UserContent) -> Self {
        c.as_text().into_owned()
    }
}

impl PartialEq<&str> for UserContent {
    fn eq(&self, other: &&str) -> bool {
        self.as_text().as_ref() == *other
    }
}

impl PartialEq<String> for UserContent {
    fn eq(&self, other: &String) -> bool {
        self.as_text().as_ref() == other
    }
}

impl ContentPart {
    /// Returns modality as string (without model_spec_core dependency).
    pub fn modality_string(&self) -> String {
        match self {
            ContentPart::Text { .. } => "Text".to_string(),
            ContentPart::ImageUrl { .. } | ContentPart::ImageBase64 { .. } => "Image".to_string(),
            ContentPart::AudioBase64 { .. } => "Audio".to_string(),
            ContentPart::VideoUrl { .. } | ContentPart::VideoBase64 { .. } => "Video".to_string(),
            ContentPart::PdfUrl { .. } | ContentPart::PdfBase64 { .. } => "Pdf".to_string(),
            ContentPart::File { .. } => "Text".to_string(),
        }
    }
}

/// One function tool call the model requested (aligned with OpenAI `tool_calls[]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Assistant turn: optional visible text plus optional parallel tool calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantPayload {
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<AssistantToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

mod assistant_payload_serde {
    use super::{AssistantPayload, AssistantToolCall};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AssistantSerde {
        Legacy(String),
        Structured {
            content: String,
            #[serde(default)]
            tool_calls: Vec<AssistantToolCall>,
            #[serde(default)]
            reasoning_content: Option<String>,
        },
    }

    #[derive(Serialize)]
    struct AssistantStruct<'a> {
        content: &'a str,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: &'a Vec<AssistantToolCall>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: &'a Option<String>,
    }

    pub fn serialize<S>(payload: &AssistantPayload, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if payload.tool_calls.is_empty() && payload.reasoning_content.is_none() {
            payload.content.serialize(serializer)
        } else {
            AssistantStruct {
                content: payload.content.as_str(),
                tool_calls: &payload.tool_calls,
                reasoning_content: &payload.reasoning_content,
            }
            .serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AssistantPayload, D::Error>
    where
        D: Deserializer<'de>,
    {
        match AssistantSerde::deserialize(deserializer)? {
            AssistantSerde::Legacy(content) => Ok(AssistantPayload {
                content,
                tool_calls: vec![],
                reasoning_content: None,
            }),
            AssistantSerde::Structured {
                content,
                tool_calls,
                reasoning_content,
            } => Ok(AssistantPayload {
                content,
                tool_calls,
                reasoning_content,
            }),
        }
    }
}

/// Tool call content for tool messages (result of tool execution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallContent {
    /// Plain text result.
    Text(String),
    /// File modification shown as a diff.
    Diff {
        /// The file path being modified.
        path: String,
        /// The original content (None for new files).
        old_text: Option<String>,
        /// The new content after modification.
        new_text: String,
    },
    /// Terminal command output with a terminal ID.
    Terminal { terminal_id: String },
}

// Custom Serialize: Text serializes as plain string, Diff/Terminal as objects.
impl Serialize for ToolCallContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ToolCallContent::Text(t) => t.serialize(serializer),
            ToolCallContent::Diff {
                path,
                old_text,
                new_text,
            } => {
                use serde::ser::SerializeStruct;
                let mut s = serializer.serialize_struct("Diff", 4)?;
                s.serialize_field("type", "diff")?;
                s.serialize_field("path", path)?;
                s.serialize_field("old_text", old_text)?;
                s.serialize_field("new_text", new_text)?;
                s.end()
            }
            ToolCallContent::Terminal { terminal_id } => {
                use serde::ser::SerializeStruct;
                let mut s = serializer.serialize_struct("Terminal", 2)?;
                s.serialize_field("type", "terminal")?;
                s.serialize_field("terminal_id", terminal_id)?;
                s.end()
            }
        }
    }
}

// Custom Deserialize: plain string → Text, object with type field → Diff/Terminal.
impl<'de> Deserialize<'de> for ToolCallContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct ToolCallContentVisitor;

        impl<'de> Visitor<'de> for ToolCallContentVisitor {
            type Value = ToolCallContent;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or a structured tool content object")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ToolCallContent::Text(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(ToolCallContent::Text(value))
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut path = None;
                let mut old_text = None;
                let mut new_text = None;
                let mut terminal_id = None;
                let mut content_type = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => content_type = Some(map.next_value()?),
                        "path" => path = Some(map.next_value()?),
                        "old_text" => old_text = Some(map.next_value()?),
                        "new_text" => new_text = Some(map.next_value()?),
                        "terminal_id" => terminal_id = Some(map.next_value()?),
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                let content_type: String =
                    content_type.ok_or_else(|| de::Error::missing_field("type"))?;
                match content_type.as_str() {
                    "terminal" => Ok(ToolCallContent::Terminal {
                        terminal_id: terminal_id
                            .ok_or_else(|| de::Error::missing_field("terminal_id"))?,
                    }),
                    "diff" => Ok(ToolCallContent::Diff {
                        path: path.ok_or_else(|| de::Error::missing_field("path"))?,
                        old_text,
                        new_text: new_text.ok_or_else(|| de::Error::missing_field("new_text"))?,
                    }),
                    other => Err(de::Error::custom(format!(
                        "expected type 'diff' or 'terminal', got '{}'",
                        other
                    ))),
                }
            }
        }

        deserializer.deserialize_any(ToolCallContentVisitor)
    }
}

impl ToolCallContent {
    /// Create a text tool call content.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text(content.into())
    }

    /// Create a diff tool call content.
    pub fn diff(path: impl Into<String>, old_text: Option<String>, new_text: impl Into<String>) -> Self {
        Self::Diff {
            path: path.into(),
            old_text,
            new_text: new_text.into(),
        }
    }

    /// Create a terminal tool call content.
    pub fn terminal(terminal_id: impl Into<String>) -> Self {
        Self::Terminal {
            terminal_id: terminal_id.into(),
        }
    }

    /// Returns the text content if this is a Text variant, None otherwise.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolCallContent::Text(t) => Some(t),
            ToolCallContent::Diff { .. } | ToolCallContent::Terminal { .. } => None,
        }
    }

    /// Returns the display string for this content.
    pub fn to_display_string(&self) -> String {
        match self {
            ToolCallContent::Text(t) => t.clone(),
            ToolCallContent::Diff { path, .. } => {
                format!("Modified file: {}", path)
            }
            ToolCallContent::Terminal { terminal_id } => {
                format!("Terminal: {}", terminal_id)
            }
        }
    }

    /// Returns the approximate byte length of the content text.
    pub fn len(&self) -> usize {
        match self {
            ToolCallContent::Text(s) => s.len(),
            ToolCallContent::Diff { new_text, .. } => new_text.len(),
            ToolCallContent::Terminal { terminal_id } => terminal_id.len(),
        }
    }

    /// Returns true if the content is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Consume and return the text representation.
    pub fn into_text(self) -> String {
        match self {
            ToolCallContent::Text(t) => t,
            ToolCallContent::Diff { path, .. } => {
                format!("Modified file: {}", path)
            }
            ToolCallContent::Terminal { terminal_id } => {
                format!("Terminal: {}", terminal_id)
            }
        }
    }
}

impl From<String> for ToolCallContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for ToolCallContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl std::fmt::Display for ToolCallContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCallContent::Text(t) => write!(f, "{}", t),
            ToolCallContent::Diff { path, .. } => write!(f, "Diff({})", path),
            ToolCallContent::Terminal { terminal_id } => write!(f, "Terminal({})", terminal_id),
        }
    }
}

/// A single message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    /// System prompt; typically placed first in the message list.
    System(String),
    /// User input.
    User(UserContent),
    /// Model reply, optionally including tool calls for the next round.
    #[serde(with = "assistant_payload_serde")]
    Assistant(AssistantPayload),
    /// Tool execution result (OpenAI `role: tool`); pairs with a prior assistant `tool_calls` id.
    Tool {
        tool_call_id: String,
        content: ToolCallContent,
    },
}

impl Message {
    /// Creates a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::System(content.into())
    }

    /// Creates a user message.
    pub fn user(content: impl Into<UserContent>) -> Self {
        Self::User(content.into())
    }

    /// Creates a user message with multimodal content.
    pub fn user_multimodal(parts: Vec<ContentPart>) -> Result<Self, ContentError> {
        Ok(Self::User(UserContent::multimodal(parts)?))
    }

    /// Creates an assistant message with text only (no tool calls).
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant(AssistantPayload {
            content: content.into(),
            tool_calls: vec![],
            reasoning_content: None,
        })
    }

    /// Creates an assistant message that includes tool calls (and optional text).
    pub fn assistant_with_tool_calls(content: String, tool_calls: Vec<AssistantToolCall>) -> Self {
        Self::Assistant(AssistantPayload {
            content,
            tool_calls,
            reasoning_content: None,
        })
    }

    /// Creates an assistant message with text and optional reasoning (no tool calls).
    pub fn assistant_with_reasoning(content: impl Into<String>, reasoning: Option<String>) -> Self {
        Self::Assistant(AssistantPayload {
            content: content.into(),
            tool_calls: vec![],
            reasoning_content: reasoning,
        })
    }

    /// Creates an assistant message that includes tool calls, text, and optional reasoning.
    pub fn assistant_with_tool_calls_and_reasoning(
        content: String,
        tool_calls: Vec<AssistantToolCall>,
        reasoning: Option<String>,
    ) -> Self {
        Self::Assistant(AssistantPayload {
            content,
            tool_calls,
            reasoning_content: reasoning,
        })
    }

    /// Returns the role name as a string.
    pub fn role(&self) -> &'static str {
        match self {
            Message::System(_) => "system",
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
            Message::Tool { .. } => "tool",
        }
    }

    /// Returns the primary text content.
    pub fn content(&self) -> Cow<'_, str> {
        match self {
            Message::System(s) => Cow::Borrowed(s),
            Message::User(c) => c.as_text(),
            Message::Assistant(p) => Cow::Borrowed(p.content.as_str()),
            Message::Tool { content, .. } => Cow::Owned(content.to_display_string()),
        }
    }

    /// Role plus a single `content` string for HTTP/API or SQLite `(role, content)` rows.
    pub fn to_role_content_pair(&self) -> (&'static str, String) {
        match self {
            Message::System(c) => ("system", c.clone()),
            Message::User(c) => ("user", c.as_text().into_owned()),
            Message::Assistant(p) => {
                if p.tool_calls.is_empty() {
                    ("assistant", p.content.clone())
                } else {
                    (
                        "assistant",
                        serde_json::to_string(p).unwrap_or_else(|_| p.content.clone()),
                    )
                }
            }
            Message::Tool {
                tool_call_id,
                content,
            } => (
                "tool",
                serde_json::json!({ "tool_call_id": tool_call_id, "content": content.to_display_string() }).to_string(),
            ),
        }
    }

    /// Like `to_role_content_pair`, but generates a UUID for empty tool_call_id.
    pub fn to_role_content_pair_for_store(&self) -> (&'static str, String) {
        if let Message::Tool {
            tool_call_id,
            content,
        } = self
        {
            if tool_call_id.is_empty() {
                warn!("tool message with empty tool_call_id on persist; generating id");
                let id = format!("call_{}", uuid::Uuid::new_v4());
                return (
                    "tool",
                    serde_json::json!({ "tool_call_id": id, "content": content.to_display_string() }).to_string(),
                );
            }
        }
        self.to_role_content_pair()
    }
}

/// Assistant `content` for chat-completion HTTP requests when the turn has **no** `tool_calls`.
pub fn assistant_content_for_chat_api(s: &str) -> Cow<'_, str> {
    if s.trim().is_empty() {
        Cow::Borrowed("\u{2060}")
    } else {
        Cow::Borrowed(s)
    }
}

/// Convert ContentPart to ModalityType for model validation
pub fn content_part_modality(part: &ContentPart) -> &'static str {
    match part {
        ContentPart::Text { .. } | ContentPart::File { .. } => "text",
        ContentPart::ImageUrl { .. } | ContentPart::ImageBase64 { .. } => "image",
        ContentPart::AudioBase64 { .. } => "audio",
        ContentPart::VideoUrl { .. } | ContentPart::VideoBase64 { .. } => "video",
        ContentPart::PdfUrl { .. } | ContentPart::PdfBase64 { .. } => "pdf",
    }
}

/// Helper function to check for orphan tool calls
pub fn check_orphan_tool_calls(messages: &[Message]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut expected_tool_results = std::collections::HashSet::new();
    
    for msg in messages {
        match msg {
            Message::Assistant(payload) => {
                for tc in &payload.tool_calls {
                    expected_tool_results.insert(tc.id.clone());
                }
            }
            Message::Tool { tool_call_id, .. } => {
                expected_tool_results.remove(tool_call_id);
            }
            _ => {}
        }
    }
    
    for orphan_id in expected_tool_results {
        warnings.push(format!("Tool call '{}' without matching tool result", orphan_id));
    }
    
    warnings
}

/// Helper function to create a summary of a message
pub fn message_summary(index: usize, msg: &Message) -> String {
    let content_preview = msg.content()
        .chars()
        .take(50)
        .collect::<String>();
    
    format!("{}: {}{}", index, msg.role(), 
        if content_preview.len() < msg.content().len() {
            format!("{}...", content_preview)
        } else {
            content_preview
        })
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Tool {
                tool_call_id,
                content,
            } => write!(
                f,
                "tool[{}]: {}",
                tool_call_id,
                content
                    .to_display_string()
                    .chars()
                    .take(200)
                    .collect::<String>()
            ),
            _ => write!(f, "{}: {}", self.role(), self.content()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_system_user_assistant_constructors() {
        let sys = Message::system("s");
        assert!(matches!(&sys, Message::System(c) if c == "s"));
        let usr = Message::user("u");
        assert!(matches!(&usr, Message::User(UserContent::Text(c)) if c == "u"));
        let ast = Message::assistant("a");
        assert!(
            matches!(&ast, Message::Assistant(p) if p.content == "a" && p.tool_calls.is_empty())
        );
    }

    #[test]
    fn message_serialize_deserialize_roundtrip() {
        for msg in [
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("ast"),
            Message::assistant_with_tool_calls(
                "".into(),
                vec![AssistantToolCall {
                    id: "c1".into(),
                    name: "fn".into(),
                    arguments: "{}".into(),
                }],
            ),
            Message::Tool {
                tool_call_id: "c1".into(),
                content: r#"{"ok":true}"#.into(),
            },
        ] {
            let json = serde_json::to_string(&msg).expect("serialize");
            let back: Message = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(msg, back);
        }
    }

    #[test]
    fn assistant_plain_serializes_as_string() {
        let msg = Message::assistant("hi");
        let v: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(v, serde_json::json!({"Assistant": "hi"}));
    }

    #[test]
    fn message_role() {
        assert_eq!(Message::system("x").role(), "system");
        assert_eq!(Message::user("x").role(), "user");
        assert_eq!(Message::assistant("x").role(), "assistant");
        assert_eq!(
            Message::Tool {
                tool_call_id: "c".into(),
                content: "y".into(),
            }
            .role(),
            "tool"
        );
    }

    #[test]
    fn message_content() {
        assert_eq!(Message::system("hello").content(), "hello");
        assert_eq!(Message::user("world").content(), "world");
        assert_eq!(Message::assistant("reply").content(), "reply");
    }

    #[test]
    fn message_display() {
        assert_eq!(Message::system("sys").to_string(), "system: sys");
        assert_eq!(Message::user("usr").to_string(), "user: usr");
        assert_eq!(Message::assistant("ast").to_string(), "assistant: ast");
    }

    #[test]
    fn assistant_content_for_chat_api_maps_empty() {
        assert_eq!(
            super::assistant_content_for_chat_api("").as_ref(),
            "\u{2060}"
        );
        assert_eq!(
            super::assistant_content_for_chat_api("   ").as_ref(),
            "\u{2060}"
        );
        assert_eq!(super::assistant_content_for_chat_api("hi").as_ref(), "hi");
    }

    #[test]
    fn backward_compatibility_legacy_format() {
        let json = r#"{"Assistant":"hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();

        match msg {
            Message::Assistant(payload) => {
                assert_eq!(payload.content, "hello");
                assert_eq!(payload.tool_calls, vec![]);
                assert_eq!(payload.reasoning_content, None);
            }
            _ => panic!("Expected Assistant message"),
        }
    }

    #[test]
    fn user_content_text_serialization() {
        let uc = UserContent::Text("hello".to_string());
        let json = serde_json::to_string(&uc).unwrap();
        assert_eq!(json, "\"hello\"");

        let uc2: UserContent = serde_json::from_str(&json).unwrap();
        assert_eq!(uc, uc2);
    }

    #[test]
    fn user_content_multimodal_serialization() {
        let parts = vec![
            ContentPart::Text {
                text: "see this".to_string(),
            },
            ContentPart::ImageUrl {
                url: "https://example.com/img.png".to_string(),
                detail: Some("high".to_string()),
            },
        ];
        let uc = UserContent::Multimodal(parts);
        let json = serde_json::to_string(&uc).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"type\":\"image_url\""));

        let uc2: UserContent = serde_json::from_str(&json).unwrap();
        assert_eq!(uc, uc2);
    }
}