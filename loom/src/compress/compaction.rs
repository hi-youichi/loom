//! Conversation compaction: prune old tool results and compact history via LLM summarization.
//!
//! Capabilities:
//! - **prune**: Replace old tool results beyond a token limit with a placeholder to control context length.
//! - **compact**: Summarize earlier messages into one System message via LLM and keep the most recent N as-is.

use tracing::{debug, info};

use crate::error::AgentError;
use crate::llm::LlmClient;
use crate::message::Message;
use crate::tool_source::ToolCallContent;

use super::config::CompactionConfig;
use super::context_window::estimate_tokens;

/// Placeholder text used to replace pruned tool results in messages.
pub const PRUNE_PLACEHOLDER: &str = "[Old tool result cleared]";

/// Returns true if the message is a User message in tool-result form (`Tool xxx returned: ...`).
fn is_tool_result_message(m: &Message) -> bool {
    match m {
        Message::Tool { .. } => true,
        Message::User(c) => {
            let s = c.as_text();
            s.starts_with("Tool ") && s.contains(" returned: ")
        }
        _ => false,
    }
}

/// Replace old tool results beyond the keep-token limit with a placeholder.
///
/// Traverses messages from newest to oldest, accumulating tokens for tool-result messages only.
/// Results that push the total over `prune_keep_tokens` are marked for pruning. If the total
/// prunable tokens would be less than `prune_minimum`, no change is made.
pub fn prune(messages: Vec<Message>, config: &CompactionConfig) -> Vec<Message> {
    // Skip when pruning is off or keep limit is zero
    if !config.prune || config.prune_keep_tokens == 0 {
        let reason = if !config.prune {
            "prune_disabled"
        } else {
            "keep_tokens_zero"
        };
        debug!(reason, "prune skipped");
        return messages;
    }
    // Only apply pruning if we would remove at least this many tokens (avoids tiny, frequent edits)
    let min = config.prune_minimum.unwrap_or(20_000);

    let mut total: u32 = 0; // accumulated tool-result tokens (newest to oldest)
    let mut pruned: u32 = 0; // total tokens marked for pruning
    let mut to_prune = Vec::new(); // indices of messages to replace with placeholder

    // Walk from newest to oldest; once total exceeds keep, mark older tool results for pruning
    for (i, m) in messages.iter().enumerate().rev() {
        if is_tool_result_message(m) {
            let tok = estimate_tokens(std::slice::from_ref(m));
            total += tok;
            if total > config.prune_keep_tokens {
                pruned += tok;
                to_prune.push(i);
            }
        }
    }

    let to_prune_count = to_prune.len();
    debug!(
        tool_result_tokens_total = total,
        pruned,
        to_prune_count,
        prune_keep_tokens = config.prune_keep_tokens,
        prune_minimum = min,
        "prune scan done"
    );

    // Do nothing if we would prune fewer than min tokens
    if pruned < min {
        debug!(pruned, min, "prune skipped: below minimum");
        return messages;
    }

    // Replace marked messages with placeholder
    let mut out = messages;
    for i in &to_prune {
        match out.get_mut(*i) {
            Some(Message::User(_)) => {
                out[*i] = Message::user(crate::message::UserContent::Text(PRUNE_PLACEHOLDER.to_string()));
            }
            Some(Message::Tool { tool_call_id, .. }) => {
                let id = tool_call_id.clone();
                out[*i] = Message::Tool { tool_call_id: id, content: ToolCallContent::text(PRUNE_PLACEHOLDER.to_string(),
                ) };
            }
            _ => {}
        }
    }
    info!(
        replaced_count = to_prune_count,
        tokens_cleared = pruned,
        "prune applied"
    );
    out
}

/// Summarize earlier messages into one System message via LLM and keep the most recent N as-is.
///
/// Output is `[one summary System message] + [last compact_keep_recent original messages]`.
pub async fn compact(
    messages: &[Message],
    llm: &dyn LlmClient,
    config: &CompactionConfig,
) -> Result<Vec<Message>, AgentError> {
    let keep = config.compact_keep_recent;
    let message_count = messages.len();
    if message_count <= keep {
        debug!(
            message_count,
            keep, "compact skipped: message count within keep"
        );
        return Ok(messages.to_vec());
    }
    // Split: older messages to summarize, last `keep` messages to keep verbatim
    let split = messages.len().saturating_sub(keep);
    let (to_summarize, recent) = messages.split_at(split);
    let estimated_tokens_to_summarize = estimate_tokens(to_summarize);

    info!(
        to_summarize_count = split,
        keep_recent = keep,
        estimated_tokens_to_summarize,
        "compact summarization starting"
    );

    // Ask LLM to summarize the older part
    let prompt = build_summary_prompt(to_summarize);
    let summary_msgs = vec![Message::user(crate::message::UserContent::Text(prompt))];
    let response = llm.invoke(&summary_msgs).await?;
    let content = response.content;

    // Prepend one System message with the summary, then the recent messages
    let summary = Message::System(format!("[Summary of earlier conversation]: {}", content));
    let mut out = vec![summary];
    out.extend(recent.iter().cloned());

    let input_messages = message_count;
    let output_messages = out.len();
    info!(
        input_messages,
        output_messages,
        summary_len = content.len(),
        kept_recent = keep,
        "compact applied"
    );
    Ok(out)
}

