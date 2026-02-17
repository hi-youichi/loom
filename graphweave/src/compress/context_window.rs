//! Token estimation and overflow detection for context window.
//!
//! Uses a heuristic (~4 chars per token) and, when available, hybrid strategy
//! with last LLM usage + delta for messages after last think.

use crate::message::Message;

/// Heuristic: approximate characters per token for English/mixed text (used by `estimate_tokens`).
const CHARS_PER_TOKEN: u32 = 4;

/// Heuristic token estimate: ~4 characters per token.
pub fn estimate_tokens(messages: &[Message]) -> u32 {
    let total: usize = messages
        .iter()
        .map(|m| match m {
            Message::System(s) | Message::User(s) | Message::Assistant(s) => s.len(),
        })
        .sum();
    (total / CHARS_PER_TOKEN as usize) as u32
}

/// Input for overflow check: only the fields needed to decide if context overflows.
///
/// Constructed by the caller (e.g. from `ReActState` + `CompactionConfig`); this module
/// does not depend on those types.
#[derive(Debug)]
pub struct ContextWindowCheck<'a> {
    /// Conversation messages (used for token estimate or delta after last think).
    pub messages: &'a [Message],
    /// Last LLM usage (prompt_tokens, completion_tokens) when available for hybrid estimate.
    pub usage: Option<(u32, u32)>,
    /// Message count at last Think; messages after this index use delta estimate.
    pub message_count_after_last_think: Option<usize>,
    /// Maximum context size in tokens.
    pub max_context_tokens: u32,
    /// Tokens to reserve for generation.
    pub reserve_tokens: u32,
}

/// Hybrid overflow check: use last LLM usage + estimated delta for new messages when available.
///
/// Only requires the fields in `ContextWindowCheck`; no dependency on `ReActState` or `CompactionConfig`.
pub fn is_overflow(input: &ContextWindowCheck<'_>) -> bool {
    // Current token count: hybrid (usage + delta) when we have last-Think usage and message count, else full estimate.
    let current = match (input.usage, input.message_count_after_last_think) {
        (Some((prompt, completion)), Some(count)) if count <= input.messages.len() => {
            let base = prompt + completion; // Tokens consumed by the last Think round (from provider).
            let delta = estimate_tokens(&input.messages[count..]); // Messages added after that round.
            base + delta
        }
        _ => estimate_tokens(input.messages), // No usage or count → estimate entire history.
    };
    // Overflow when context + reserve for generation exceeds limit.
    current + input.reserve_tokens > input.max_context_tokens
}

#[cfg(test)]
mod tests {
    //! Unit tests as executable specification for `estimate_tokens` and `is_overflow`.
    //!
    //! **estimate_tokens(messages)**  
    //! Sums the character length of all System/User/Assistant message contents, then divides by 4
    //! (heuristic: ~4 chars per token). Uses integer division, so e.g. 10 chars → 2 tokens.
    //!
    //! **is_overflow(input)**  
    //! Computes "current" token count:
    //! - If we have both `usage` (prompt_tokens, completion_tokens) and `message_count_after_last_think`:
    //!   current = (prompt_tokens + completion_tokens) + estimate_tokens(messages[count..])
    //!   (hybrid: real usage for the last Think round + estimated delta for messages added after).
    //! - Otherwise: current = estimate_tokens(messages) (pure heuristic).
    //! Overflow when: current + reserve_tokens > max_context_tokens.

    use crate::message::Message;

    use super::*;

    // --- estimate_tokens ---

    #[test]
    fn estimate_tokens_empty_is_zero() {
        // Empty slice: sum of lengths = 0, 0 / 4 = 0.
        assert_eq!(estimate_tokens(&[]), 0);
    }

    #[test]
    fn estimate_tokens_uses_four_chars_per_token() {
        // Formula: total_chars / 4 (integer division). 8 chars → 8/4 = 2 tokens.
        let msgs = vec![Message::User("12345678".to_string())];
        assert_eq!(estimate_tokens(&msgs), 2);
    }

    #[test]
    fn estimate_tokens_sums_all_messages() {
        // All message types (System, User, Assistant) contribute their string length.
        // 2 + 4 + 4 = 10 chars → 10/4 = 2 tokens.
        let msgs = vec![
            Message::System("ab".to_string()),
            Message::User("cdef".to_string()),
            Message::Assistant("ghij".to_string()),
        ];
        assert_eq!(estimate_tokens(&msgs), 2);
    }

    // --- is_overflow ---

    #[test]
    fn is_overflow_without_usage_uses_estimate_only() {
        // No usage / no message_count_after_last_think → current = estimate_tokens(messages).
        // 400 chars → 100 tokens. Overflow when current + reserve > max: 100 + 10 = 110 > 100 → true.
        let messages = vec![Message::User("x".repeat(400))];
        let input = ContextWindowCheck {
            messages: &messages,
            usage: None,
            message_count_after_last_think: None,
            max_context_tokens: 100,
            reserve_tokens: 10,
        };
        assert!(is_overflow(&input));
    }

    #[test]
    fn is_overflow_under_limit_no_overflow() {
        // Same formula; 100 chars → 25 tokens. 25 + 10 = 35 < 1000 → no overflow.
        let messages = vec![Message::User("x".repeat(100))];
        let input = ContextWindowCheck {
            messages: &messages,
            usage: None,
            message_count_after_last_think: None,
            max_context_tokens: 1000,
            reserve_tokens: 10,
        };
        assert!(!is_overflow(&input));
    }

    #[test]
    fn is_overflow_hybrid_uses_usage_plus_delta() {
        // Hybrid path: usage = Some((50, 10)), message_count_after_last_think = Some(1).
        // current = (50 + 10) + estimate_tokens(messages[1..]) = 60 + estimate(["new"]) = 60 + (3/4) = 60 + 0 = 60.
        // 60 + 10 = 70 < 100 → no overflow. Demonstrates that messages after last Think are estimated, not double-counted.
        let messages = vec![
            Message::User("old".to_string()),
            Message::User("new".to_string()),
        ];
        let input = ContextWindowCheck {
            messages: &messages,
            usage: Some((50, 10)),
            message_count_after_last_think: Some(1),
            max_context_tokens: 100,
            reserve_tokens: 10,
        };
        assert!(!is_overflow(&input));
    }
}
