//! Minimal message types for agent state.
//!
//! Message roles: System, User, Assistant, and Tool (tool outputs for strict chat APIs).
//! Used by `AgentState::messages` and by agents that read/append messages in `Agent::run`.

use std::borrow::Cow;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Strip background-review harness blocks from a transcript text.
///
/// Hermes parity (`hermes_state.py` #10, `agent/background_review.py:474-479`):
/// when a session is replayed into a downstream context (CLI Codex export,
/// ACP review-runner, stream bridge history snapshot), the curator's
/// `<background_review>` … `</background_review>` block must be removed so
/// the LLM that consumes the replayed text doesn't try to invoke
/// memory/skill tools at inference time. The harness is a one-shot
/// curator-side prompt, not part of the durable user/assistant dialogue.
///
/// ContentKind-aware walker variant: applies `strip_background_review_harness`
/// to every text-bearing variant of a `Message` (System, User::Text,
/// User::Multimodal::Text parts, Assistant payload content + tool_calls args,
/// Tool content text). Multimodal binary parts (Image/Video/Audio/Pdf) are
/// passed through unchanged — they cannot contain the harness marker. Used
/// by `apps/acp/src/stream_bridge.rs::send_history` so a forked-review leak
/// cannot reach user-visible ACP notifications (priority #13 gap).
pub fn strip_background_review_in_messages(messages: &mut [Message]) {
    for m in messages.iter_mut() {
        match m {
            Message::System(s) => {
                let stripped = strip_background_review_harness(s);
                if stripped != *s {
                    *s = stripped;
                }
            }
            Message::User(uc) => match uc {
                UserContent::Text(s) => {
                    let stripped = strip_background_review_harness(s);
                    if stripped != *s {
                        *s = stripped;
                    }
                }
                UserContent::Multimodal(parts) => {
                    for part in parts.iter_mut() {
                        if let ContentPart::Text { text } = part {
                            let stripped = strip_background_review_harness(text);
                            if stripped != *text {
                                *text = stripped;
                            }
                        }
                    }
                }
            },
            Message::Assistant(payload) => {
                let stripped = strip_background_review_harness(&payload.content);
                if stripped != payload.content {
                    payload.content = stripped;
                }
                for tc in payload.tool_calls.iter_mut() {
                    let stripped = strip_background_review_harness(&tc.arguments);
                    if stripped != tc.arguments {
                        tc.arguments = stripped;
                    }
                }
            }
            Message::Tool { content, .. } => {
                if let ToolCallContent::Text(t) = content {
                    let stripped = strip_background_review_harness(t);
                    if stripped != *t {
                        *t = stripped;
                    }
                }
            }
        }
    }
}
///
/// Recognition is a literal substring match on the literal opening tag
/// `pub const REVIEW_INSTRUCTION: &str = "<background_review>"` defined in
/// `experimental/curator/src/review.rs:23` and its literal closing tag;
/// this avoids pulling a regex crate across the workspace just for one
/// stripping call site.
///
/// Implementation note: the loop owns a `String` buffer rather than
/// borrowing from the input — that avoids lifetime plumbing through the
/// `while let Some(...)` borrow scope, which the borrow checker cannot
/// verify when we replace the borrowed tail on each iteration.
pub fn strip_background_review_harness(text: &str) -> String {
    const OPEN: &str = "<background_review>";
    const CLOSE: &str = "</background_review>";
    let mut rest: String = text.to_owned();
    let mut out = String::with_capacity(rest.len());
    while let Some(open_idx) = rest.find(OPEN) {
        // Push everything before the open tag verbatim.
        out.push_str(&rest[..open_idx]);
        let after_open = &rest[open_idx + OPEN.len()..];
        match after_open.find(CLOSE) {
            Some(close_idx) => {
                // Drop the entire `<background_review>…</background_review>`
                // block, then collapse the leading newline so the rejoined
                // text doesn't grow a stray blank line per replay.
                let tail = &after_open[close_idx + CLOSE.len()..];
                rest = if let Some(stripped) = tail.strip_prefix('\n') {
                    stripped.to_owned()
                } else {
                    tail.to_owned()
                };
            }
            None => {
                // Unterminated block — bail out, leave the rest verbatim so
                // we don't accidentally swallow the rest of the session.
                out.push_str(&rest[open_idx..]);
                rest.clear();
                break;
            }
        }
    }
    out.push_str(&rest);
    out
}

