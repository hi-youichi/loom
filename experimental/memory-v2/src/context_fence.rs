//! Memory context fencing — wrap/scrub `<memory-context>` blocks.
//!
//! When memory is injected into a conversation turn (not just the system prompt),
//! it must be fenced with `<memory-context>` tags so downstream consumers can
//! distinguish recalled memory from new user input.  LLMs sometimes echo
//! these fenced blocks back in their streaming output — `StreamingContextScrubber`
//! strips them so they never reach the UI.
//!
//! Aligns with Hermes `agent/memory_manager.py` (`StreamingContextScrubber`,
//! `sanitize_context`, `build_memory_context_block`).
//!
//! # Usage
//! ```no_run
//! use memory_v2::context_fence::{StreamingContextScrubber, build_memory_context_block};
//!
//! // Wrap raw memory for injection
//! let block = build_memory_context_block("User likes coffee");
//!
//! // Scrub echoed blocks from streaming output
//! let mut scrubber = StreamingContextScrubber::new();
//! let visible = scrubber.feed("<memory-context>\nleaked\n</memory-context>visible");
//! let trailing = scrubber.flush();
//! ```

/// Tags used for fencing.
const OPEN_TAG: &str = "<memory-context>";
const CLOSE_TAG: &str = "</memory-context>";

/// One-shot sanitizer: strip fence tags, injected context blocks, and system
/// notes from a complete string.
///
/// Aligns with Hermes `sanitize_context` (`memory_manager.py:54-59`).
/// For streaming output use [`StreamingContextScrubber`] instead.
pub fn sanitize_context(text: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static FENCE_TAG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)</?\s*memory-context\s*>").unwrap());
    static INTERNAL_CONTEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<\s*memory-context\s*>.*?</\s*memory-context\s*>").unwrap()
    });
    static INTERNAL_NOTE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\[System note:\s*The following is recalled memory context,\s*NOT new user input\.\s*Treat as (?:informational background data|authoritative reference data[^\]]*)\.\]\s*",
        ).unwrap()
    });

    let text = INTERNAL_CONTEXT_RE.replace_all(text, "");
    let text = INTERNAL_NOTE_RE.replace_all(&text, "");
    let text = FENCE_TAG_RE.replace_all(&text, "");
    text.into_owned()
}

/// Wrap raw memory context in a fenced block with a system note.
///
/// If the input is already pre-wrapped (contains fence tags), it is stripped
/// first and a warning is logged.
///
/// Aligns with Hermes `build_memory_context_block` (`memory_manager.py:227-241`).
pub fn build_memory_context_block(raw_context: &str) -> String {
    if raw_context.trim().is_empty() {
        return String::new();
    }
    let clean = sanitize_context(raw_context);
    if clean != raw_context {
        tracing::warn!("memory provider returned pre-wrapped context; stripped");
    }
    format!(
        "<memory-context>\n\
         [System note: The following is recalled memory context, \
         NOT new user input. Treat as authoritative reference data — \
         this is the agent's persistent memory and should inform all responses.]\n\n\
         {clean}\n\
         </memory-context>"
    )
}

/// Stateful scrubber for streaming text that may contain split
/// `<memory-context>` spans.
///
/// The one-shot [`sanitize_context`] regex cannot survive chunk boundaries:
/// a `<memory-context>` opened in one delta and closed in a later delta
/// leaks its payload to the UI because the non-greedy block regex needs
/// both tags in one string.  This scrubber runs a small state machine
/// across deltas, holding back partial-tag tails and discarding
/// everything inside a span (including the system-note line).
///
/// # Usage
/// ```no_run
/// # use memory_v2::context_fence::StreamingContextScrubber;
/// let mut scrubber = StreamingContextScrubber::new();
/// let visible = scrubber.feed("some delta");
/// // ... emit visible to UI ...
/// let trailing = scrubber.flush();
/// ```
///
/// The scrubber is re-entrant per agent instance.  Callers building new
/// top-level responses (new turn) should create a fresh scrubber or call
/// [`reset`](Self::reset).
///
/// Aligns with Hermes `StreamingContextScrubber` (`memory_manager.py:62-225`).
pub struct StreamingContextScrubber {
    /// Whether we are currently inside a `<memory-context>...</memory-context>` span.
    in_span: bool,
    /// Held-back text that might be the start of an open/close tag.
    buf: String,
    /// Whether the last emitted text ended at a block boundary (newline or whitespace-only).
    at_block_boundary: bool,
}

