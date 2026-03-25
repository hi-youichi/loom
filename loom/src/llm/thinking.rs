//! Incremental thinking-tag parser for LLM streaming output (`<think>` / `</think>`).
//!
//! Used by both `ChatOpenAI` and `ChatOpenAICompat` to separate reasoning
//! content from final assistant replies during streaming.

pub(crate) const THINKING_START: &str = "\u{3c}think\u{3e}";
pub(crate) const THINKING_END: &str = "\u{3c}/think\u{3e}";

/// Segment produced by the incremental parser.
#[derive(Debug)]
pub(crate) enum ThinkingSegment {
    /// Normal assistant message content.
    Message(String),
    /// Reasoning/thinking content (inside thinking tags).
    Thinking(String),
}

/// Removes thinking-tag blocks from a complete string.
///
/// Used to produce the final stored `content` after streaming completes.
pub(crate) fn strip_thinking_tags(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find(THINKING_START) {
        out.push_str(&rest[..start]);
        rest = &rest[start + THINKING_START.len()..];
        if let Some(end) = rest.find(THINKING_END) {
            rest = &rest[end + THINKING_END.len()..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

/// Extracts text inside thinking tags from a complete string.
///
/// Returns `None` if no thinking blocks found.
pub(crate) fn collect_thinking_tags(s: &str) -> Option<String> {
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
pub(crate) struct ThinkingTagParser {
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
            segs2.iter().any(|s| matches!(s, ThinkingSegment::Thinking(t) if t == "inner")),
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
