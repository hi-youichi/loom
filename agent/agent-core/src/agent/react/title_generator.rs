//! Post-graph title generation: fire-and-forget background task.
//!
//! Replaces the old in-graph `TitleNode` + `TitleAssemblyMiddleware` approach.
//! Benefits:
//! - Runs after graph exits, no provider contention with main LLM
//! - Sees the full conversation (better title quality)
//! - Proper cancellation semantics (tokio::select! on cancel token)
//! - No `wait_for_title(30s)` blocking the return path

use std::sync::Arc;

use loom_llm::message::Message;
use loom_llm::LlmProvider;
use tracing::warn;

/// Max characters for stored session summary.
const MAX_SUMMARY_CHARS: usize = 50;

/// Clamp a string to MAX_SUMMARY_CHARS, appending "..." if truncated.
pub fn clamp_summary_chars(s: &str) -> String {
    let count = s.chars().count();
    if count <= MAX_SUMMARY_CHARS {
        return s.to_string();
    }
    let ellipsis = "...";
    let keep = MAX_SUMMARY_CHARS.saturating_sub(ellipsis.chars().count());
    format!("{}{}", s.chars().take(keep).collect::<String>(), ellipsis)
}

/// Build the messages for a title-generation LLM call.
///
/// Takes the last few messages from the conversation (most recent first)
/// and wraps them with a system prompt and a user request.
pub fn build_title_messages(conversation: &[Message]) -> Vec<Message> {
    // Take up to 6 recent messages to keep the prompt short.
    let recent: Vec<Message> = conversation.iter().rev().take(6).cloned().collect();

    let system = Message::system(
        "You are a session titler. Given a conversation, generate a short title \
         (≤50 characters, Chinese OK). Output ONLY the title, no quotes or explanation."
            .to_string(),
    );
    let user = Message::user("Give this conversation a short title.");

    std::iter::once(system)
        .chain(recent.into_iter().rev())
        .chain(std::iter::once(user))
        .collect()
}

/// Generate a session title from the conversation using a separate LLM call.
///
/// Returns `Some(title)` on success, `None` on failure (logged via `warn!`).
/// The title is clamped to 50 characters.
pub async fn generate_title(
    provider: &Arc<dyn LlmProvider>,
    conversation: &[Message],
) -> Option<String> {
    let messages = build_title_messages(conversation);
    let client = provider.create_client(provider.default_model()).ok()?;
    match client.invoke(&messages).await {
        Ok(response) => {
            let title = clamp_summary_chars(response.content.trim());
            if title.is_empty() {
                warn!("Title generation returned empty string");
                None
            } else {
                Some(title)
            }
        }
        Err(e) => {
            warn!("Title generation failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_short_string_unchanged() {
        assert_eq!(clamp_summary_chars("hello"), "hello");
    }

    #[test]
    fn clamp_exact_50_chars_unchanged() {
        let exact_50 = "a".repeat(50);
        assert_eq!(clamp_summary_chars(&exact_50), exact_50);
        assert_eq!(exact_50.len(), 50);
    }

    #[test]
    fn clamp_truncates_long_string() {
        let long_string = "a".repeat(60);
        let result = clamp_summary_chars(&long_string);
        assert_eq!(result.len(), 50);
        assert_eq!(result, "a".repeat(47) + "...");
    }

    #[test]
    fn clamp_unicode_chars() {
        let chinese = "这是一段中文测试文字，每个字符算一个字";
        let result = clamp_summary_chars(chinese);
        assert_eq!(result, chinese);

        let long_chinese = "这是一段很长的中文测试文字，每个字符算一个字，会被截断显示还有更多的字加上去超过五十个字符的限制，需要添加更多的中文字符来确保总长度超过五十个字符";
        let result = clamp_summary_chars(long_chinese);
        assert_eq!(result.chars().count(), 50);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn clamp_empty_string() {
        assert_eq!(clamp_summary_chars(""), "");
    }

    #[test]
    fn clamp_single_char_over_50() {
        let single_over = "a".repeat(51);
        let result = clamp_summary_chars(&single_over);
        assert_eq!(result.len(), 50);
        assert_eq!(result, "a".repeat(47) + "...");
    }

    #[test]
    fn build_title_messages_uses_system_and_user() {
        let conv = vec![Message::user("hello"), Message::assistant("hi there")];
        let msgs = build_title_messages(&conv);
        assert!(msgs.len() >= 3); // system + at least 1 conv + user request
                                  // First message is system
        assert!(matches!(&msgs[0], Message::System(_)));
        // Last message is the user request
        assert!(matches!(msgs.last().unwrap(), Message::User(_)));
    }

    #[test]
    fn build_title_messages_limits_to_6() {
        let conv: Vec<Message> = (0..20).map(|i| Message::user(format!("msg {i}"))).collect();
        let msgs = build_title_messages(&conv);
        // 1 system + 6 conv + 1 user = 8
        assert_eq!(msgs.len(), 8);
    }
}