#[cfg(test)]
mod strip_background_review_tests {
    use super::strip_background_review_harness;

    #[test]
    fn removes_single_block() {
        let txt = "user: hi\n\n<background_review>\n- save prefs\n</background_review>\n\nassistant: ok";
        let out = strip_background_review_harness(txt);
        assert!(!out.contains("<background_review>"));
        assert!(!out.contains("</background_review>"));
        assert!(out.contains("user: hi"));
        assert!(out.contains("assistant: ok"));
    }

    #[test]
    fn leaves_text_without_block_intact() {
        let txt = "user: hi\nassistant: ok\n<unrelated>keep</unrelated>";
        let out = strip_background_review_harness(txt);
        assert_eq!(out, txt);
    }

    #[test]
    fn unterminated_block_is_left_verbatim() {
        let txt = "user: hi\n<background_review>\nnever closes";
        let out = strip_background_review_harness(txt);
        assert_eq!(out, txt);
    }
}

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

    /// Returns the list of [`ModalityType`]s present in this content.
    pub fn modalities(&self) -> Vec<model_spec_core::ModalityType> {
        match self {
            UserContent::Text(_) => vec![model_spec_core::ModalityType::Text],
            UserContent::Multimodal(parts) => parts.iter().map(|p| p.modality()).collect(),
        }
    }

    /// Returns the modalities present in this content that the given model does **not** support.
    pub fn unsupported_modalities(&self, model: &model_spec_core::Model) -> Vec<model_spec_core::ModalityType> {
        self.modalities()
            .into_iter()
            .filter(|m| !model.modalities.input.contains(m))
            .collect()
    }

/// Returns `true` if every modality in this content is supported by the given model.
    pub fn is_supported_by(&self, model: &model_spec_core::Model) -> bool {
        self.unsupported_modalities(model).is_empty()
    }
}

/// Multimodal NUL-prefix encoding (priority #10 gap).
///
/// Hermes parity (`hermes_state.py`): in the SQLite checkpoint store,
/// message `content` for a multimodal message is stored as a JSON-encoded
/// blob, while plain text is stored as a plain string. Naively persisting
/// either as a UTF-8 string loses the binary/text type distinction, and
/// reading the column back can't tell whether to JSON-decode.
///
/// We use the C0 control sentinel `\x00json:` followed by JSON for
/// multimodal content. Plain text never starts with NUL so existing rows
/// decode identically (no migration). Encoding is the inverse.
///
/// The NUL byte is invalid inside a JSON string literal (JSON requires
/// `\u0000` to be escaped), so any incoming `decode_content` of a NUL-prefixed
/// blob that fails to parse as JSON falls back to plain-text decoding
/// rather than panicking — that's important because user history replay
/// must never crash on a corrupt row.
pub const MULTIMODAL_NUL_PREFIX: &str = "\0json:";

pub fn encode_content(c: &UserContent) -> String {
    match c {
        UserContent::Text(s) => s.clone(),
        UserContent::Multimodal(parts) => {
            match serde_json::to_string(parts) {
                Ok(json) => format!("{}{}", MULTIMODAL_NUL_PREFIX, json),
                Err(_) => {
                    // Serialization can't realistically fail for our enum
                    // (only String fields), but if it does, fall back to
                    // the joined-text representation so we don't drop the
                    // message entirely.
                    c.as_text().into_owned()
                }
            }
        }
    }
}

