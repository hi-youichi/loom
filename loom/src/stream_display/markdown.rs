//! Lightweight terminal markdown renderer.
//!
//! Parses common markdown structures and renders them with ANSI escape codes
//! for terminal display. Used by the goal runner and stream display to render
//! LLM output (reasoning, replies) with formatting.

use crate::stream_display::panel_format::{bold, color_enabled, dim, yellow};

/// Renders a markdown string to a terminal-friendly ANSI string.
///
/// Supports:
/// - Headings: `# H1`, `## H2`, `### H3` etc.
/// - Bold: `**text**` or `__text__`
/// - Italic: `*text*` or `_text_`
/// - Code blocks: ` ```lang ... ``` `
/// - Inline code: `` `code` ``
/// - Unordered lists: `- item` or `* item`
/// - Ordered lists: `1. item`
/// - Blockquotes: `> quote`
/// - Horizontal rules: `---` or `***`
/// - Links: `[text](url)` → `text (url)`
pub fn render_markdown(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 128);
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut _heading_prefix = "";

    for line in input.lines() {
        // Toggle code block state
        if line.trim_start().starts_with("```") {
            if in_code_block {
                // End code block
                output.push_str(&format_code_block_end());
                in_code_block = false;
                code_block_lang.clear();
            } else {
                // Start code block
                let lang = line.trim_start().trim_start_matches('`').trim();
                code_block_lang = lang.to_string();
                output.push_str(&format_code_block_start(&code_block_lang));
                in_code_block = true;
            }
            output.push('\n');
            continue;
        }

        if in_code_block {
            output.push_str(&format_code_line(line));
            output.push('\n');
            continue;
        }

        // Check for horizontal rule
        let trimmed = line.trim();
        if is_horizontal_rule(trimmed) {
            output.push_str(&format_horizontal_rule());
            output.push('\n');
            continue;
        }

        // Check for headings
        if let Some((level, content)) = parse_heading(line) {
            output.push_str(&format_heading(level, content));
            output.push('\n');
            _heading_prefix = "";
            continue;
        }

        // Check for blockquote
        if let Some(content) = line.strip_prefix('>') {
            output.push_str(&format_blockquote(content.trim_start()));
            output.push('\n');
            continue;
        }

        // Check for list items (unordered)
        if let Some(content) = parse_unordered_list_item(line) {
            output.push_str(&format_list_item("•", content));
            output.push('\n');
            _heading_prefix = "";
            continue;
        }

        // Check for list items (ordered)
        if let Some((num, content)) = parse_ordered_list_item(line) {
            output.push_str(&format_list_item(&format!("{}.", num), content));
            output.push('\n');
            _heading_prefix = "";
            continue;
        }

        // Regular line — render inline formatting
        if !line.is_empty() {
            output.push_str(&render_inline(line));
            output.push('\n');
            _heading_prefix = "";
        } else {
            output.push('\n');
        }
    }

    // Close unclosed code block
    if in_code_block {
        output.push_str(&format_code_block_end());
    }

    output
}

/// Renders inline markdown formatting (bold, italic, code, links).
pub fn render_inline(input: &str) -> String {
    let mut result = String::with_capacity(input.len() + 32);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Inline code: `...`
        if chars[i] == '`' {
            let start = i + 1;
            let end = find_closing_backtick(&chars, start);
            if end > start {
                let code: String = chars[start..end].iter().collect();
                result.push_str(&format_inline_code(&code));
                i = end + 1;
                continue;
            }
        }

        // Bold: **text** or __text__
        if i + 1 < chars.len() && (chars[i] == '*' && chars[i + 1] == '*'
            || chars[i] == '_' && chars[i + 1] == '_')
        {
            let marker = chars[i];
            let start = i + 2;
            if let Some(end) = find_closing_marker(&chars, start, marker, 2) {
                let text: String = chars[start..end].iter().collect();
                result.push_str(&bold(&render_inline(&text)));
                i = end + 2;
                continue;
            }
        }

        // Italic: *text* or _text_
        if (chars[i] == '*' || chars[i] == '_')
            && (i + 1 < chars.len() && chars[i + 1] != chars[i])
        {
            let marker = chars[i];
            let start = i + 1;
            if let Some(end) = find_closing_marker(&chars, start, marker, 1) {
                let text: String = chars[start..end].iter().collect();
                result.push_str(&format_italic(&render_inline(&text)));
                i = end + 1;
                continue;
            }
        }

        // Link: [text](url)
        if chars[i] == '[' {
            if let Some((text_end, url_end)) = find_link_end(&chars, i) {
                let text: String = chars[i + 1..text_end].iter().collect();
                let url: String = chars[text_end + 2..url_end].iter().collect();
                result.push_str(&format_link(&render_inline(&text), &url));
                i = url_end + 1;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

// ── Parsing helpers ──────────────────────────────────────────────

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|&b| b == b'#').count() as u8;
    if level > 0 && level <= 6 {
        let content = trimmed[level as usize..].trim_start();
        if !content.is_empty() {
            return Some((level, content));
        }
    }
    None
}

fn parse_unordered_list_item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        // Make sure it's not a horizontal rule
        let after_marker = &trimmed[2..];
        if !after_marker.chars().all(|c| c == '-' || c == '*') {
            let indent = line.len() - trimmed.len();
            let _ = indent; // Could use for nesting later
            return Some(after_marker.trim_start());
        }
    }
    None
}

