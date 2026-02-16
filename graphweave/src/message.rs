//! Minimal message types for agent state.
//!
//! Message roles: System (usually first in the list), User, Assistant.
//! Used by `AgentState::messages` and by agents that read/append messages in `Agent::run`.

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
}