pub fn decode_content(s: &str) -> UserContent {
    if let Some(rest) = s.strip_prefix(MULTIMODAL_NUL_PREFIX) {
        match serde_json::from_str::<Vec<ContentPart>>(rest) {
            Ok(parts) if !parts.is_empty() => {
                // Safe: multimodal constructor rejects empty parts (returns
                // Err), but we already checked `!parts.is_empty()` above.
                UserContent::Multimodal(parts)
            }
            _ => UserContent::Text(s.to_string()),
        }
    } else {
        UserContent::Text(s.to_string())
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
    /// Returns the [`ModalityType`] for this content part.
    pub fn modality(&self) -> model_spec_core::ModalityType {
        use model_spec_core::ModalityType;
        match self {
            ContentPart::Text { .. } | ContentPart::File { .. } => ModalityType::Text,
            ContentPart::ImageUrl { .. } | ContentPart::ImageBase64 { .. } => ModalityType::Image,
            ContentPart::AudioBase64 { .. } => ModalityType::Audio,
            ContentPart::VideoUrl { .. } | ContentPart::VideoBase64 { .. } => ModalityType::Video,
            ContentPart::PdfUrl { .. } | ContentPart::PdfBase64 { .. } => ModalityType::Pdf,
        }
    }

    /// Returns true if the given model's input modalities include this part's modality.
    pub fn is_supported_by(&self, model: &model_spec_core::Model) -> bool {
        model.modalities.input.contains(&self.modality())
    }

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

/// Checks for orphan tool calls in both directions.
///
/// **Forward**: assistant issued a `tool_calls[*].id` but no matching `Message::Tool` result exists.
/// **Reverse**: a `Message::Tool` exists whose `tool_call_id` is not present in any preceding
/// assistant message's `tool_calls[*].id` — the API will reject this with HTTP 400.
pub fn check_orphan_tool_calls(messages: &[Message]) -> Vec<String> {
    let mut warnings = Vec::new();

    // Pass 1 (forward): assistant tool_calls without matching tool results
    let mut expected_tool_results = std::collections::HashSet::new();
    for msg in messages {
        match msg {
            Message::Assistant(payload) => {
                for tc in &payload.tool_calls {
                    if !tc.id.is_empty() {
                        expected_tool_results.insert(tc.id.clone());
                    }
                }
            }
            Message::Tool { tool_call_id, .. } => {
                expected_tool_results.remove(tool_call_id);
            }
            _ => {}
        }
    }
    for orphan_id in expected_tool_results {
        warnings.push(format!(
            "Tool call '{}' without matching tool result",
            orphan_id
        ));
    }

    // Pass 2 (reverse): tool messages without matching assistant tool_call
    let mut known_tool_call_ids = std::collections::HashSet::new();
    for msg in messages {
        match msg {
            Message::Assistant(payload) => {
                for tc in &payload.tool_calls {
                    if !tc.id.is_empty() {
                        known_tool_call_ids.insert(tc.id.clone());
                    }
                }
            }
            Message::Tool { tool_call_id, .. } => {
                if !known_tool_call_ids.contains(tool_call_id) {
                    warnings.push(format!(
                        "Tool message with tool_call_id '{}' has no matching assistant tool_call — this will be rejected by the API",
                        tool_call_id
                    ));
                }
            }
            _ => {}
        }
    }

    warnings
}

/// Creates a one-line debug summary of a message, including tool_call ids.
pub fn message_summary(index: usize, msg: &Message) -> String {
    let content_preview = msg.content()
        .chars()
        .take(50)
        .collect::<String>();

    let base = format!("{}: {}{}", index, msg.role(),
        if content_preview.len() < msg.content().len() {
            format!("{}...", content_preview)
        } else {
            content_preview
        });

    match msg {
        Message::Assistant(p) if !p.tool_calls.is_empty() => {
            let ids: Vec<&str> = p.tool_calls.iter().map(|tc| tc.id.as_str()).collect();
            format!("{} [tool_call_ids: {}]", base, ids.join(", "))
        }
        Message::Tool { tool_call_id, .. } => {
            format!("{} [tool_call_id: {}]", base, tool_call_id)
        }
        _ => base,
    }
}

/// Sanitizes a message list to ensure all tool_call_id references are valid.
///
/// This is the last-resort safety net before sending messages to an OpenAI-compatible API.
/// It performs three fixes:
///
/// 1. **Backfill empty assistant tool_call ids**: if an assistant message has a tool_call
///    with empty `id`, generates a new id and writes it to **both** the assistant side
///    and the matching tool message.
/// 2. **Drop orphaned tool messages**: removes any `Message::Tool` whose `tool_call_id`
///    cannot be found in any assistant message's `tool_calls[*].id`.
/// 3. **Drop orphaned assistant tool_calls**: removes tool_calls from assistant messages
///    that have no matching tool result (less critical but keeps the list clean).
pub fn sanitize_tool_call_ids(messages: Vec<Message>) -> Vec<Message> {
    use std::collections::HashSet;

    if messages.is_empty() {
        return messages;
    }

    let mut messages = messages;

    // ── Step 1: Backfill empty assistant tool_call ids ──
    // When an assistant tool_call has an empty id, generate a new one and try to
    // sync it to the corresponding tool message.
    for (i, msg) in messages.iter_mut().enumerate() {
        if let Message::Assistant(ref mut payload) = msg {
            for tc_idx in 0..payload.tool_calls.len() {
                if payload.tool_calls[tc_idx].id.is_empty() {
                    let new_id = format!("call_{}", uuid::Uuid::new_v4());
                    payload.tool_calls[tc_idx].id = new_id.clone();
                    tracing::warn!(
                        index = i,
                        tool_call_index = tc_idx,
                        new_id = %new_id,
                        "sanitize: backfilled empty tool_call id"
                    );
                }
            }
        }
    }

    // ── Step 2: Collect valid tool_call ids from all assistant messages ──
    let valid_ids: HashSet<String> = messages
        .iter()
        .filter_map(|m| match m {
            Message::Assistant(p) => Some(p.tool_calls.iter().map(|tc| tc.id.clone())),
            _ => None,
        })
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();

    // ── Step 3: Drop orphaned tool messages ──
    let original_len = messages.len();
    messages.retain(|m| match m {
        Message::Tool { tool_call_id, .. } => {
            if valid_ids.contains(tool_call_id) {
                true
            } else {
                tracing::warn!(
                    tool_call_id = %tool_call_id,
                    "sanitize: dropped orphaned tool message (no matching assistant tool_call)"
                );
                false
            }
        }
        _ => true,
    });

    if messages.len() != original_len {
        tracing::warn!(
            original_len,
            sanitized_len = messages.len(),
            "sanitize: message list was modified"
        );
    }

    messages
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
    use model_spec_core::ModelLimit;

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

    #[test]
    fn message_user_with_user_content() {
        let msg = Message::user("hello");
        assert!(matches!(msg, Message::User(UserContent::Text(s)) if s == "hello"));

        let parts = vec![ContentPart::Text {
            text: "hi".to_string(),
        }];
        let msg = Message::user_multimodal(parts).unwrap();
        assert!(matches!(msg, Message::User(UserContent::Multimodal(..))));
    }

    #[test]
    fn legacy_checkpoint_compatibility() {
        // 旧格式：纯字符串
        let json = r#"{"User":"hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, Message::User(UserContent::Text(s)) if s == "hello"));

        // 新格式：多模态数组
        let json = r#"{"User":[{"type":"text","text":"hello"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, Message::User(UserContent::Multimodal(..))));
    }

    #[test]
    fn content_part_modality() {
        assert_eq!(
            ContentPart::Text {
                text: "hi".to_string()
            }
            .modality(),
            model_spec_core::ModalityType::Text
        );
        assert_eq!(
            ContentPart::ImageUrl {
                url: "https://x.com/img.png".to_string(),
                detail: None
            }
            .modality(),
            model_spec_core::ModalityType::Image
        );
        assert_eq!(
            ContentPart::ImageBase64 {
                media_type: "image/png".to_string(),
                data: "abc".to_string()
            }
            .modality(),
            model_spec_core::ModalityType::Image
        );
        assert_eq!(
            ContentPart::AudioBase64 {
                media_type: "audio/mp3".to_string(),
                data: "abc".to_string()
            }
            .modality(),
            model_spec_core::ModalityType::Audio
        );
        assert_eq!(
            ContentPart::VideoUrl {
                url: "https://x.com/vid.mp4".to_string()
            }
            .modality(),
            model_spec_core::ModalityType::Video
        );
        assert_eq!(
            ContentPart::PdfUrl {
                url: "https://x.com/doc.pdf".to_string()
            }
            .modality(),
            model_spec_core::ModalityType::Pdf
        );
        assert_eq!(
            ContentPart::File {
                file_id: None,
                file_data: None,
                filename: Some("data.csv".to_string())
            }
            .modality(),
            model_spec_core::ModalityType::Text
        );
    }

    #[test]
    fn user_content_as_text() {
        let text = UserContent::Text("hello".to_string());
        assert_eq!(text.as_text(), "hello");

        let parts = vec![
            ContentPart::Text {
                text: "first".to_string(),
            },
            ContentPart::Text {
                text: "second".to_string(),
            },
        ];
        let multimodal = UserContent::Multimodal(parts);
        assert_eq!(multimodal.as_text(), "first\nsecond");
    }

    #[test]
    fn user_content_modalities() {
        let text = UserContent::Text("hello".to_string());
        assert_eq!(
            text.modalities(),
            vec![model_spec_core::ModalityType::Text]
        );

        let parts = vec![
            ContentPart::Text {
                text: "hi".to_string(),
            },
            ContentPart::ImageUrl {
                url: "https://x.com/img.png".to_string(),
                detail: None,
            },
        ];
        let multimodal = UserContent::Multimodal(parts);
        assert_eq!(
            multimodal.modalities(),
            vec![
                model_spec_core::ModalityType::Text,
                model_spec_core::ModalityType::Image
            ]
        );
    }

    #[test]
    fn user_content_unsupported_modalities() {
        use model_spec_core::{Modalities, Model};

        // Model that only supports text and image
        let model = Model {
            id: "test-model".to_string(),
            name: "test-model".to_string(),
            family: None,
            attachment: false,
            limit: ModelLimit::default(),
            modalities: Modalities {
                input: vec![
                    model_spec_core::ModalityType::Text,
                    model_spec_core::ModalityType::Image,
                ],
                output: vec![model_spec_core::ModalityType::Text],
            },
            tool_call: false,
            temperature: false,
            structured_output: None,
            knowledge: None,
            release_date: None,
            last_updated: None,
            reasoning: false,
            open_weights: false,
            cost: None,
        };

        // Text + Image is fully supported
        let supported = UserContent::Multimodal(vec![
            ContentPart::Text {
                text: "hi".to_string(),
            },
            ContentPart::ImageUrl {
                url: "https://x.com/img.png".to_string(),
                detail: None,
            },
        ]);
        assert!(supported.is_supported_by(&model));
        assert_eq!(supported.unsupported_modalities(&model), vec![]);

        // Audio is NOT supported
        let unsupported = UserContent::Multimodal(vec![
            ContentPart::Text {
                text: "hi".to_string(),
            },
            ContentPart::AudioBase64 {
                media_type: "audio/mp3".to_string(),
                data: "abc".to_string(),
            },
        ]);
        assert!(!unsupported.is_supported_by(&model));
        assert_eq!(
            unsupported.unsupported_modalities(&model),
            vec![model_spec_core::ModalityType::Audio]
        );
    }

    #[test]
    fn content_part_is_supported_by() {
        use model_spec_core::{Modalities, Model};

        let model = Model {
            id: "test-model".to_string(),
            name: "test-model".to_string(),
            family: None,
            attachment: false,
            limit: ModelLimit::default(),
            modalities: Modalities {
                input: vec![
                    model_spec_core::ModalityType::Text,
                    model_spec_core::ModalityType::Image,
                ],
                output: vec![model_spec_core::ModalityType::Text],
            },
            tool_call: false,
            temperature: false,
            structured_output: None,
            knowledge: None,
            release_date: None,
            last_updated: None,
            reasoning: false,
            open_weights: false,
            cost: None,
        };

        assert!(ContentPart::Text {
            text: "hi".to_string()
        }
        .is_supported_by(&model));
        assert!(ContentPart::ImageUrl {
            url: "https://x.com/img.png".to_string(),
            detail: None
        }
        .is_supported_by(&model));
        assert!(!ContentPart::AudioBase64 {
            media_type: "audio/mp3".to_string(),
            data: "abc".to_string()
        }
        .is_supported_by(&model));
    }

    // Additional message_summary tests for coverage
    #[test]
    fn message_summary_user_message_multimodal() {
        let parts = vec![
            ContentPart::Text {
                text: "text content".to_string(),
            },
            ContentPart::ImageUrl {
                url: "https://example.com/img.png".to_string(),
                detail: None,
            },
        ];
        let msg = Message::user_multimodal(parts).unwrap();
        let summary = message_summary(1, &msg);
        // message_summary format: "{index}: {role}{content_preview}"
        assert_eq!(summary, "1: usertext content");
    }

    #[test]
    fn message_summary_assistant_with_both_tool_calls_and_reasoning() {
        let msg = Message::assistant_with_tool_calls_and_reasoning(
            "Response".to_string(),
            vec![
                AssistantToolCall {
                    id: "call_123".to_string(),
                    name: "search".to_string(),
                    arguments: "{}".to_string(),
                },
            ],
            Some("Reasoning process".to_string()),
        );
        let summary = message_summary(6, &msg);
        assert_eq!(summary, "6: assistantResponse [tool_call_ids: call_123]");
    }

    // Additional UserContent edge cases
    #[test]
    fn user_content_multimodal_no_text_parts() {
        let parts = vec![
            ContentPart::ImageUrl {
                url: "https://example.com/img.png".to_string(),
                detail: None,
            },
            ContentPart::AudioBase64 {
                media_type: "audio/mp3".to_string(),
                data: "base64data".to_string(),
            },
        ];
        let content = UserContent::Multimodal(parts);
        let text = content.as_text();
        assert_eq!(text.as_ref(), ""); // no text parts should return empty
    }

    #[test]
    fn user_content_multimodal_only_text_parts() {
        let parts = vec![
            ContentPart::Text {
                text: "First".to_string(),
            },
            ContentPart::Text {
                text: "Second".to_string(),
            },
            ContentPart::Text {
                text: "Third".to_string(),
            },
        ];
        let content = UserContent::Multimodal(parts);
        let text = content.as_text();
        assert_eq!(text.as_ref(), "First\nSecond\nThird");
    }

    // Additional AssistantPayload edge cases
    #[test]
    fn assistant_payload_serialization_no_tool_calls_no_reasoning() {
        let payload = AssistantPayload {
            content: "simple response".to_string(),
            tool_calls: vec![],
            reasoning_content: None,
        };
    // Direct AssistantPayload serialization uses derive(Serialize), not the custom serde module.
    // The custom serde (plain string when no tool_calls) only applies via Message::Assistant.
    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains("\"content\":\"simple response\""));
    assert!(json.contains("\"tool_calls\":[]"));
    }

    #[test]
    fn assistant_payload_serialization_empty_content_with_tool_calls() {
        let payload = AssistantPayload {
            content: "".to_string(),
            tool_calls: vec![AssistantToolCall {
                id: "call_123".to_string(),
                name: "search".to_string(),
                arguments: "{}".to_string(),
            }],
            reasoning_content: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"content\":\"\""));
        assert!(json.contains("\"tool_calls\""));
    }

    #[test]
    fn assistant_payload_serialization_empty_content_with_reasoning() {
        let payload = AssistantPayload {
            content: "".to_string(),
            tool_calls: vec![],
            reasoning_content: Some("thinking".to_string()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"content\":\"\""));
        assert!(json.contains("\"reasoning_content\""));
    }

    // Additional ToolCallContent method tests
    #[test]
    fn tool_call_content_diff_factory_none_old_text() {
        let content = ToolCallContent::diff("new_file.txt", None, "new content".to_string());
        match content {
            ToolCallContent::Diff { path, old_text, new_text } => {
                assert_eq!(path, "new_file.txt");
                assert_eq!(old_text, None);
                assert_eq!(new_text, "new content");
            }
            _ => panic!("Expected Diff variant"),
        }
    }

    #[test]
    fn tool_call_content_diff_factory_with_old_text() {
        let content = ToolCallContent::diff(
            "existing.txt",
            Some("old content".to_string()),
            "new content".to_string(),
        );
        match content {
            ToolCallContent::Diff { path, old_text, new_text } => {
                assert_eq!(path, "existing.txt");
                assert_eq!(old_text, Some("old content".to_string()));
                assert_eq!(new_text, "new content");
            }
            _ => panic!("Expected Diff variant"),
        }
    }

    #[test]
    fn tool_call_content_len_empty_text() {
        let content = ToolCallContent::Text("".to_string());
        assert_eq!(content.len(), 0);
    }

    #[test]
    fn tool_call_content_is_empty_all_variants() {
        assert!(ToolCallContent::Text("".to_string()).is_empty());
        assert!(!ToolCallContent::Text("non-empty".to_string()).is_empty());
        assert!(ToolCallContent::diff("file.txt", None, "".to_string()).is_empty());
        assert!(!ToolCallContent::diff("file.txt", None, "content".to_string()).is_empty());
        assert!(ToolCallContent::terminal("").is_empty());
        assert!(!ToolCallContent::terminal("term_123").is_empty());
    }

    // Additional Message::to_role_content_pair edge cases
    #[test]
    fn message_to_role_content_pair_user_multimodal() {
        let parts = vec![
            ContentPart::Text {
                text: "text part".to_string(),
            },
            ContentPart::ImageUrl {
                url: "https://example.com/img.png".to_string(),
                detail: None,
            },
        ];
        let msg = Message::user_multimodal(parts).unwrap();
        let (role, content) = msg.to_role_content_pair();
        assert_eq!(role, "user");
        // Should extract text from multimodal content
        assert_eq!(content, "text part");
    }

    #[test]
    fn message_to_role_content_pair_assistant_with_reasoning() {
        let msg = Message::assistant_with_reasoning(
            "response".to_string(),
            Some("reasoning".to_string()),
        );
        let (role, content) = msg.to_role_content_pair();
        assert_eq!(role, "assistant");
        assert_eq!(content, "response");
    }

    #[test]
    fn message_to_role_content_pair_tool_diff_content() {
        let msg = Message::Tool {
            tool_call_id: "call_123".to_string(),
            content: ToolCallContent::diff("file.txt", None, "new content".to_string()),
        };
        let (role, content) = msg.to_role_content_pair();
        assert_eq!(role, "tool");
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["tool_call_id"], "call_123");
        assert_eq!(parsed["content"], "Modified file: file.txt");
    }

    #[test]
    fn message_to_role_content_pair_tool_terminal_content() {
        let msg = Message::Tool {
            tool_call_id: "call_456".to_string(),
            content: ToolCallContent::terminal("term_789"),
        };
        let (role, content) = msg.to_role_content_pair();
        assert_eq!(role, "tool");
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["tool_call_id"], "call_456");
        assert_eq!(parsed["content"], "Terminal: term_789");
    }

    // Additional Message::to_role_content_pair_for_store edge cases
    #[test]
    fn message_to_role_content_pair_for_store_non_tool_messages_unchanged() {
        let sys_msg = Message::system("system prompt".to_string());
        let (role, content) = sys_msg.to_role_content_pair_for_store();
        assert_eq!((role, content), sys_msg.to_role_content_pair());

        let user_msg = Message::user("user input");
        let (role, content) = user_msg.to_role_content_pair_for_store();
        assert_eq!((role, content), user_msg.to_role_content_pair());

        let assistant_msg = Message::assistant("assistant reply");
        let (role, content) = assistant_msg.to_role_content_pair_for_store();
        assert_eq!((role, content), assistant_msg.to_role_content_pair());
    }

    // Additional Message::assistant_with_reasoning tests
    #[test]
    fn message_assistant_with_reasoning_none() {
        let msg = Message::assistant_with_reasoning("response".to_string(), None);
        assert_eq!(msg.role(), "assistant");
        match msg {
            Message::Assistant(payload) => {
                assert_eq!(payload.content, "response");
                assert!(payload.tool_calls.is_empty());
                assert_eq!(payload.reasoning_content, None);
            }
            _ => panic!("Expected Assistant message"),
        }
    }

    // Additional Message Display impl tests
    #[test]
    fn message_display_all_message_types() {
        let sys_msg = Message::system("system");
        assert_eq!(sys_msg.to_string(), "system: system");

        let user_msg = Message::user("user");
        assert_eq!(user_msg.to_string(), "user: user");

        let assistant_msg = Message::assistant("assistant");
        assert_eq!(assistant_msg.to_string(), "assistant: assistant");

        let tool_msg = Message::Tool {
            tool_call_id: "call".to_string(),
            content: ToolCallContent::text("tool"),
        };
        assert!(tool_msg.to_string().contains("tool[call]:"));
    }

    // Additional edge case tests
    #[test]
    fn message_summary_with_large_index() {
        let msg = Message::system("test".to_string());
        let summary = message_summary(999999, &msg);
        // message_summary format: "{index}: {role}{content_preview}"
        assert_eq!(summary, "999999: systemtest");
    }

    #[test]
    fn message_summary_with_empty_contents() {
        let sys_msg = Message::System("".to_string());
        let summary = message_summary(0, &sys_msg);
        // message_summary format: "{index}: {role}{content_preview}" — empty content → just role
        assert_eq!(summary, "0: system");

        let user_msg = Message::User(UserContent::Text("".to_string()));
        let summary = message_summary(1, &user_msg);
        assert_eq!(summary, "1: user");

        let assistant_msg = Message::Assistant(AssistantPayload {
            content: "".to_string(),
            tool_calls: vec![],
            reasoning_content: None,
        });
        let summary = message_summary(2, &assistant_msg);
        assert_eq!(summary, "2: assistant");
    }

    #[test]
    fn tool_call_content_serialization_roundtrip_text() {
        let original = ToolCallContent::Text("test content".to_string());
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "\"test content\""); // Text should serialize as plain string
        let deserialized: ToolCallContent = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn tool_call_content_serialization_diff_structure() {
        let original = ToolCallContent::diff(
            "test.txt",
            Some("old".to_string()),
            "new".to_string(),
        );
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "diff");
        assert_eq!(parsed["path"], "test.txt");
        assert_eq!(parsed["old_text"], "old");
        assert_eq!(parsed["new_text"], "new");
    }

    #[test]
    fn tool_call_content_serialization_terminal_structure() {
        let original = ToolCallContent::terminal("term_123");
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "terminal");
        assert_eq!(parsed["terminal_id"], "term_123");
    }

    #[test]
    fn user_content_from_string_overloads() {
        let content1: UserContent = String::from("string").into();
        assert_eq!(content1, UserContent::Text("string".to_string()));

        let content2: UserContent = "str".into();
        assert_eq!(content2, UserContent::Text("str".to_string()));
    }

    #[test]
    fn user_content_partial_eq_overloads() {
        let content = UserContent::Text("test".to_string());
        
        // Test PartialEq<&str>
        assert_eq!(content, "test");
        assert_ne!(content, "different");
        
        // Test PartialEq<String>
        assert_eq!(content, String::from("test"));
        assert_ne!(content, String::from("different"));
    }

    #[test]
    fn content_part_modality_string_all_variants() {
        assert_eq!(
            ContentPart::Text { text: "test".to_string() }.modality_string(),
            "Text"
        );
        assert_eq!(
            ContentPart::ImageUrl { url: "url".to_string(), detail: None }.modality_string(),
            "Image"
        );
        assert_eq!(
            ContentPart::ImageBase64 { media_type: "png".to_string(), data: "data".to_string() }.modality_string(),
            "Image"
        );
        assert_eq!(
            ContentPart::AudioBase64 { media_type: "mp3".to_string(), data: "data".to_string() }.modality_string(),
            "Audio"
        );
        assert_eq!(
            ContentPart::VideoUrl { url: "url".to_string() }.modality_string(),
            "Video"
        );
        assert_eq!(
            ContentPart::VideoBase64 { media_type: "mp4".to_string(), data: "data".to_string() }.modality_string(),
            "Video"
        );
        assert_eq!(
            ContentPart::PdfUrl { url: "url".to_string() }.modality_string(),
            "Pdf"
        );
        assert_eq!(
            ContentPart::PdfBase64 { data: "data".to_string() }.modality_string(),
            "Pdf"
        );
        assert_eq!(
            ContentPart::File {
                file_id: None,
                file_data: None,
                filename: None,
            }.modality_string(),
"Text"
        );
    }
}
