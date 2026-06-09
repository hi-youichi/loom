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

use loom_llm::error::AgentError;
use loom_graph::Next;
use loom_llm::{LlmHeaders, LlmProvider};
use loom_llm::message::Message;
use loom_types::state::ReActState;
use loom_graph::Node;

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
    #[allow(dead_code)]
    headers: Option<LlmHeaders>,
}

impl TitleNode {
    pub fn new(provider: Arc<dyn LlmProvider>, headers: Option<LlmHeaders>) -> Self {
        Self { provider, headers }
    }
}

#[async_trait]
impl Node<ReActState> for TitleNode {
    fn id(&self) -> &str { "title" }

    async fn run(&self, mut state: ReActState) -> Result<(ReActState, Next), AgentError> {
        // Build messages for title generation
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

        let client = self.provider.create_client(self.provider.default_model())?;
        let response = client.invoke(&messages).await?;
        let title_raw = response.content.trim().to_string();

        let title = clamp_summary_chars(&title_raw);
        state.summary = Some(title);

        Ok((state, Next::Node("think".into())))
    }
}