impl StreamingContextScrubber {
    /// Create a fresh scrubber (not inside a span, no held buffer).
    pub fn new() -> Self {
        Self {
            in_span: false,
            buf: String::new(),
            at_block_boundary: true,
        }
    }

    /// Reset to initial state — drops any held buffer and span state.
    ///
    /// Call this when starting a new turn to prevent partial-tag tails from
    /// a previous turn from bleeding into the next.
    pub fn reset(&mut self) {
        self.in_span = false;
        self.buf.clear();
        self.at_block_boundary = true;
    }

    /// Feed a chunk of streaming text and return the visible (scrubbed) portion.
    ///
    /// Any trailing fragment that could be the start of an open/close tag
    /// is held back internally and surfaced on the next `feed()` call or
    /// emitted by [`flush`](Self::flush).
    pub fn feed(&mut self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        // Prepend held buffer
        let mut buf = std::mem::take(&mut self.buf);
        buf.push_str(text);

        let mut out = String::new();

        loop {
            if self.in_span {
                // Look for close tag (case-insensitive)
                match find_ci(&buf, CLOSE_TAG) {
                    Some(idx) => {
                        // Skip span content + close tag
                        buf = buf[idx + CLOSE_TAG.len()..].to_string();
                        self.in_span = false;
                    }
                    None => {
                        // Hold back potential partial close tag; drop the rest
                        let held = max_partial_suffix(&buf, CLOSE_TAG);
                        if held > 0 {
                            self.buf = buf[buf.len() - held..].to_string();
                        }
                        return out;
                    }
                }
            } else {
                match self.find_boundary_open_tag(&buf) {
                    Some(idx) => {
                        // Emit text before the tag, enter span
                        if idx > 0 {
                            self.append_visible(&mut out, &buf[..idx]);
                        }
                        buf = buf[idx + OPEN_TAG.len()..].to_string();
                        self.in_span = true;
                    }
                    None => {
                        // No open tag — hold back a potential partial open tag
                        let held = self.max_pending_open_suffix(&buf).or_else(|| {
                            if max_partial_suffix(&buf, OPEN_TAG) > 0 {
                                Some(max_partial_suffix(&buf, OPEN_TAG))
                            } else {
                                None
                            }
                        });
                        // Note: max_pending_open_suffix and max_partial_suffix
                        // can't both be non-zero simultaneously (pending checks
                        // for complete tag, partial checks for incomplete prefix).
                        match held {
                            Some(h) if h > 0 => {
                                let split = buf.len().saturating_sub(h);
                                let split = buf.floor_char_boundary(split);
                                self.append_visible(&mut out, &buf[..split]);
                                self.buf = buf[split..].to_string();
                            }
                            _ => {
                                self.append_visible(&mut out, &buf);
                            }
                        }
                        return out;
                    }
                }
            }
        }
    }

