//! Incremental thinking-tag parser for LLM streaming output (`<think>` / `</think>`).
//!
//! Used by both `ChatOpenAI` and `ChatOpenAICompat` to separate reasoning
//! content from final assistant replies during streaming.

pub const THINKING_START: &str = "\u{3c}think\u{3e}";
pub const THINKING_END: &str = "\u{3c}/think\u{3e}";

/// Hermes-aligned reasoning-tag open/close pairs (`cli.py:strip_reasoning_tags`).
///
/// OpenAI o1/o-series, Anthropic extended-thinking, DeepSeek R1, and a
/// handful of Chinese base models all surface their chain-of-thought
/// under different tag names. The persisted chat history must not
/// include any of these, otherwise the next replay leaks the previous
/// turn's reasoning. Open and close variants are kept separate so that
/// a model that forgets a closing tag (only an opener appears in the
/// stream) can still be sanitised to its opening point.
pub const REASONING_TAGS: &[(&str, &str)] = &[
    (THINKING_START, THINKING_END),
    ("\u{3c}thinking\u{3e}", "\u{3c}/thinking\u{3e}"),
    ("\u{3c}reasoning\u{3e}", "\u{3c}/reasoning\u{3e}"),
    ("\u{3c}REASONING_SCRATCHPAD\u{3e}", "\u{3c}/REASONING_SCRATCHPAD\u{3e}"),
    ("\u{3c}thought\u{3e}", "\u{3c}/thought\u{3e}"),
];

/// Hermes-aligned tool/function-call tag pairs (`cli.py:strip_tool_tags`).
///
/// Some tool-emitting providers (notably the in-house JSON-mode
/// shim used by the background-review harness) wrap raw tool call
/// payloads in legacy XML tags instead of the OpenAI native
/// `tool_calls` field. Those tags must not survive into the
/// persisted history because re-playing them would re-fire the
/// tool.
pub const TOOL_TAGS: &[(&str, &str)] = &[
    ("\u{3c}tool_call\u{3e}", "\u{3c}/tool_call\u{3e}"),
    ("\u{3c}tool_calls\u{3e}", "\u{3c}/tool_calls\u{3e}"),
    ("\u{3c}tool_result\u{3e}", "\u{3c}/tool_result\u{3e}"),
    ("\u{3c}function_call\u{3e}", "\u{3c}/function_call\u{3e}"),
    ("\u{3c}function_calls\u{3e}", "\u{3c}/function_calls\u{3e}"),
];

/// Segment produced by the incremental parser.
#[derive(Debug)]
pub enum ThinkingSegment {
    /// Normal assistant message content.
    Message(String),
    /// Reasoning/thinking content (inside thinking tags).
    Thinking(String),
}

/// Removes thinking-tag blocks from a complete string.
///
/// Used to produce the final stored `content` after streaming completes.
pub fn strip_thinking_tags(s: &str) -> String {
    strip_reasoning_and_tool_tags(s)
}

/// Removes both reasoning (`<think>`/`<thinking>`/...) and tool
/// (`<tool_call>`/...) tag blocks from a complete string.
///
/// Handles three termination cases per pair, mirroring Hermes'
/// `_strip_paired_tags`:
///   1. **closed** — `<tag>body</tag>` removed entirely,
///   2. **unterminated** — `<tag>...end-of-string` removed down to
///      the opener (matches Hermes' "drop the rest"),
///   3. **orphan close** — `</tag>` without an opener is dropped
///      too, so a stray closing tag does not leak.
///
/// The original [`strip_thinking_tags`] is preserved as a thin
/// wrapper for backwards compatibility.
pub fn strip_reasoning_and_tool_tags(s: &str) -> String {
    let mut out = strip_paired_tags(s, REASONING_TAGS);
    out = strip_paired_tags(&out, TOOL_TAGS);
    out
}

fn strip_paired_tags(s: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        // Find the earliest opener across all pairs.
        let next_open = pairs
            .iter()
            .filter_map(|(open, _)| rest.find(open).map(|i| (i, open)))
            .min_by_key(|(i, _)| *i);
        let Some((idx, open)) = next_open else {
            out.push_str(rest);
            return out;
        };
        // Drop orphan closes (any pair's closer before `idx`).
        let mut scan = &rest[..idx];
        let mut dropped_orphans = String::new();
        for (_, close) in pairs {
            while let Some(j) = scan.find(close) {
                dropped_orphans.push_str(&scan[..j]);
                scan = &scan[j + close.len()..];
            }
        }
        out.push_str(&dropped_orphans);
        out.push_str(scan);
        // Advance past the opener.
        let after_open = &rest[idx + open.len()..];
        // Find the matching closer.
        let close_for_open = pairs
            .iter()
            .find(|(o, _)| *o == *open)
            .map(|(_, c)| *c)
            .unwrap_or("");
        match after_open.find(close_for_open) {
            Some(j) => {
                rest = &after_open[j + close_for_open.len()..];
            }
            None => {
                // Unterminated: drop the rest of the string entirely.
                return out;
            }
        }
    }
}

