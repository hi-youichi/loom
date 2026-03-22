//! Minimal message types for agent state.
//!
//! Message roles: System (usually first in the list), User, Assistant.
//! Used by `AgentState::messages` and by agents that read/append messages in `Agent::run`.

use std::borrow::Cow;

/// A single message in the conversation.
///
/// Roles: system prompt, user input, assistant reply.
/// No separate Tool role in this minimal design; extend in later Sprints.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    /// System prompt; typically placed first in the message list.
    System(String),
    /// User input.
    User(String),
    /// Model/agent reply.
    Assistant(String),
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

    /// Creates an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant(content.into())
    }

    /// Returns the role name as a string.
    pub fn role(&self) -> &'static str {
        match self {
            Message::System(_) => "system",
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
        }
    }

    /// Returns the content of the message.
    pub fn content(&self) -> &str {
        match self {
            Message::System(s) => s,
            Message::User(s) => s,
            Message::Assistant(s) => s,
        }
    }
}

/// Assistant `content` for chat-completion HTTP requests.
///
/// Loom stores tool rounds as a (possibly empty) assistant turn followed by user
/// text with tool output. Providers such as OpenAI reject assistant messages whose
/// `content` is empty when that turn did not include `tool_calls` in the same payload.
/// A single WORD JOINER is non-empty for validators and invisible in typical UIs.
pub(crate) fn assistant_content_for_chat_api(s: &str) -> Cow<'_, str> {
    if s.trim().is_empty() {
        Cow::Borrowed("\u{2060}")
    } else {
        Cow::Borrowed(s)
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.role(), self.content())
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
        assert!(matches!(&ast, Message::Assistant(c) if c == "a"));
    }

    /// **Scenario**: Each Message variant round-trips through serde.
    #[test]
    fn message_serialize_deserialize_roundtrip() {
        for msg in [
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("ast"),
        ] {
            let json = serde_json::to_string(&msg).expect("serialize");
            let back: Message = serde_json::from_str(&json).expect("deserialize");
            match (&msg, &back) {
                (Message::System(a), Message::System(b)) => assert_eq!(a, b),
                (Message::User(a), Message::User(b)) => assert_eq!(a, b),
                (Message::Assistant(a), Message::Assistant(b)) => assert_eq!(a, b),
                _ => panic!("variant mismatch: {:?} vs {:?}", msg, back),
            }
        }
    }

    /// **Scenario**: role() returns correct string for each variant.
    #[test]
    fn message_role() {
        assert_eq!(Message::system("x").role(), "system");
        assert_eq!(Message::user("x").role(), "user");
        assert_eq!(Message::assistant("x").role(), "assistant");
    }

    /// **Scenario**: content() returns the inner string for each variant.
    #[test]
    fn message_content() {
        assert_eq!(Message::system("hello").content(), "hello");
        assert_eq!(Message::user("world").content(), "world");
        assert_eq!(Message::assistant("reply").content(), "reply");
    }

    /// **Scenario**: Display formats as "role: content".
    #[test]
    fn message_display() {
        assert_eq!(Message::system("sys").to_string(), "system: sys");
        assert_eq!(Message::user("usr").to_string(), "user: usr");
        assert_eq!(Message::assistant("ast").to_string(), "assistant: ast");
    }

    /// **Scenario**: empty assistant text is mapped to a non-empty placeholder for APIs.
    #[test]
    fn assistant_content_for_chat_api_maps_empty() {
        assert_eq!(super::assistant_content_for_chat_api("").as_ref(), "\u{2060}");
        assert_eq!(super::assistant_content_for_chat_api("   ").as_ref(), "\u{2060}");
        assert_eq!(super::assistant_content_for_chat_api("hi").as_ref(), "hi");
    }
}
