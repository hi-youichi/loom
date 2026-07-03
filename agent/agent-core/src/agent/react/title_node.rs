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
use tokio::sync::{Notify, OnceCell};
use tracing::warn;

use loom_graph_core::GraphError;
use loom_graph_core::Next;
use loom_graph_core::Node;
use loom_llm::{LlmHeaders, LlmProvider};
use loom_llm::message::Message;
use stream_event::StreamEvent;
use crate::run::TypedAnyStreamEvent;
use crate::state::ReActState;

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
    provider: Arc<dyn LlmProvider>,
    slot: Arc<OnceCell<String>>,
    notify: Arc<Notify>,
    sender: Option<Arc<dyn Fn(TypedAnyStreamEvent) + Send + Sync>>,
}

impl TitleNode {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        _headers: Option<LlmHeaders>,
        slot: Arc<OnceCell<String>>,
        notify: Arc<Notify>,
        sender: Option<Arc<dyn Fn(TypedAnyStreamEvent) + Send + Sync>>,
    ) -> Self {
        Self { provider, slot, notify, sender }
    }
}

#[async_trait]
impl Node<ReActState> for TitleNode {
    fn id(&self) -> &str { "title" }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), GraphError> {
        let recent = state
            .messages
            .iter()
            .rev()
            .take(6)
            .cloned()
            .collect::<Vec<_>>();

        let system = Message::system(
            "You are a session titler. Given a conversation, generate a short title (≤50 characters, Chinese OK). Output ONLY the title, no quotes or explanation.".to_string(),
        );
        let user = Message::user("Give this conversation a short title.");

        let messages = std::iter::once(system)
            .chain(recent.into_iter().rev())
            .chain(std::iter::once(user))
            .collect::<Vec<_>>();

        let provider = Arc::clone(&self.provider);
        let slot = Arc::clone(&self.slot);
        let notify = Arc::clone(&self.notify);
        let sender = self.sender.clone();

        tokio::spawn(async move {
            let result = async {
                let client = provider.create_client(provider.default_model())?;
                let response = client.invoke(&messages).await?;
                let title_raw = response.content.trim().to_string();
                Ok::<_, GraphError>(clamp_summary_chars(&title_raw))
            }
            .await;

            match result {
                Ok(title) => {
                    // Send the title event FIRST so the CLI's on_event callback
                    // (which sets pending_title) runs before the runner's
                    // wait_for_title wakes up and emits the finalize sentinel.
                    if let Some(sender) = sender {
                        let snapshot = ReActState {
                            summary: Some(title.clone()),
                            ..Default::default()
                        };
                        sender(TypedAnyStreamEvent::React(StreamEvent::Updates {
                            node_id: "title".to_string(),
                            state: snapshot,
                            namespace: None,
                        }));
                    }
                    // Then set the slot and signal the runner.
                    let _ = slot.set(title);
                    notify.notify_one();
                }
                Err(e) => {
                    warn!("Title generation failed: {e}");
                    // Still notify so the runner doesn't wait the full timeout
                    // for a title that will never arrive.
                    notify.notify_one();
                }
            }
        });

        Ok((state, Next::Node("think".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_short_string_unchanged() {
        assert_eq!(clamp_summary_chars("hello"), "hello");
    }

    #[test]
    fn test_clamp_exact_50_chars_unchanged() {
        let exact_50 = "a".repeat(50);
        assert_eq!(clamp_summary_chars(&exact_50), exact_50);
        assert_eq!(exact_50.len(), 50);
    }

    #[test]
    fn test_clamp_truncates_long_string() {
        let long_string = "a".repeat(60);
        let result = clamp_summary_chars(&long_string);
        assert_eq!(result.len(), 50);
        assert_eq!(result, "a".repeat(47) + "...");
    }

    #[test]
    fn test_clamp_unicode_chars() {
        let chinese = "这是一段中文测试文字，每个字符算一个字";
        let result = clamp_summary_chars(chinese);
        assert_eq!(result, chinese);
        
        let long_chinese = "这是一段很长的中文测试文字，每个字符算一个字，会被截断显示还有更多的字加上去超过五十个字符的限制，需要添加更多的中文字符来确保总长度超过五十个字符";
        let result = clamp_summary_chars(long_chinese);
        assert_eq!(result.chars().count(), 50);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_clamp_empty_string() {
        assert_eq!(clamp_summary_chars(""), "");
    }

    #[test]
    fn test_clamp_single_char_over_50() {
        let single_over = "a".repeat(51);
        let result = clamp_summary_chars(&single_over);
        assert_eq!(result.len(), 50);
        assert_eq!(result, "a".repeat(47) + "...");
    }
}