/// Extracts text inside thinking tags from a complete string.
///
/// Returns `None` if no thinking blocks found.
pub fn collect_thinking_tags(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find(THINKING_START) {
        rest = &rest[start + THINKING_START.len()..];
        if let Some(end) = rest.find(THINKING_END) {
            out.push_str(&rest[..end]);
            rest = &rest[end + THINKING_END.len()..];
        } else {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[derive(Clone, Copy)]
enum ThinkingParseState {
    Outside,
    Inside,
}

/// Incremental parser for thinking tags in streamed content deltas.
///
/// Feed each content delta via [`Self::feed`], which returns parsed segments.
/// Call [`Self::flush`] at end-of-stream to drain any remaining buffer.
pub struct ThinkingTagParser {
    buf: String,
    state: ThinkingParseState,
}

impl ThinkingTagParser {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            state: ThinkingParseState::Outside,
        }
    }

    /// Feed an incremental content delta. Returns zero or more parsed segments.
    ///
    /// The parser buffers partial tag matches across calls, so it is safe to
    /// split an opening tag across two deltas (e.g. `"<thi"` then `"nk>rest"`).
    pub fn feed(&mut self, delta: &str) -> Vec<ThinkingSegment> {
        let mut segments = Vec::new();
        if delta.is_empty() {
            return segments;
        }
        self.buf.push_str(delta);

        loop {
            match self.state {
                ThinkingParseState::Outside => {
                    if let Some(i) = self.buf.find(THINKING_START) {
                        let before = self.buf[..i].to_string();
                        if !before.is_empty() {
                            segments.push(ThinkingSegment::Message(before));
                        }
                        self.buf = self.buf[i + THINKING_START.len()..].to_string();
                        self.state = ThinkingParseState::Inside;
                    } else {
                        let keep = self
                            .buf
                            .len()
                            .saturating_sub(THINKING_START.len().saturating_sub(1));
                        // Ensure keep is on a valid UTF-8 character boundary
                        let keep = self.buf.floor_char_boundary(keep);
                        let to_send = self.buf[..keep].to_string();
                        self.buf = self.buf[keep..].to_string();
                        if !to_send.is_empty() {
                            segments.push(ThinkingSegment::Message(to_send));
                        }
                        break;
                    }
                }
                ThinkingParseState::Inside => {
                    if let Some(i) = self.buf.find(THINKING_END) {
                        let inside = self.buf[..i].to_string();
                        if !inside.is_empty() {
                            segments.push(ThinkingSegment::Thinking(inside));
                        }
                        self.buf = self.buf[i + THINKING_END.len()..].to_string();
                        self.state = ThinkingParseState::Outside;
                    } else {
                        let keep = self
                            .buf
                            .len()
                            .saturating_sub(THINKING_END.len().saturating_sub(1));
                        // Ensure keep is on a valid UTF-8 character boundary
                        let keep = self.buf.floor_char_boundary(keep);
                        let to_send = self.buf[..keep].to_string();
                        self.buf = self.buf[keep..].to_string();
                        if !to_send.is_empty() {
                            segments.push(ThinkingSegment::Thinking(to_send));
                        }
                        break;
                    }
                }
            }
        }
        segments
    }

    /// Flush remaining buffer at end-of-stream.
    pub fn flush(self) -> Option<ThinkingSegment> {
        if self.buf.is_empty() {
            return None;
        }
        match self.state {
            ThinkingParseState::Outside => Some(ThinkingSegment::Message(self.buf)),
            ThinkingParseState::Inside => Some(ThinkingSegment::Thinking(self.buf)),
        }
    }
}

impl Default for ThinkingTagParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_thinking_tags_removes_blocks() {
        assert_eq!(strip_thinking_tags("hello"), "hello");
        let with_block = format!("a {}think{} b", THINKING_START, THINKING_END);
        assert_eq!(strip_thinking_tags(&with_block), "a  b");
        let only_block = format!("{}only{}", THINKING_START, THINKING_END);
        assert_eq!(strip_thinking_tags(&only_block), "");
    }

    #[test]
    fn collect_thinking_tags_extracts_inner_text() {
        let tagged = format!(
            "before {}alpha{} middle {}beta{}",
            THINKING_START, THINKING_END, THINKING_START, THINKING_END
        );
        assert_eq!(collect_thinking_tags(&tagged).as_deref(), Some("alphabeta"));
        assert_eq!(collect_thinking_tags("plain text"), None);
    }

    #[test]
    fn parser_handles_split_tag() {
        let mut p = ThinkingTagParser::new();
        let start: String = THINKING_START.chars().take(4).collect();
        let rest_start: String = THINKING_START.chars().skip(4).collect();
        let segs = p.feed(&start);
        assert!(segs.is_empty());
        let segs2 = p.feed(&format!("{}inner{}", rest_start, THINKING_END));
        assert!(
            segs2
                .iter()
                .any(|s| matches!(s, ThinkingSegment::Thinking(t) if t == "inner")),
            "expected Thinking(inner), got {:?}",
            segs2
        );
    }

    #[test]
    fn parser_flush_outside() {
        let mut p = ThinkingTagParser::new();
        p.feed("tail");
        match p.flush() {
            Some(ThinkingSegment::Message(s)) => assert_eq!(s, "tail"),
            other => panic!("expected Message(tail), got {:?}", other),
        }
    }

    #[test]
    fn parser_flush_inside() {
        let mut p = ThinkingTagParser::new();
        let partial = format!("{}rest", THINKING_START);
        p.feed(&partial);
        match p.flush() {
            Some(ThinkingSegment::Thinking(s)) => assert_eq!(s, "rest"),
            other => panic!("expected Thinking(rest), got {:?}", other),
        }
    }
}
