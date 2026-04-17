//! Title node: generate session title after first think.
//!
//! This node runs once after the first think to create a human-readable
//! title of the conversation for session list display.
//!
//! **Default:** The ReAct graph omits this node unless
//! [`crate::agent::react::runner::options::TitleConfig::enabled`] is set to `true`
//! (for example via [`crate::agent::react::runner::options::AgentOptions::title_config`] or
//! `ReactRunner::new` with `Some(TitleConfig { enabled: true, .. })`).

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AgentError;
use crate::graph::Next;
use crate::llm::LlmClient;
use crate::message::Message;
use crate::state::ReActState;
use crate::Node;

/// Max characters for stored session summary (matches prompt "不超过50字"; total includes "..." when truncated).
const MAX_SUMMARY_CHARS: usize = 50;

fn clamp_summary_chars(s: &str) -> String {
    let count = s.chars().count();
    if count <= MAX_SUMMARY_CHARS {
        return s.to_string();
    }
    let ellipsis = "...";
    let keep = MAX_SUMMARY_CHARS.saturating_sub(ellipsis.chars().count());
    format!("{}{}", s.chars().take(keep).collect::<String>(), ellipsis)
}

/// Node that generates a session title after the first think.
///
/// Uses a separate LLM call to create a concise title (≤50 chars)
/// suitable for display in session lists.
pub struct TitleNode {
    llm: Arc<dyn LlmClient>,
}

impl TitleNode {
    /// Creates a new TitleNode with the given LLM client.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Node<ReActState> for TitleNode {
    fn id(&self) -> &str {
        "title"
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        tracing::debug!("title_node::run - enter");

        if state.summary.is_some() {
            tracing::debug!("title_node::run - summary already set, skipping");
            return Ok((state, Next::Continue));
        }

        let user_messages: Vec<_> = state
            .messages
            .iter()
            .filter_map(|m| match m {
                Message::User(content) => Some(content.clone()),
                _ => None,
            })
            .take(3)
            .collect();

        if user_messages.is_empty() {
            tracing::debug!("title_node::run - no user messages found, skipping");
            return Ok((state, Next::Continue));
        }

        let user_texts: Vec<_> = user_messages
            .iter()
            .map(|c| c.as_text().to_string())
            .collect();

        tracing::debug!(
            "title_node::run - generating title from {} user message(s)",
            user_texts.len()
        );

        let prompt = format!(
            r#"用一句话总结这个对话的主题（不超过50字，用对话的语言）：

{}

只输出摘要内容，不要其他内容。"#,
            user_texts.join("\n")
        );

        let title_messages = vec![
            Message::system(
                "You are a helpful assistant that creates concise conversation summaries.",
            ),
            Message::user(prompt),
        ];

        match self.llm.invoke(&title_messages).await {
            Ok(response) => {
                let raw = response.content.trim();
                let title = clamp_summary_chars(raw);

                if raw.len() != title.len() {
                    tracing::debug!(
                        "title_node::run - title truncated: {} chars -> {} chars",
                        raw.chars().count(),
                        title.chars().count()
                    );
                }

                tracing::info!("title_node::run - generated title: {:?}", title);

                let new_state = ReActState {
                    messages: state.messages,
                    last_reasoning_content: state.last_reasoning_content,
                    tool_calls: state.tool_calls,
                    tool_results: state.tool_results,
                    turn_count: state.turn_count,
                    approval_result: state.approval_result,
                    usage: state.usage,
                    total_usage: state.total_usage,
                    message_count_after_last_think: state.message_count_after_last_think,
                    summary: Some(title),
                    think_count: state.think_count,
                    should_continue: state.should_continue,
                };

                Ok((new_state, Next::Continue))
            }
            Err(e) => {
                tracing::warn!("title_node::run - LLM invoke failed: {}", e);
                Ok((state, Next::Continue))
            }
        }
    }
}

/// Determines if this is the first think (should route to title).
///
/// Returns true when:
/// - think_count == 1 (first think just completed)
/// - summary is not yet set
pub fn is_first_think(state: &ReActState) -> bool {
    state.think_count == 1 && state.summary.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_first_think_true() {
        let state = ReActState {
            messages: vec![Message::user("Hello")],
            think_count: 1,
            summary: None,
            ..Default::default()
        };
        assert!(is_first_think(&state));
    }

    #[test]
    fn test_is_first_think_false_already_titled() {
        let state = ReActState {
            messages: vec![Message::user("Hello")],
            think_count: 1,
            summary: Some("Summary".to_string()),
            ..Default::default()
        };
        assert!(!is_first_think(&state));
    }

    #[test]
    fn test_is_first_think_false_second_think() {
        let state = ReActState {
            messages: vec![Message::user("Hello")],
            think_count: 2,
            summary: None,
            ..Default::default()
        };
        assert!(!is_first_think(&state));
    }

    /// **Scenario**: summary clamp uses char boundaries (no panic on CJK).
    #[test]
    fn clamp_summary_chars_utf8_safe() {
        // Long enough to force truncation; byte index 57 used to split inside '点' and panic.
        let s = "确认配置文件多provider设置中，API的models端点是否用于查询所有模型。请检查多区域部署与密钥轮换策略，并验证限流与审计日志是否完整。";
        let out = super::clamp_summary_chars(s);
        assert!(out.chars().count() <= super::MAX_SUMMARY_CHARS);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn clamp_summary_chars_short_unchanged() {
        assert_eq!(super::clamp_summary_chars("hi"), "hi");
    }
}