    /// Emit any held-back buffer at end-of-stream.
    ///
    /// Returns `None` if nothing was held back.
    ///
    /// If we're still inside an unterminated span the remaining content is
    /// discarded (safer: leaking partial memory context is worse than a
    /// truncated answer).  Otherwise the held-back partial-tag tail is
    /// emitted verbatim (it turned out not to be a real tag).
    pub fn flush(&mut self) -> Option<String> {
        if self.in_span {
            self.buf.clear();
            self.in_span = false;
            return None;
        }
        if self.buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buf))
        }
    }

    // ── Internal helpers ──────────────────────────────

    /// Find an opening fence only when it starts a block-like span.
    ///
    /// Returns the byte index of the `<` in `buf`, or `None`.
    fn find_boundary_open_tag(&self, buf: &str) -> Option<usize> {
        let buf_lower = buf.to_lowercase();
        let mut search_start = 0;
        loop {
            match buf_lower[search_start..].find(OPEN_TAG) {
                Some(rel_idx) => {
                    let idx = search_start + rel_idx;
                    if self.is_block_boundary(buf, idx) && Self::has_block_opener_suffix(buf, idx) {
                        return Some(idx);
                    }
                    search_start = idx + 1;
                }
                None => return None,
            }
        }
    }

    /// Hold a complete boundary tag until the following char confirms it.
    ///
    /// Returns `Some(len)` if `buf` ends with a complete `OPEN_TAG` at a
    /// block boundary (waiting for newline confirmation), `None` otherwise.
    fn max_pending_open_suffix(&self, buf: &str) -> Option<usize> {
        if !buf.to_lowercase().ends_with(OPEN_TAG) {
            return None;
        }
        let idx = buf.len() - OPEN_TAG.len();
        if !self.is_block_boundary(buf, idx) {
            return None;
        }
        Some(OPEN_TAG.len())
    }

    /// Check if the character after an open tag at `idx` is a newline
    /// (confirming it's a block opener, not inline prose).
    fn has_block_opener_suffix(buf: &str, idx: usize) -> bool {
        let after_idx = idx + OPEN_TAG.len();
        if after_idx >= buf.len() {
            return false;
        }
        buf[after_idx..].starts_with('\n') || buf[after_idx..].starts_with('\r')
    }

    /// Check if the open tag at `idx` is at a block boundary.
    ///
    /// A block boundary means the tag is either at the start of the stream
    /// (or after only whitespace from a block boundary) or follows a newline
    /// with only whitespace on that line.
    fn is_block_boundary(&self, buf: &str, idx: usize) -> bool {
        if idx == 0 {
            return self.at_block_boundary;
        }
        let preceding = &buf[..idx];
        match preceding.rfind('\n') {
            Some(last_newline) => preceding[last_newline + 1..].trim().is_empty(),
            None => self.at_block_boundary && preceding.trim().is_empty(),
        }
    }

    /// Append visible text to output and update block-boundary tracking.
    fn append_visible(&mut self, out: &mut String, text: &str) {
        if text.is_empty() {
            return;
        }
        out.push_str(text);
        self.update_block_boundary(text);
    }

    /// Update `at_block_boundary` based on the last emitted text.
    fn update_block_boundary(&mut self, text: &str) {
        match text.rfind('\n') {
            Some(last_newline) => {
                self.at_block_boundary = text[last_newline + 1..].trim().is_empty();
            }
            None => {
                self.at_block_boundary = self.at_block_boundary && text.trim().is_empty();
            }
        }
    }
}

impl Default for StreamingContextScrubber {
    fn default() -> Self {
        Self::new()
    }
}

// ── Free functions ────────────────────────────────────

/// Case-insensitive search for `needle` in `haystack`.
/// Returns the byte index of the first match, or `None`.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h_lower = haystack.to_lowercase();
    let n_lower = needle.to_lowercase();
    h_lower.find(&n_lower)
}

