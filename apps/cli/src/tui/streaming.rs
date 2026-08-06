//! Streaming output renderer for AI assistant responses.
//!
//! Provides [`StreamingCell`] for rendering incremental AI responses as they
//! arrive, with support for thinking state detection and code block tracking.
//!
//! # Example
//!
//! ```ignore
//! let mut cell = StreamingCell::new();
//! cell.append_text("Hello");
//! cell.append_text(", world!");
//! cell.render(area, buf);
//! let final_text = cell.finish();
//! ```

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Widget,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::tui::render::Renderable;

/// A streaming cell for rendering AI assistant responses incrementally.
///
/// Accumulates content as it arrives via [`append_text()`](Self::append_text),
/// tracks whether the AI is currently in a thinking phase or inside a code
/// block, and renders the current state with appropriate visual cues.
///
/// Call [`finish()`](Self::finish) to consume the cell and obtain the final
/// accumulated content once streaming is complete.
pub struct StreamingCell {
    /// Accumulated content received so far.
    content: String,
    /// Text received since the last [`flush()`](Self::flush) call.
    pending_text: String,
    /// Whether the AI is currently in a thinking phase.
    is_thinking: bool,
    /// Whether the cursor is inside a code block (``` ``` ```).
    in_code_block: bool,
}

impl StreamingCell {
    /// Create a new empty streaming cell.
    pub fn new() -> Self {
        Self {
            content: String::new(),
            pending_text: String::new(),
            is_thinking: false,
            in_code_block: false,
        }
    }

    /// Append a text delta to the accumulated content.
    ///
    /// This is called incrementally as new chunks arrive from the AI.
    /// The content is immediately available for rendering.
    pub fn append_text(&mut self, delta: &str) {
        self.content.push_str(delta);
        self.pending_text.push_str(delta);

        // Track thinking state transitions via heuristics
        if delta.contains('◌') || delta.contains("思考") {
            self.is_thinking = true;
        }
        if self.is_thinking && delta.contains('\n') && !delta.contains('◌') {
            self.is_thinking = false;
        }

        // Track code block state (``` delimiters)
        for line in delta.lines() {
            if line.trim().starts_with("```") {
                self.in_code_block = !self.in_code_block;
            }
        }
    }

    /// Whether the cell has received any content yet.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Consume the cell and return the final accumulated content.
    ///
    /// After calling this, the cell is dropped and the content is moved out.
    /// Use this when streaming is complete to obtain the full response text.
    pub fn finish(self) -> String {
        self.content
    }

    /// Flush pending text — clear the delta buffer.
    ///
    /// Called after a render cycle to indicate that the pending text has
    /// been displayed and no longer needs to be tracked separately.
    pub fn flush(&mut self) {
        self.pending_text.clear();
    }

    /// Whether the AI is currently in a thinking phase.
    pub fn is_thinking(&self) -> bool {
        self.is_thinking
    }

    /// Whether the cursor is inside a code block.
    pub fn in_code_block(&self) -> bool {
        self.in_code_block
    }

    /// Get a reference to the accumulated content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Get a reference to the pending (unflushed) text.
    pub fn pending_text(&self) -> &str {
        &self.pending_text
    }

    /// Set the thinking state explicitly.
    ///
    /// Allows the caller to override the heuristic-based thinking detection.
    pub fn set_thinking(&mut self, is_thinking: bool) {
        self.is_thinking = is_thinking;
    }
}