fn parse_ordered_list_item(line: &str) -> Option<(u32, &str)> {
    let trimmed = line.trim_start();
    let dot_pos = trimmed.find(". ")?;
    let num_str = &trimmed[..dot_pos];
    let num: u32 = num_str.parse().ok()?;
    let content = &trimmed[dot_pos + 2..];
    Some((num, content))
}

fn is_horizontal_rule(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = match s.chars().next() {
        Some(c) if c == '-' || c == '*' || c == '_' => c,
        _ => return false,
    };
    let count = s.chars().filter(|&c| c == first).count();
    count >= 3 && s.chars().all(|c| c == first || c == ' ')
}

fn find_closing_backtick(chars: &[char], start: usize) -> usize {
    for (i, &ch) in chars.iter().enumerate().skip(start) {
        if ch == '`' {
            return i;
        }
    }
    start // No closing backtick found
}

fn find_closing_marker(chars: &[char], start: usize, marker: char, count: usize) -> Option<usize> {
    let mut found = 0;
    for (i, &ch) in chars.iter().enumerate().skip(start) {
        if ch == marker {
            found += 1;
            if found == count {
                return Some(i - count + 1);
            }
        } else {
            found = 0;
        }
    }
    None
}

fn find_link_end(chars: &[char], start: usize) -> Option<(usize, usize)> {
    // Find closing ]
    let mut depth = 0;
    let mut text_end = None;
    for (i, &ch) in chars.iter().enumerate().skip(start) {
        match ch {
            '[' => depth += 1,
            ']' => {
                if depth == 1 {
                    text_end = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let text_end = text_end?;

    // Find ( ... )
    if text_end + 1 >= chars.len() || chars[text_end + 1] != '(' {
        return None;
    }
    for (i, &ch) in chars.iter().enumerate().skip(text_end + 2) {
        if ch == ')' {
            return Some((text_end, i));
        }
    }
    None
}

// ── Formatting helpers ───────────────────────────────────────────

fn format_heading(level: u8, content: &str) -> String {
    let rendered = render_inline(content);
    if color_enabled() {
        match level {
            1 => format!("\x1b[1;4m{}\x1b[0m", rendered),
            2 => format!("\x1b[1m{}\x1b[0m", rendered),
            3 => format!("\x1b[1;36m{}\x1b[0m", rendered),
            _ => format!("\x1b[1m{}\x1b[0m", rendered),
        }
    } else {
        let marker = "#".repeat(level as usize);
        format!("{} {}", marker, rendered)
    }
}

fn format_code_block_start(lang: &str) -> String {
    if color_enabled() {
        if lang.is_empty() {
            "\x1b[2m────────────────────\x1b[0m".to_string()
        } else {
            format!("\x1b[2m─── {} ────────────\x1b[0m", lang)
        }
    } else {
        format!("---{}---", if lang.is_empty() { String::new() } else { format!(" {} ", lang) })
    }
}

fn format_code_block_end() -> String {
    if color_enabled() {
        "\x1b[2m────────────────────\x1b[0m".to_string()
    } else {
        "------".to_string()
    }
}

fn format_code_line(line: &str) -> String {
    if color_enabled() {
        format!("\x1b[2m  {}\x1b[0m", line)
    } else {
        format!("  {}", line)
    }
}

fn format_inline_code(code: &str) -> String {
    if color_enabled() {
        format!("\x1b[33m{}\x1b[0m", code)
    } else {
        format!("`{}`", code)
    }
}

fn format_italic(text: &str) -> String {
    if color_enabled() {
        format!("\x1b[3m{}\x1b[0m", text)
    } else {
        text.to_string()
    }
}

fn format_link(text: &str, url: &str) -> String {
    if color_enabled() {
        format!("\x1b[4m{}\x1b[0m ({})", text, dim(url))
    } else {
        format!("{} ({})", text, url)
    }
}

fn format_blockquote(content: &str) -> String {
    if color_enabled() {
        format!("\x1b[2m│\x1b[0m {}", render_inline(content))
    } else {
        format!("> {}", render_inline(content))
    }
}

fn format_list_item(bullet: &str, content: &str) -> String {
    let rendered = render_inline(content);
    if color_enabled() {
        format!("  {} {}", yellow(bullet), rendered)
    } else {
        format!("  {} {}", bullet, rendered)
    }
}

fn format_horizontal_rule() -> String {
    if color_enabled() {
        "\x1b[2m────────────────────\x1b[0m".to_string()
    } else {
        "────────────────────".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_heading_h1() {
        let result = render_markdown("# Hello World");
        assert!(result.contains("Hello World"));
        // Should not contain raw #
        if color_enabled() {
            assert!(!result.contains("# Hello"));
        }
    }

    #[test]
    fn render_heading_h2() {
        let result = render_markdown("## Section Title");
        assert!(result.contains("Section Title"));
    }

    #[test]
    fn render_heading_h3() {
        let result = render_markdown("### Subsection");
        assert!(result.contains("Subsection"));
    }

    #[test]
    fn render_bold() {
        let result = render_markdown("This is **bold** text");
        assert!(result.contains("bold"));
    }

    #[test]
    fn render_italic() {
        let result = render_markdown("This is *italic* text");
        assert!(result.contains("italic"));
    }

    #[test]
    fn render_inline_code() {
        let result = render_markdown("Use `cargo test` to run");
        assert!(result.contains("cargo test"));
    }

    #[test]
    fn render_code_block() {
        let input = "```rust\nfn main() {}\n```";
        let result = render_markdown(input);
        assert!(result.contains("fn main() {}"));
    }

    #[test]
    fn render_unordered_list() {
        let input = "- First item\n- Second item";
        let result = render_markdown(input);
        assert!(result.contains("First item"));
        assert!(result.contains("Second item"));
    }

    #[test]
    fn render_ordered_list() {
        let input = "1. First\n2. Second\n3. Third";
        let result = render_markdown(input);
        assert!(result.contains("First"));
        assert!(result.contains("Second"));
    }

    #[test]
    fn render_blockquote() {
        let result = render_markdown("> This is a quote");
        assert!(result.contains("This is a quote"));
    }

    #[test]
    fn render_horizontal_rule() {
        let result = render_markdown("---");
        assert!(result.contains("──"));
    }

    #[test]
    fn render_link() {
        let result = render_markdown("[Loom](https://example.com)");
        assert!(result.contains("Loom"));
        assert!(result.contains("example.com"));
    }

    #[test]
    fn render_plain_text_passthrough() {
        let input = "Just some plain text\nwith multiple lines.";
        let result = render_markdown(input);
        assert!(result.contains("Just some plain text"));
        assert!(result.contains("with multiple lines."));
    }

    #[test]
    fn render_empty_input() {
        let result = render_markdown("");
        assert!(result.is_empty() || result == "\n");
    }

    #[test]
    fn render_nested_formatting() {
        let input = "**Bold and `code` inside**";
        let result = render_markdown(input);
        assert!(result.contains("code"));
        assert!(result.contains("Bold and"));
    }

    #[test]
    fn render_mixed_document() {
        let input = r#"# Project Title

This is a **description** with `inline code`.

## Features

- Feature one
- Feature two

```python
print("hello")
```

> A quote

---

End."#;
        let result = render_markdown(input);
        assert!(result.contains("Project Title"));
        assert!(result.contains("description"));
        assert!(result.contains("inline code"));
        assert!(result.contains("Feature one"));
        assert!(result.contains("print"));
        assert!(result.contains("A quote"));
        assert!(result.contains("End"));
    }

    #[test]
    fn render_inline_no_markers() {
        let result = render_inline("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn render_inline_bold_italic() {
        let result = render_inline("**bold** and *italic*");
        assert!(result.contains("bold"));
        assert!(result.contains("italic"));
    }

    #[test]
    fn render_inline_code_backtick() {
        let result = render_inline("use `foo` here");
        assert!(result.contains("foo"));
    }

    #[test]
    fn unclosed_code_block_gets_closed() {
        let input = "```\nsome code\n";
        let result = render_markdown(input);
        // Should have closing separator even without explicit ```
        assert!(result.contains("some code"));
    }

    #[test]
    fn render_code_block_with_language() {
        let input = "```javascript\nconsole.log('hi');\n```";
        let result = render_markdown(input);
        assert!(result.contains("javascript"));
        assert!(result.contains("console.log"));
    }
}