/// Build the prompt sent to the LLM: instructions on what to summarize, then the message list.
fn build_summary_prompt(msgs: &[Message]) -> String {
    // Instruction lines telling the LLM what to focus on
    let mut parts = vec![
        "Summarize the following conversation. Focus on:".to_string(),
        "- What was done".to_string(),
        "- What is being worked on".to_string(),
        "- Which files are involved".to_string(),
        "- What needs to be done next".to_string(),
        "".to_string(),
    ];
    // Append each message with a role prefix
    for m in msgs {
        match m {
            Message::System(s) => parts.push(format!("System: {}", s)),
            Message::User(c) => parts.push(format!("User: {}", c.as_text())),
            Message::Assistant(p) => {
                if p.tool_calls.is_empty() {
                    parts.push(format!("Assistant: {}", p.content));
                } else {
                    parts.push(format!(
                        "Assistant: {} [tool_calls: {}]",
                        p.content,
                        p.tool_calls
                            .iter()
                            .map(|tc| tc.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
            Message::Tool { content, .. } => parts.push(format!("Tool: {}", content.to_display_string())),
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use crate::message::Message;

    use super::*;

    fn tool_result_msg(name: &str, content: &str) -> Message {
        Message::User(crate::message::UserContent::Text(format!(
            "Tool {} returned: {}",
            name, content
        )))
    }

    #[test]
    fn is_tool_result_user_with_prefix() {
        assert!(is_tool_result_message(&Message::User(
            crate::message::UserContent::Text("Tool bash returned: ok".to_string())
        )));
    }

    #[test]
    fn is_tool_result_plain_user() {
        assert!(!is_tool_result_message(&Message::user("hello".to_string())));
    }

    #[test]
    fn is_tool_result_system() {
        assert!(!is_tool_result_message(&Message::System(
            "Tool x returned: y".to_string()
        )));
    }

    #[test]
    fn is_tool_result_assistant() {
        assert!(!is_tool_result_message(&Message::assistant(
            "Tool x returned: y".to_string()
        )));
    }

    #[test]
    fn build_summary_prompt_empty() {
        let p = build_summary_prompt(&[]);
        assert!(p.contains("Summarize the following conversation"));
        assert!(p.contains("What was done"));
    }

    #[test]
    fn build_summary_prompt_with_messages() {
        let msgs = vec![
            Message::System("sys msg".to_string()),
            Message::user("user msg".to_string()),
            Message::assistant("asst msg".to_string()),
        ];
        let p = build_summary_prompt(&msgs);
        assert!(p.contains("System: sys msg"));
        assert!(p.contains("User: user msg"));
        assert!(p.contains("Assistant: asst msg"));
    }

    #[test]
    fn prune_disabled_returns_unchanged() {
        let config = CompactionConfig {
            prune: false,
            prune_keep_tokens: 1000,
            ..Default::default()
        };
        let msgs = vec![
            Message::user("hi"),
            tool_result_msg("a", "data"),
        ];
        let out = prune(msgs.clone(), &config);
        assert_eq!(out.len(), msgs.len());
        assert!(matches!(&out[0], Message::User(UserContent::Text(s)) if s == "hi"));
        assert!(matches!(&out[1], Message::User(UserContent::Text(s)) if s.contains("Tool a returned:")));
    }

    #[test]
    fn prune_keep_tokens_zero_returns_unchanged() {
        let config = CompactionConfig {
            prune: true,
            prune_keep_tokens: 0,
            ..Default::default()
        };
        let msgs = vec![tool_result_msg("a", "x")];
        let out = prune(msgs.clone(), &config);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Message::User(UserContent::Text(s)) if s.contains("Tool a returned:")));
    }

    #[test]
    fn prune_no_tool_results_returns_unchanged() {
        let config = CompactionConfig {
            prune: true,
            prune_keep_tokens: 100,
            prune_minimum: Some(0),
            ..Default::default()
        };
        let msgs = vec![
            Message::user("hi".to_string()),
            Message::assistant("hello".to_string()),
        ];
        let out = prune(msgs.clone(), &config);
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], Message::User(UserContent::Text(s)) if s == "hi"));
        assert!(matches!(&out[1], Message::Assistant(p) if p.content == "hello"));
    }

    #[test]
    fn prune_replaces_old_tool_results_beyond_keep() {
        // Each tool result is "Tool X returned: " + 20 chars ≈ 40 chars = 10 tokens. Two = 20 total.
        // keep = 15: newest (10) kept, then older (10) pushes total to 20 > 15, so we prune the older (index 1).
        let config = CompactionConfig {
            prune: true,
            prune_keep_tokens: 15,
            prune_minimum: Some(0),
            ..Default::default()
        };
        let msgs = vec![
            Message::user("user".to_string()),
            tool_result_msg("old", "12345678901234567890"), // "Tool old returned: " + 20 chars
            tool_result_msg("new", "abcdefghijabcdefghij"), // "Tool new returned: " + 20 chars
        ];
        let out = prune(msgs, &config);
        assert_eq!(out.len(), 3);
        assert!(matches!(&out[0], Message::User(UserContent::Text(s)) if s == "user"));
        assert!(matches!(&out[1], Message::User(UserContent::Text(s)) if s == PRUNE_PLACEHOLDER));
        assert!(matches!(&out[2], Message::User(UserContent::Text(s)) if s.contains("Tool new returned:")));
    }

    #[test]
    fn prune_below_minimum_returns_unchanged() {
        let config = CompactionConfig {
            prune: true,
            prune_keep_tokens: 1,
            prune_minimum: Some(100_000), // would prune 1 token but min is 100k
            ..Default::default()
        };
        let msgs = vec![
            Message::user("x".to_string()),
            tool_result_msg("a", &"y".repeat(400)), // 100 tokens
        ];
        let out = prune(msgs.clone(), &config);
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], Message::User(UserContent::Text(s)) if s == "x"));
        assert!(matches!(&out[1], Message::User(UserContent::Text(s)) if s.contains("Tool a returned:")));
    }
}