impl Default for StreamingCell {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for StreamingCell {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        // Build the block with appropriate title and border style
        let (title, border_style) = if self.is_thinking {
            (" Assistant (思考中) ", Style::default().fg(Color::Yellow))
        } else if self.in_code_block {
            (" Assistant (代码块) ", Style::default().fg(Color::Cyan))
        } else {
            (" Assistant ", Style::default().fg(Color::Green))
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        // Build display text: if thinking, prepend a spinner indicator
        let display_text = if self.content.is_empty() && self.is_thinking {
            "◌ 思考中...".to_string()
        } else if self.is_thinking {
            format!("◌ 思考中...\n{}", self.content)
        } else {
            self.content.clone()
        };

        let paragraph = Paragraph::new(display_text)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.content.is_empty() {
            return 3; // top border + 1 content line + bottom border
        }

        // Approximate wrapped line count: each line may wrap at `width` chars
        let wrapped_lines: u16 = self
            .content
            .lines()
            .map(|line| {
                let len = line.len() as u32;
                if len == 0 {
                    1
                } else {
                    ((len + width as u32 - 1) / width as u32) as u16 // ceiling division, safe from overflow
                }
            })
            .sum();

        let thinking_extra = if self.is_thinking && !self.content.is_empty() {
            1
        } else {
            0
        };

        wrapped_lines + 2 + thinking_extra // 2 for borders
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn test_new_is_empty() {
        let cell = StreamingCell::new();
        assert!(cell.is_empty());
        assert_eq!(cell.content(), "");
    }

    #[test]
    fn test_append_text_accumulates() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello, ");
        assert!(!cell.is_empty());
        assert_eq!(cell.content(), "Hello, ");

        cell.append_text("world!");
        assert_eq!(cell.content(), "Hello, world!");
    }

    #[test]
    fn test_finish_returns_content() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello, world!");
        let result = cell.finish();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_finish_drops_cell() {
        let cell = StreamingCell::new();
        let result = cell.finish();
        assert_eq!(result, "");
        // cell is moved, cannot be used after finish
    }

    #[test]
    fn test_flush_clears_pending() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello");
        assert_eq!(cell.pending_text(), "Hello");
        cell.flush();
        assert_eq!(cell.pending_text(), "");
        assert_eq!(cell.content(), "Hello");
    }

    #[test]
    fn test_thinking_state_heuristic() {
        let mut cell = StreamingCell::new();
        assert!(!cell.is_thinking());

        cell.append_text("◌ 思考中...");
        assert!(cell.is_thinking());

        cell.append_text("\nSome content");
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_code_block_detection() {
        let mut cell = StreamingCell::new();
        assert!(!cell.in_code_block());

        cell.append_text("```rust\n");
        assert!(cell.in_code_block());

        cell.append_text("let x = 1;\n");
        assert!(cell.in_code_block());

        cell.append_text("```\n");
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_nested_code_block_is_toggle() {
        let mut cell = StreamingCell::new();
        cell.append_text("```\n```\n```\n");
        // opened → closed → opened → true
        assert!(cell.in_code_block());
    }

    #[test]
    fn test_renderable_desired_height_empty() {
        let cell = StreamingCell::new();
        // Empty cell: 3 lines (border + 1 content + border)
        assert_eq!(cell.desired_height(80), 3);
    }

    #[test]
    fn test_renderable_desired_height_with_content() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello\nWorld");
        // 2 lines + 2 borders = 4
        assert_eq!(cell.desired_height(80), 4);
    }

    #[test]
    fn test_renderable_desired_height_thinking() {
        let mut cell = StreamingCell::new();
        cell.append_text("◌ 思考中...\nSome content");
        // 2 lines + 2 borders + 1 thinking extra = 5
        let height = cell.desired_height(80);
        assert_eq!(height, 5);
    }

    #[test]
    fn test_renderable_desired_height_wrapping() {
        let mut cell = StreamingCell::new();
        // A single long line that wraps at width=10
        cell.append_text("12345678901234567890");
        // 20 chars / 10 width = 2 wrapped lines + 2 borders = 4
        assert_eq!(cell.desired_height(10), 4);
    }

    #[test]
    fn test_render_empty_no_panic() {
        let cell = StreamingCell::new();
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);
        // Should not panic — border + empty content renders cleanly
    }

