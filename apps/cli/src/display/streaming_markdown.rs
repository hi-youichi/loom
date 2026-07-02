//! Streaming markdown renderer for terminal output.
//!
//! A stateful line-buffering renderer that accumulates stream chunks and
//! renders complete lines with ANSI formatting. This handles the case where
//! markdown constructs (bold, italic, code blocks, etc.) are split across
//! multiple LLM stream chunks.

use loom_stream::{MessageChunk, MessageChunkKind};
use super::markdown::*;
use std::io::Write;

/// Stateful line-buffering markdown renderer for streaming output.
///
/// Accumulates character-by-character into a line buffer. When a newline
/// is received, the complete line is rendered with markdown → ANSI formatting.
/// Thinking chunks bypass the buffer and are directly dim-printed to stderr.
pub struct StreamingMarkdownRenderer {
    /// Current line buffer (accumulated until `\n`, then flushed).
    line_buf: String,
    /// Whether we are inside a fenced code block.
    in_code_block: bool,
    /// Language tag for the current code block.
    code_lang: String,
}

impl StreamingMarkdownRenderer {
    /// Creates a new renderer with empty state.
    pub fn new() -> Self {
        Self {
            line_buf: String::with_capacity(256),
            in_code_block: false,
            code_lang: String::new(),
        }
    }

    /// Process a single streaming chunk.
    ///
    /// - Thinking chunks are directly dim-printed to stderr (no markdown).
    /// - Message chunks are buffered line-by-line; each complete line is
    ///   rendered with markdown formatting and printed to stdout.
    pub fn push_chunk(&mut self, chunk: &MessageChunk) {
        if chunk.kind == MessageChunkKind::Thinking {
            eprint!("{}", super::terminal::dim(&chunk.content));
            let _ = std::io::stderr().flush();
            return;
        }

        // Message: line-buffer + markdown rendering
        for ch in chunk.content.chars() {
            if ch == '\n' {
                self.flush_line();
            } else if ch == '\r' {
                // Skip carriage return (handle \r\n gracefully)
            } else {
                self.line_buf.push(ch);
            }
        }
        let _ = std::io::stdout().flush();
    }

    /// Flush the current line buffer, rendering it with markdown formatting.
    fn flush_line(&mut self) {
        let line = std::mem::take(&mut self.line_buf);

        // Code block fence detection
        if line.trim_start().starts_with("```") {
            if self.in_code_block {
                print!("{}", format_code_block_end());
                self.in_code_block = false;
                self.code_lang.clear();
            } else {
                let lang = line.trim_start().trim_start_matches('`').trim();
                self.code_lang = lang.to_string();
                print!("{}", format_code_block_start(&self.code_lang));
                self.in_code_block = true;
            }
            println!();
            return;
        }

        // Inside code block: dim output, no markdown parsing
        if self.in_code_block {
            println!("{}", format_code_line(&line));
            return;
        }

        // Normal line: line-level + inline markdown rendering
        let rendered = self.render_line(&line);
        println!("{}", rendered);
    }

    /// Render a single line with markdown line-level and inline formatting.
    fn render_line(&self, line: &str) -> String {
        if let Some((level, content)) = parse_heading(line) {
            return format_heading(level, content);
        }
        if let Some(content) = parse_unordered_list_item(line) {
            return format_list_item("•", content);
        }
        if let Some((num, content)) = parse_ordered_list_item(line) {
            return format_list_item(&format!("{}.", num), content);
        }
        if let Some(content) = line.strip_prefix('>') {
            return format_blockquote(content.trim_start());
        }
        if is_horizontal_rule(line.trim()) {
            return format_horizontal_rule();
        }
        // Default: inline rendering (bold, italic, code, links)
        render_inline(line)
    }