/// Return the length of the longest `buf` suffix that is a prefix of `tag`.
///
/// Case-insensitive.  Returns 0 if no suffix could start the tag.
///
/// E.g., for `buf = "hello <mem"` and `tag = "<memory-context>"`,
/// returns 4 (the `"<mem"` suffix is a prefix of the tag).
fn max_partial_suffix(buf: &str, tag: &str) -> usize {
    let tag_lower = tag.to_lowercase();
    let buf_lower = buf.to_lowercase();
    let max_check = buf_lower.len().min(tag_lower.len().saturating_sub(1));
    for i in (1..=max_check).rev() {
        let start = buf_lower.len() - i;
        if !buf_lower.is_char_boundary(start) {
            continue;
        }
        if tag_lower.starts_with(&buf_lower[start..]) {
            return i;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_context tests ──

    #[test]
    fn sanitize_strips_complete_block() {
        let leaked = "<memory-context>\n\
            [System note: The following is recalled memory context, NOT new \
            user input. Treat as informational background data.]\n\
            payload\n\
            </memory-context>\nVisible";
        let out = sanitize_context(leaked);
        assert_eq!(out.trim(), "Visible");
    }

    #[test]
    fn sanitize_strips_orphan_tags() {
        let out = sanitize_context("text <memory-context> more");
        assert_eq!(out, "text  more");
    }

    #[test]
    fn sanitize_preserves_plain_text() {
        assert_eq!(sanitize_context("hello world"), "hello world");
    }

    #[test]
    fn sanitize_case_insensitive() {
        let out = sanitize_context("<MEMORY-CONTEXT>x</Memory-Context>");
        assert_eq!(out.trim(), "");
    }

    // ── build_memory_context_block tests ──

    #[test]
    fn build_block_clean_input() {
        let out = build_memory_context_block("plain fact about user");
        assert!(out.contains("<memory-context>"));
        assert!(out.contains("</memory-context>"));
        assert!(out.contains("plain fact about user"));
        assert_eq!(out.matches("<memory-context>").count(), 1);
        assert_eq!(out.matches("</memory-context>").count(), 1);
    }

    #[test]
    fn build_block_empty_returns_empty() {
        assert_eq!(build_memory_context_block(""), "");
        assert_eq!(build_memory_context_block("   "), "");
    }

    #[test]
    fn build_block_strips_prewrapped() {
        let prewrapped = "<memory-context>\n[System note: ...]\n\nreal fact\n</memory-context>";
        let out = build_memory_context_block(prewrapped);
        // Pre-wrapped block is sanitized: the entire <memory-context>...</memory-context>
        // block is stripped, leaving only the bare text. The result is then
        // re-wrapped (so exactly one open + one close tag remain).
        assert_eq!(out.matches("<memory-context>").count(), 1);
        assert_eq!(out.matches("</memory-context>").count(), 1);
    }

    // ── StreamingContextScrubber basics ──

    #[test]
    fn scrubber_empty_input_returns_empty() {
        let mut s = StreamingContextScrubber::new();
        assert_eq!(s.feed(""), "");
        assert_eq!(s.flush(), None);
    }

    #[test]
    fn scrubber_plain_text_passes_through() {
        let mut s = StreamingContextScrubber::new();
        assert_eq!(s.feed("hello world"), "hello world");
        assert_eq!(s.flush(), None);
    }

    #[test]
    fn scrubber_complete_block_in_single_delta() {
        let mut s = StreamingContextScrubber::new();
        let leaked = "<memory-context>\n\
            [System note: The following is recalled memory context, NOT new \
            user input. Treat as informational background data.]\n\n\
            ## Honcho Context\nstale memory\n\
            </memory-context>\n\nVisible answer";
        let out = s.feed(leaked) + &s.flush().unwrap_or_default();
        assert_eq!(out, "\n\nVisible answer");
    }

    #[test]
    fn scrubber_open_close_in_separate_deltas() {
        let mut s = StreamingContextScrubber::new();
        let deltas = [
            "Hello\n",
            "<memory-context>\npayload ",
            "more payload\n",
            "</memory-context> world",
        ];
        let mut out = String::new();
        for d in &deltas {
            out.push_str(&s.feed(d));
        }
        out.push_str(&s.flush().unwrap_or_default());
        assert_eq!(out, "Hello\n world");
        assert!(!out.contains("payload"));
    }

    #[test]
    fn scrubber_realistic_fragmented_chunks() {
        let mut s = StreamingContextScrubber::new();
        let deltas = [
            "<memory-context>\n[System note: The following",
            " is recalled memory context, NOT new user input. \
             Treat as informational background data.]\n\n",
            "## Honcho Context\nstale memory\n",
            "</memory-context>\n\nVisible answer",
        ];
        let mut out = String::new();
        for d in &deltas {
            out.push_str(&s.feed(d));
        }
        out.push_str(&s.flush().unwrap_or_default());
        assert_eq!(out, "\n\nVisible answer");
        assert!(!out.contains("System note"));
        assert!(!out.contains("Honcho Context"));
        assert!(!out.contains("stale memory"));
    }

    #[test]
    fn scrubber_open_tag_split_across_two_deltas() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("pre \n<memory")
            + &s.feed("-context>\nleak</memory-context> post")
            + &s.flush().unwrap_or_default();
        assert_eq!(out, "pre \n post");
        assert!(!out.contains("leak"));
    }

    #[test]
    fn scrubber_open_tag_waits_for_newline_confirmation() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("pre \n<memory-context>")
            + &s.feed("\nleak</memory-context> post")
            + &s.flush().unwrap_or_default();
        assert_eq!(out, "pre \n post");
        assert!(!out.contains("leak"));
    }

    #[test]
    fn scrubber_close_tag_split_across_two_deltas() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("pre \n<memory-context>\nleak</memory")
            + &s.feed("-context> post")
            + &s.flush().unwrap_or_default();
        assert_eq!(out, "pre \n post");
        assert!(!out.contains("leak"));
    }

    // ── Partial-tag false positive tests ──

    #[test]
    fn scrubber_partial_open_tag_tail_emitted_on_flush() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("hello <mem") + &s.feed("ory other") + &s.flush().unwrap_or_default();
        assert_eq!(out, "hello <memory other");
    }

    #[test]
    fn scrubber_partial_tag_released_when_disambiguated() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("price < ") + &s.feed("10 dollars") + &s.flush().unwrap_or_default();
        assert_eq!(out, "price < 10 dollars");
    }

    #[test]
    fn scrubber_inline_tag_mention_not_scrubbed() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("In that previous `<memory")
            + &s.feed("-context>` block, ")
            + &s.feed("there was no matching fact.")
            + &s.flush().unwrap_or_default();
        assert_eq!(
            out,
            "In that previous `<memory-context>` block, there was no matching fact."
        );
    }

    #[test]
    fn scrubber_mid_sentence_mention_not_scrubbed() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("The <memory-context> tag name is documented here.")
            + &s.flush().unwrap_or_default();
        assert_eq!(out, "The <memory-context> tag name is documented here.");
    }

    #[test]
    fn scrubber_line_start_mention_without_close_not_scrubbed() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("Visible intro\n")
            + &s.feed("<memory-context> is the literal tag name mentioned here.")
            + &s.flush().unwrap_or_default();
        assert_eq!(
            out,
            "Visible intro\n<memory-context> is the literal tag name mentioned here."
        );
    }

    // ── Unterminated span tests ──

    #[test]
    fn scrubber_unterminated_span_drops_payload() {
        let mut s = StreamingContextScrubber::new();
        let out =
            s.feed("pre \n<memory-context>\nsecret never closed") + &s.flush().unwrap_or_default();
        assert_eq!(out, "pre \n");
        assert!(!out.contains("secret"));
    }

    #[test]
    fn scrubber_reset_clears_hung_span() {
        let mut s = StreamingContextScrubber::new();
        s.feed("pre <memory-context>half");
        s.reset();
        let out = s.feed("clean text") + &s.flush().unwrap_or_default();
        assert_eq!(out, "clean text");
    }

    // ── Case insensitivity ──

    #[test]
    fn scrubber_uppercase_tags_still_scrubbed() {
        let mut s = StreamingContextScrubber::new();
        let out = s.feed("<MEMORY-CONTEXT>\nsecret")
            + &s.feed("</Memory-Context>visible")
            + &s.flush().unwrap_or_default();
        assert_eq!(out, "visible");
        assert!(!out.contains("secret"));
    }

    // ── Cross-turn reset tests ──

    #[test]
    fn scrubber_reset_clears_held_partial_tag() {
        let mut s = StreamingContextScrubber::new();
        let out_turn_1 = s.feed("answer<memo");
        assert_eq!(out_turn_1, "answer");

        s.reset();

        let out_turn_2 = s.feed("<marker>fresh content") + &s.flush().unwrap_or_default();
        assert_eq!(out_turn_2, "<marker>fresh content");
    }

    #[test]
    fn scrubber_reset_clears_in_span_state() {
        let mut s = StreamingContextScrubber::new();
        s.feed("text\n<memory-context>secret-tail");
        s.reset();
        let out = s.feed("post-reset visible text") + &s.flush().unwrap_or_default();
        assert_eq!(out, "post-reset visible text");
    }

    // ── max_partial_suffix unit tests ──

    #[test]
    fn max_partial_suffix_full_match() {
        // Buffer ending with a 4-char prefix of "<memory-context>"
        assert_eq!(max_partial_suffix("hello <mem", OPEN_TAG), 4);
    }

    #[test]
    fn max_partial_suffix_no_match() {
        assert_eq!(max_partial_suffix("hello world", OPEN_TAG), 0);
    }

    #[test]
    fn max_partial_suffix_close_tag() {
        assert_eq!(max_partial_suffix("text</memo", CLOSE_TAG), 6);
    }
}