    #[test]
    fn test_render_with_content() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello, world!");
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);
        // Should not panic; content appears inside bordered block
    }

    #[test]
    fn test_render_thinking() {
        let mut cell = StreamingCell::new();
        cell.append_text("◌");
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);
        // Should not panic; "◌ 思考中..." prefix rendered
    }

    #[test]
    fn test_set_thinking_explicit() {
        let mut cell = StreamingCell::new();
        cell.set_thinking(true);
        assert!(cell.is_thinking());
        cell.set_thinking(false);
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_default_trait() {
        let cell = StreamingCell::default();
        assert!(cell.is_empty());
        assert_eq!(cell.content(), "");
    }

    #[test]
    fn test_content_after_append() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello");
        assert_eq!(cell.content(), "Hello");
        cell.append_text(", world!");
        assert_eq!(cell.content(), "Hello, world!");
    }

    #[test]
    fn test_pending_text_tracking() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello");
        assert_eq!(cell.pending_text(), "Hello");
        cell.append_text(", world!");
        assert_eq!(cell.pending_text(), "Hello, world!");
        cell.flush();
        assert_eq!(cell.pending_text(), "");
    }

    // -----------------------------------------------------------------------
    // Trait method signatures — Renderable default methods
    // -----------------------------------------------------------------------

    #[test]
    fn test_cursor_pos_default() {
        let cell = StreamingCell::new();
        let area = Rect::new(0, 0, 80, 24);
        // StreamingCell does not provide cursor positioning — always None
        assert_eq!(cell.cursor_pos(area), None);
    }

    #[test]
    fn test_cursor_style_default() {
        use crossterm::cursor::SetCursorStyle;
        let cell = StreamingCell::new();
        let area = Rect::new(0, 0, 80, 24);
        // StreamingCell uses the default cursor shape
        assert_eq!(
            cell.cursor_style(area),
            SetCursorStyle::DefaultUserShape
        );
    }

    // -----------------------------------------------------------------------
    // Boundary conditions
    // -----------------------------------------------------------------------

    #[test]
    fn test_append_empty_string() {
        let mut cell = StreamingCell::new();
        cell.append_text("");
        // Empty string should not change any state
        assert!(cell.is_empty());
        assert_eq!(cell.content(), "");
        assert_eq!(cell.pending_text(), "");
        assert!(!cell.is_thinking());
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_append_only_empty_strings() {
        let mut cell = StreamingCell::new();
        cell.append_text("");
        cell.append_text("");
        cell.append_text("");
        // Still empty after multiple empty appends
        assert!(cell.is_empty());
        assert_eq!(cell.content(), "");
    }

    #[test]
    fn test_desired_height_width_one() {
        let mut cell = StreamingCell::new();
        // A 60-char line wrapping at width=1 → 60 wrapped lines
        cell.append_text("abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz01234567");
        // 60 wrapped lines + 2 borders = 62
        assert_eq!(cell.desired_height(1), 62);
    }

    #[test]
    fn test_desired_height_exact_width_boundary() {
        let mut cell = StreamingCell::new();
        // Line exactly matches width: 20 chars / 20 width = 1 wrapped line
        cell.append_text("12345678901234567890");
        assert_eq!(cell.desired_height(20), 3); // 1 + 2 borders
    }

    #[test]
    fn test_desired_height_one_over_width() {
        let mut cell = StreamingCell::new();
        // Line one char over width: 21 chars / 20 width = 2 wrapped lines
        cell.append_text("123456789012345678901");
        assert_eq!(cell.desired_height(20), 4); // 2 + 2 borders
    }

    #[test]
    fn test_desired_height_max_width() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello, world!");
        // With a very wide area (but not u16::MAX — that would overflow
        // in `len + width - 1`), no wrapping needed
        assert_eq!(cell.desired_height(10000), 3); // 1 line + 2 borders
    }

    #[test]
    fn test_desired_height_overflow_edge() {
        // EDGE CASE: len + width - 1 previously overflowed u16 when both were
        // large (e.g., len=2, width=65535). Now uses u32 intermediate arithmetic.
        let mut cell = StreamingCell::new();
        cell.append_text("ab");
        // 2 chars / 65535 width = 1 wrapped line, + 2 borders = 3
        assert_eq!(cell.desired_height(u16::MAX), 3);
    }

    #[test]
    fn test_desired_height_single_char_line() {
        let mut cell = StreamingCell::new();
        cell.append_text("a");
        // Single char, no wrapping
        assert_eq!(cell.desired_height(80), 3); // 1 line + 2 borders
    }

    #[test]
    fn test_desired_height_many_empty_lines() {
        let mut cell = StreamingCell::new();
        cell.append_text("\n\n\n\n\n");
        // 5 empty lines + 2 borders = 7
        assert_eq!(cell.desired_height(80), 7);
    }

    // -----------------------------------------------------------------------
    // Error paths — known edge cases / potential panics
    // -----------------------------------------------------------------------

    #[test]
    #[should_panic(expected = "attempt to divide by zero")]
    fn test_desired_height_zero_width_panics() {
        // KNOWN BUG: desired_height(0) divides by zero in the ceiling-division
        // expression `(len + width - 1) / width` when width == 0.
        // A contentful cell triggers the division; the empty-cell fast-path
        // returns 3 and avoids the panic.
        let mut cell = StreamingCell::new();
        cell.append_text("content");
        cell.desired_height(0);
    }

    #[test]
    fn test_desired_height_zero_width_empty_is_safe() {
        // Empty cell takes the fast path and returns 3 without dividing.
        let cell = StreamingCell::new();
        assert_eq!(cell.desired_height(0), 3);
    }

    #[test]
    fn test_finish_then_cell_dropped_compile_check() {
        // Compile-time check: finish() consumes self, so the cell is moved.
        // Subsequent use is a compile error, not a runtime issue.
        let cell = StreamingCell::new();
        let _content = cell.finish();
        // Uncommenting the next line would not compile:
        // let _ = cell.is_empty();
    }

    // -----------------------------------------------------------------------
    // State transitions — thinking state machine
    // -----------------------------------------------------------------------

    #[test]
    fn test_thinking_triggered_by_chinese_keyword() {
        let mut cell = StreamingCell::new();
        assert!(!cell.is_thinking());
        // "思考" (Chinese for "thinking") should trigger thinking state
        cell.append_text("让我思考一下这个问题");
        assert!(cell.is_thinking());
    }

    #[test]
    fn test_thinking_toggle_cycle() {
        let mut cell = StreamingCell::new();
        // Start: not thinking
        assert!(!cell.is_thinking());

        // Trigger thinking via spinner
        cell.append_text("◌");
        assert!(cell.is_thinking());

        // End thinking via newline without spinner
        cell.append_text("\n");
        assert!(!cell.is_thinking());

        // Trigger thinking again
        cell.append_text("◌ 再思考...");
        assert!(cell.is_thinking());

        // End again
        cell.append_text("\noutput");
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_thinking_newline_without_spinner_ends_it() {
        let mut cell = StreamingCell::new();
        cell.append_text("◌ thinking");
        assert!(cell.is_thinking());

        // A delta containing \n but no ◌ should end thinking
        cell.append_text("\n");
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_thinking_newline_with_spinner_keeps_thinking() {
        let mut cell = StreamingCell::new();
        cell.append_text("◌");
        assert!(cell.is_thinking());

        // A delta containing both \n AND ◌ should keep thinking
        cell.append_text("still ◌\nthinking");
        assert!(cell.is_thinking());
    }

    #[test]
    fn test_thinking_ended_by_newline_without_spinner_after_chinese() {
        let mut cell = StreamingCell::new();
        cell.append_text("思考中");
        assert!(cell.is_thinking());

        // Newline without ◌ ends thinking even when triggered by Chinese keyword
        cell.append_text("\n");
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_thinking_set_false_after_explicit_set() {
        let mut cell = StreamingCell::new();
        cell.set_thinking(true);
        assert!(cell.is_thinking());

        // Explicit set_thinking(false) overrides heuristic
        cell.set_thinking(false);
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_thinking_not_triggered_by_normal_text() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello, world! This is normal text.");
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_thinking_not_triggered_by_other_symbols() {
        let mut cell = StreamingCell::new();
        cell.append_text("● ○ ◎");
        assert!(!cell.is_thinking());
    }

    // -----------------------------------------------------------------------
    // State transitions — code block tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_code_block_toggle_inside_line() {
        let mut cell = StreamingCell::new();
        // The code checks `line.trim().starts_with("```")` — it must be at
        // the *start* of the trimmed line, not just anywhere on the line.
        // "some text ```rust" does NOT start with "```".
        cell.append_text("some text ```rust code here");
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_code_block_triggers_only_when_line_starts_with_triple_backtick() {
        let mut cell = StreamingCell::new();
        // Only lines whose trimmed content starts with "```" toggle the state.
        // A line containing "```" in the middle does NOT trigger.
        cell.append_text("prefix ``` not a code block start");
        assert!(!cell.in_code_block());

        // A line starting with "```" does toggle
        cell.append_text("```rust\n");
        assert!(cell.in_code_block());
    }

    #[test]
    fn test_code_block_toggle_more_than_three_ticks() {
        let mut cell = StreamingCell::new();
        cell.append_text("````");
        // Starts with "```", so it toggles
        assert!(cell.in_code_block());

        cell.append_text("````");
        // Another ```` line also starts with "```", toggles back
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_code_block_not_triggered_by_single_backtick() {
        let mut cell = StreamingCell::new();
        cell.append_text("`inline code`");
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_code_block_not_triggered_by_double_backtick() {
        let mut cell = StreamingCell::new();
        cell.append_text("``double``");
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_code_block_thinking_simultaneous() {
        let mut cell = StreamingCell::new();
        // Both states active at the same time
        cell.append_text("◌");
        assert!(cell.is_thinking());

        // Append code block opener WITHOUT a newline first, so thinking persists
        cell.append_text("```rust");
        assert!(cell.in_code_block());
        assert!(cell.is_thinking()); // Still thinking (no \n without ◌)

        // Code block closes; thinking still active (no \n in this delta)
        cell.append_text("```");
        assert!(!cell.in_code_block());
        assert!(cell.is_thinking());

        // Newline ends thinking (no ◌ in this delta)
        cell.append_text("\n");
        assert!(!cell.is_thinking());
    }

    #[test]
    fn test_code_block_toggle_trimmed_line() {
        let mut cell = StreamingCell::new();
        cell.append_text("  ```rust");
        // trim() removes leading spaces, so "```rust" starts with "```"
        assert!(cell.in_code_block());
    }

    #[test]
    fn test_code_block_toggle_multiple_open_close() {
        let mut cell = StreamingCell::new();
        cell.append_text("```\ncontent\n```\n");
        assert!(!cell.in_code_block()); // closed

        cell.append_text("```\nmore\n```\n");
        assert!(!cell.in_code_block()); // closed again

        cell.append_text("```\n");
        assert!(cell.in_code_block()); // open again
    }

    // -----------------------------------------------------------------------
    // Render output verification
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_thinking_empty_shows_spinner() {
        let mut cell = StreamingCell::new();
        cell.set_thinking(true);
        assert!(cell.is_empty());

        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);

        // The buffer should contain the spinner indicator.
        // Note: ratatui inserts spaces between CJK double-width characters,
        // so "◌ 思考中..." appears as "◌ 思 考 中 ..." in the buffer.
        let rendered = buf_to_string(&buf, area);
        assert!(
            rendered.contains("◌"),
            "thinking empty cell should show spinner character, got: {rendered:?}"
        );
        assert!(
            rendered.contains('思'),
            "thinking empty cell should show '思考中' text, got: {rendered:?}"
        );
    }

    #[test]
    fn test_render_thinking_with_content_shows_prefix() {
        let mut cell = StreamingCell::new();
        cell.append_text("◌ reasoning\nHere is the answer");
        assert!(cell.is_thinking());

        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);

        let rendered = buf_to_string(&buf, area);
        // Note: ratatui inserts spaces between CJK double-width characters
        assert!(
            rendered.contains('思'),
            "thinking cell should show '思考中' prefix, got: {rendered:?}"
        );
        assert!(
            rendered.contains("Here is the answer"),
            "content should appear after thinking prefix, got: {rendered:?}"
        );
    }

    #[test]
    fn test_render_normal_no_thinking_prefix() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello, world!");

        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);

        let rendered = buf_to_string(&buf, area);
        // Normal cell should NOT show "◌ 思考中..."
        assert!(
            !rendered.contains("◌ 思考中..."),
            "normal cell should not show thinking prefix, got: {rendered:?}"
        );
        assert!(
            rendered.contains("Hello, world!"),
            "content should appear, got: {rendered:?}"
        );
    }

    #[test]
    fn test_render_with_code_block_content() {
        let mut cell = StreamingCell::new();
        cell.append_text("```rust\nlet x = 1;\n```\n");

        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);

        let rendered = buf_to_string(&buf, area);
        assert!(
            rendered.contains("```rust"),
            "code block markers should appear in rendered output, got: {rendered:?}"
        );
        assert!(
            rendered.contains("let x = 1;"),
            "code content should appear, got: {rendered:?}"
        );
    }

    #[test]
    fn test_render_multiple_append_interleaved_with_flush() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello");
        cell.flush();
        cell.append_text(", ");
        cell.flush();
        cell.append_text("world!");
        cell.flush();

        // After flushes, content is still accumulated
        assert_eq!(cell.content(), "Hello, world!");
        assert_eq!(cell.pending_text(), "");

        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);

        let rendered = buf_to_string(&buf, area);
        assert!(
            rendered.contains("Hello, world!"),
            "content after interleaved flush+append should appear, got: {rendered:?}"
        );
    }

    #[test]
    fn test_render_unicode_content() {
        let mut cell = StreamingCell::new();
        cell.append_text("你好，世界！🌍");

        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        cell.render(area, &mut buf);

        let rendered = buf_to_string(&buf, area);
        // Note: ratatui inserts spaces between CJK double-width characters,
        // so "你好，世界！" appears as "你 好 ， 世 界 ！" with spaces.
        assert!(
            rendered.contains('你'),
            "unicode content should render, got: {rendered:?}"
        );
        assert!(
            rendered.contains('界'),
            "unicode content should contain '世界', got: {rendered:?}"
        );
    }

    #[test]
    fn test_render_area_smaller_than_content() {
        let mut cell = StreamingCell::new();
        cell.append_text("Line 1\nLine 2\nLine 3\nLine 4\nLine 5");

        // Area only 2 lines tall — content should be clipped
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        // Should not panic
        cell.render(area, &mut buf);
    }

    #[test]
    fn test_render_area_zero_size() {
        let mut cell = StreamingCell::new();
        cell.append_text("Some content");

        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        // Should not panic — zero-area renders nothing
        cell.render(area, &mut buf);
    }

    // -----------------------------------------------------------------------
    // Content state — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_append_text_line_ends_with_newline() {
        let mut cell = StreamingCell::new();
        cell.append_text("Line 1\n");
        assert_eq!(cell.content(), "Line 1\n");
        cell.append_text("Line 2\n");
        assert_eq!(cell.content(), "Line 1\nLine 2\n");
    }

    #[test]
    fn test_append_text_trailing_newline_desired_height() {
        let mut cell = StreamingCell::new();
        cell.append_text("Line 1\n");
        // Rust's str::lines() treats a trailing newline as ending the last
        // line, not creating an empty line. So "Line 1\n".lines() returns
        // just ["Line 1"] — 1 line.
        assert_eq!(cell.desired_height(80), 3); // 1 + 2 borders
    }

    #[test]
    fn test_is_empty_after_append_non_empty() {
        let mut cell = StreamingCell::new();
        assert!(cell.is_empty());
        cell.append_text(" ");
        assert!(!cell.is_empty());
    }

    #[test]
    fn test_in_code_block_initial_state() {
        let cell = StreamingCell::new();
        assert!(!cell.in_code_block());
    }

    #[test]
    fn test_append_text_multiple_deltas_same_call() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello");
        cell.append_text(" ");
        cell.append_text("World");
        assert_eq!(cell.content(), "Hello World");
        assert_eq!(cell.pending_text(), "Hello World");
    }

    #[test]
    fn test_content_after_flush_preserved() {
        let mut cell = StreamingCell::new();
        cell.append_text("Hello");
        cell.flush();
        assert_eq!(cell.content(), "Hello");
        assert_eq!(cell.pending_text(), "");
    }

    // -----------------------------------------------------------------------
    // Integration-style: Renderable trait compliance
    // -----------------------------------------------------------------------

    #[test]
    fn test_streaming_cell_is_renderable() {
        // Verify that StreamingCell satisfies the Renderable trait bounds
        // by using it as a &dyn Renderable reference.
        fn takes_renderable(_r: &dyn Renderable) {}

        let cell = StreamingCell::new();
        takes_renderable(&cell);
        // Compile-time check passes
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Convert a ratatui buffer back to a string for assertion inspection.
    /// Reads all cells in the given area, stripping trailing whitespace per line.
    fn buf_to_string(buf: &Buffer, area: Rect) -> String {
        let mut result = String::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                if let Some(cell) = buf.cell((x, y)) {
                    line.push_str(cell.symbol());
                }
            }
            // Trim trailing spaces for readability
            let trimmed = line.trim_end().to_string();
            if !result.is_empty() || !trimmed.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(&trimmed);
            }
        }
        result
    }
}