    /// Flush any remaining buffered content and close unclosed code blocks.
    ///
    /// Must be called when the stream ends to ensure the last line (if it
    /// didn't end with `\n`) is rendered and any unclosed code block is closed.
    pub fn finish(&mut self) {
        // Flush remaining content in line_buf
        if !self.line_buf.is_empty() {
            self.flush_line();
        }
        // Auto-close unclosed code block
        if self.in_code_block {
            println!("{}", format_code_block_end());
            self.in_code_block = false;
            self.code_lang.clear();
        }
        let _ = std::io::stdout().flush();
    }

    /// Returns `true` if we are currently inside a fenced code block.
    #[cfg(test)]
    pub fn is_in_code_block(&self) -> bool {
        self.in_code_block
    }

    /// Returns the current line buffer content (for testing).
    #[cfg(test)]
    pub fn line_buf(&self) -> &str {
        &self.line_buf
    }
}

impl Default for StreamingMarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a message chunk.
    fn msg(content: &str) -> MessageChunk {
        MessageChunk::message(content)
    }

    /// Helper: create a thinking chunk.
    fn think(content: &str) -> MessageChunk {
        MessageChunk::thinking(content)
    }

    #[test]
    fn empty_renderer_has_default_state() {
        let r = StreamingMarkdownRenderer::new();
        assert!(!r.is_in_code_block());
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn single_line_with_newline() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("hello world\n"));
        // Line should be flushed; buffer should be empty
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn chunks_accumulate_until_newline() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("hel"));
        assert_eq!(r.line_buf(), "hel");
        r.push_chunk(&msg("lo\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn code_block_toggle() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("```rust\n"));
        assert!(r.is_in_code_block());
        r.push_chunk(&msg("fn main() {}\n"));
        assert!(r.is_in_code_block());
        r.push_chunk(&msg("```\n"));
        assert!(!r.is_in_code_block());
    }

    #[test]
    fn finish_flushes_remaining_buffer() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("unfinished line"));
        assert_eq!(r.line_buf(), "unfinished line");
        r.finish();
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn finish_closes_unclosed_code_block() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("```python\n"));
        r.push_chunk(&msg("print('hi')\n"));
        assert!(r.is_in_code_block());
        r.finish();
        assert!(!r.is_in_code_block());
    }

    #[test]
    fn carriage_return_is_ignored() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("hello\r\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn multiple_lines_in_one_chunk() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("line1\nline2\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn heading_rendered() {
        let mut r = StreamingMarkdownRenderer::new();
        // Just verify it doesn't panic and buffer is cleared
        r.push_chunk(&msg("# Hello\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn bold_italic_rendered() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("**bold** and *italic*\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn list_item_rendered() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("- item one\n"));
        r.push_chunk(&msg("1. first\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn blockquote_rendered() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("> quote text\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn horizontal_rule_rendered() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("---\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn empty_line_preserved() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn mixed_thinking_and_message() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&think("reasoning here"));
        // Thinking doesn't go through line_buf
        assert!(r.line_buf().is_empty());
        r.push_chunk(&msg("actual message\n"));
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn full_markdown_document_chunks() {
        let mut r = StreamingMarkdownRenderer::new();
        // Simulate a document arriving token-by-token
        let chunks = vec![
            "# Title\n",
            "\n",
            "This is **bold** text.\n",
            "\n",
            "- Item 1\n",
            "- Item 2\n",
            "\n",
            "```rust\n",
            "fn main() {}\n",
            "```\n",
            "\n",
            "> A quote\n",
            "\n",
            "End.\n",
        ];
        for chunk in chunks {
            r.push_chunk(&msg(chunk));
        }
        r.finish();
        assert!(!r.is_in_code_block());
        assert!(r.line_buf().is_empty());
    }

    #[test]
    fn bold_split_across_chunks() {
        let mut r = StreamingMarkdownRenderer::new();
        r.push_chunk(&msg("text **bol"));
        r.push_chunk(&msg("d** more\n"));
        assert!(r.line_buf().is_empty());
    }
}