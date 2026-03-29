//! Minimal message types for agent state.
//!
//! Message roles: System, User, Assistant, and Tool (tool outputs for strict chat APIs).
//! Used by `AgentState::messages` and by agents that read/append messages in `Agent::run`.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::memory::uuid6;
use crate::tool_source::ToolCallContent;

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

/// A single message in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    /// System prompt; typically placed first in the message list.
    System(String),
    /// User input.
    User(String),
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
    pub fn user(content: impl Into<String>) -> Self {
        Self::User(content.into())
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

    /// Returns the primary text content (assistant text, user/system string, or tool output body).
    pub fn content(&self) -> Cow<'_, str> {
        match self {
            Message::System(s) | Message::User(s) => Cow::Borrowed(s),
            Message::Assistant(p) => Cow::Borrowed(p.content.as_str()),
            Message::Tool { content, .. } => Cow::Owned(content.to_display_string()),
        }
    }

    /// Role plus a single `content` string for HTTP/API or SQLite `(role, content)` rows.
    ///
    /// Assistant messages with `tool_calls` serialize the payload as JSON. Tool messages use
    /// `{"tool_call_id","content"}`.
    pub fn to_role_content_pair(&self) -> (&'static str, String) {
        match self {
            Message::System(c) => ("system", c.clone()),
            Message::User(c) => ("user", c.clone()),
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

    /// Like [`to_role_content_pair`], but empty `tool_call_id` on tool messages gets a generated id
    /// before persistence (avoids invalid rows for strict chat APIs).
    pub fn to_role_content_pair_for_store(&self) -> (&'static str, String) {
        if let Message::Tool {
            tool_call_id,
            content,
        } = self
        {
            if tool_call_id.is_empty() {
                warn!("tool message with empty tool_call_id on persist; generating id");
                let id = format!("call_{}", uuid6());
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
///
/// OpenAI rejects assistant messages whose `content` is empty when that turn did not include
/// `tool_calls`. A single WORD JOINER (`U+2060`) is non-empty for validators and invisible in typical UIs.
/// **Note:** older code returned `""` for whitespace-only content; that failed API validation—this
/// function now always yields a non-empty string for trim-empty input.
pub(crate) fn assistant_content_for_chat_api(s: &str) -> Cow<'_, str> {
    if s.trim().is_empty() {
        Cow::Borrowed("\u{2060}")
    } else {
        Cow::Borrowed(s)
    }
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
                content.to_display_string().chars().take(200).collect::<String>()
            ),
            _ => write!(f, "{}: {}", self.role(), self.content()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: system/user/assistant constructors produce the correct variant with content.
    #[test]
    fn message_system_user_assistant_constructors() {
        let sys = Message::system("s");
        assert!(matches!(&sys, Message::System(c) if c == "s"));
        let usr = Message::user("u");
        assert!(matches!(&usr, Message::User(c) if c == "u"));
        let ast = Message::assistant("a");
        assert!(matches!(&ast, Message::Assistant(p) if p.content == "a" && p.tool_calls.is_empty()));
    }

    /// **Scenario**: Each Message variant round-trips through serde.
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

    /// **Scenario**: Plain assistant still serializes as a JSON string for backward compatibility.
    #[test]
    fn assistant_plain_serializes_as_string() {
        let msg = Message::assistant("hi");
        let v: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(v, serde_json::json!({"Assistant": "hi"}));
    }

    /// **Scenario**: role() returns correct string for each variant.
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

    /// **Scenario**: content() returns the inner string for each variant.
    #[test]
    fn message_content() {
        assert_eq!(Message::system("hello").content(), "hello");
        assert_eq!(Message::user("world").content(), "world");
        assert_eq!(Message::assistant("reply").content(), "reply");
        assert_eq!(
            Message::Tool {
                tool_call_id: "c".into(),
                content: "out".into(),
            }
            .content(),
            "out"
        );
    }

    /// **Scenario**: Display formats as "role: content".
    #[test]
    fn message_display() {
        assert_eq!(Message::system("sys").to_string(), "system: sys");
        assert_eq!(Message::user("usr").to_string(), "user: usr");
        assert_eq!(Message::assistant("ast").to_string(), "assistant: ast");
    }

    #[test]
    fn message_display_tool_short() {
        let msg = Message::Tool {
            tool_call_id: "c1".into(),
            content: "ok".into(),
        };
        assert_eq!(msg.to_string(), "tool[c1]: ok");
    }

    #[test]
    fn message_display_tool_truncates_at_200() {
        let long = "x".repeat(300);
        let msg = Message::Tool {
            tool_call_id: "c1".into(),
            content: ToolCallContent::text(long),
        };
        let display = msg.to_string();
        assert!(display.starts_with("tool[c1]: "));
        assert_eq!(display.chars().count(), "tool[c1]: ".chars().count() + 200);
    }

    #[test]
    fn to_role_content_pair_matches_variants() {
        let (r, c) = Message::user("u").to_role_content_pair();
        assert_eq!(r, "user");
        assert_eq!(c, "u");
        let (r, c) = Message::Tool {
            tool_call_id: "t1".into(),
            content: "body".into(),
        }
        .to_role_content_pair();
        assert_eq!(r, "tool");
        let v: serde_json::Value = serde_json::from_str(&c).unwrap();
        assert_eq!(v["tool_call_id"], "t1");
        assert_eq!(v["content"], "body");
    }

    #[test]
    fn to_role_content_pair_for_store_fills_empty_tool_call_id() {
        let (r, c) = Message::Tool {
            tool_call_id: String::new(),
            content: "x".into(),
        }
        .to_role_content_pair_for_store();
        assert_eq!(r, "tool");
        let v: serde_json::Value = serde_json::from_str(&c).unwrap();
        let id = v["tool_call_id"].as_str().expect("id");
        assert!(!id.is_empty());
        assert_eq!(v["content"], "x");
    }

    /// **Scenario**: empty assistant text is mapped to a non-empty placeholder for APIs.
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
    fn assistant_payload_with_reasoning() {
        let payload = super::AssistantPayload {
            content: "test content".to_string(),
            tool_calls: vec![],
            reasoning_content: Some("reasoning steps".to_string()),
        };
        
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("reasoning_content"));
        assert!(json.contains("reasoning steps"));
        
        let decoded: super::AssistantPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.content, "test content");
        assert_eq!(decoded.reasoning_content, Some("reasoning steps".to_string()));
        assert_eq!(decoded.tool_calls, vec![]);
    }

    #[test]
    fn assistant_payload_without_reasoning() {
        let payload = super::AssistantPayload {
            content: "test content".to_string(),
            tool_calls: vec![],
            reasoning_content: None,
        };
        
        let json = serde_json::to_string(&payload).unwrap();
        assert!(!json.contains("reasoning_content"));
        
        let decoded: super::AssistantPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.content, "test content");
        assert_eq!(decoded.reasoning_content, None);
    }

    #[test]
    fn backward_compatibility_legacy_format() {
        let json = r#"{"Assistant":"hello"}"#;
        let msg: super::Message = serde_json::from_str(json).unwrap();
        
        match msg {
            super::Message::Assistant(payload) => {
                assert_eq!(payload.content, "hello");
                assert_eq!(payload.tool_calls, vec![]);
                assert_eq!(payload.reasoning_content, None);
            },
            _ => panic!("Expected Assistant message"),
        }
    }

    #[test]
    fn backward_compatibility_structured_without_reasoning() {
        let json = r#"{"content":"hello","tool_calls":[]}"#;
        let payload: super::AssistantPayload = serde_json::from_str(json).unwrap();
        
        assert_eq!(payload.content, "hello");
        assert_eq!(payload.tool_calls, vec![]);
        assert_eq!(payload.reasoning_content, None);
    }

    #[test]
    fn message_assistant_with_reasoning() {
        let msg = super::Message::assistant_with_reasoning("response", Some("thinking".to_string()));
        
        match msg {
            super::Message::Assistant(payload) => {
                assert_eq!(payload.content, "response");
                assert_eq!(payload.reasoning_content, Some("thinking".to_string()));
                assert_eq!(payload.tool_calls, vec![]);
            },
            _ => panic!("Expected Assistant message"),
        }
    }

    #[test]
    fn message_assistant_with_tool_calls_and_reasoning() {
        use super::AssistantToolCall;
        
        let tool_calls = vec![
            AssistantToolCall {
                id: "call_123".to_string(),
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        ];
        
        let msg = super::Message::assistant_with_tool_calls_and_reasoning(
            "response".to_string(),
            tool_calls.clone(),
            Some("reasoning".to_string()),
        );
        
        match msg {
            super::Message::Assistant(payload) => {
                assert_eq!(payload.content, "response");
                assert_eq!(payload.tool_calls, tool_calls);
                assert_eq!(payload.reasoning_content, Some("reasoning".to_string()));
            },
            _ => panic!("Expected Assistant message"),
        }
    }

    #[test]
    fn message_serialize_deserialize_with_reasoning() {
        let msg = super::Message::assistant_with_reasoning("test", Some("reasoning".to_string()));
        
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: super::Message = serde_json::from_str(&json).unwrap();
        
        assert_eq!(msg, decoded);
    }
}
