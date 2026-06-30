use teloxide::types::ParseMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramMessageFormat {
    PlainText,
    MarkdownV2,
    Html,
}

#[derive(Debug, Clone)]
pub struct FormattedMessage {
    pub text: String,
    pub parse_mode: Option<ParseMode>,
    pub plain_text_fallback: String,
}

impl FormattedMessage {
    pub fn plain(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            plain_text_fallback: text.clone(),
            text,
            parse_mode: None,
        }
    }

    pub fn markdown_v2(text: impl Into<String>, plain_text_fallback: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            text: markdown_to_telegram_v2(&text),
            parse_mode: Some(ParseMode::MarkdownV2),
            plain_text_fallback: plain_text_fallback.into(),
        }
    }

    pub fn markdown_v2_rendered(
        rendered_text: impl Into<String>,
        plain_text_fallback: impl Into<String>,
    ) -> Self {
        Self {
            text: rendered_text.into(),
            parse_mode: Some(ParseMode::MarkdownV2),
            plain_text_fallback: plain_text_fallback.into(),
        }
    }

    pub fn html(rendered_text: impl Into<String>, plain_text_fallback: impl Into<String>) -> Self {
        Self {
            text: rendered_text.into(),
            parse_mode: Some(ParseMode::Html),
            plain_text_fallback: plain_text_fallback.into(),
        }
    }
}

pub fn escape_markdown_v2(text: &str) -> String {
    let reserved = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];

    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if reserved.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn markdown_notice(title: &str, body: &str) -> FormattedMessage {
    let rendered = format!(
        "*{}*\n\n{}",
        escape_markdown_v2(title),
        escape_markdown_v2(body)
    );
    let fallback = format!("{title}\n\n{body}");
    FormattedMessage::markdown_v2_rendered(rendered, fallback)
}

fn strip_outer_code_fence(input: &str) -> &str {
    let trimmed = input.trim();
    if !trimmed.starts_with("```") {
        return input;
    }
    let after_open = &trimmed[3..];
    let newline_pos = match after_open.find('\n') {
        Some(p) => p,
        None => return input,
    };
    let lang_tag = after_open[..newline_pos].trim();
    if lang_tag != "markdown" && lang_tag != "md" {
        return input;
    }
    let content_start = 3 + newline_pos + 1;
    if !trimmed.ends_with("```") {
        return input;
    }
    let close_pos = trimmed.len() - 3;
    if close_pos <= content_start {
        return input;
    }
    let before_close = &trimmed[close_pos..];
    if before_close != "```" {
        return input;
    }
    let inner = &trimmed[content_start..close_pos];
    if inner.contains("```") {
        return input;
    }
    inner
}

/// Convert markdown to Telegram MarkdownV2 format.
/// Supports: **bold**, *italic*, `code`, ```code blocks```, \[links\](url)
///
/// Fixed bugs:
/// - Code block boundary: `while i + 2 < len` → `while i + 3 <= len` (off-by-one)
/// - Unclosed code block: content now gets escaped as plain text
/// - Bold/italic nesting: `**bold *italic* text**` handled correctly
/// - Bold closing: `while i + 1 < len` → `while i < len` to find `**` at end of string
pub fn markdown_to_telegram_v2(markdown: &str) -> String {
    let markdown = strip_outer_code_fence(markdown);

    let mut result = String::with_capacity(markdown.len() * 2);
    let mut i = 0;
    let chars: Vec<char> = markdown.chars().collect();
    let len = chars.len();

    while i < len {
        // ── Fenced code block: ```...``` ──
        if i + 3 <= len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            let start = i;
            i += 3;
            let mut found = false;

            // Fixed: was `i + 2 < len`, missed closing ``` at the last 3 chars
            while i + 3 <= len {
                if chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
                    i += 3;
                    let code: String = chars[start..i].iter().collect();
                    result.push_str(&code);
                    found = true;
                    break;
                }
                i += 1;
            }

            if !found {
                // Unclosed code block: treat content as plain text and escape it
                let content: String = chars[start + 3..].iter().collect();
                result.push_str(&escape_markdown_v2(&content));
                break;
            }
            continue;
        }

        // ── Inline code: `...` ──
        if chars[i] == '`' {
            let start = i;
            i += 1;

            while i < len && chars[i] != '`' {
                i += 1;
            }

            if i < len {
                i += 1;
                let code: String = chars[start..i].iter().collect();
                result.push_str(&code);
            } else {
                // Unclosed inline code: treat as plain text
                let code: String = chars[start + 1..].iter().collect();
                result.push_str(&escape_markdown_v2(&code));
                break;
            }
            continue;
        }

        // ── Bold: **...** ──
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            i += 2;
            let content_start = i;
            let mut found = false;

            // Fixed: was `i + 1 < len`, missed closing ** at end of string
            while i < len {
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '*' {
                    let content: String = chars[content_start..i].iter().collect();
                    result.push('*');
                    result.push_str(&escape_markdown_v2(&content));
                    result.push('*');
                    i += 2;
                    found = true;
                    break;
                }
                i += 1;
            }

            if !found {
                result.push_str("\\*\\*");
                let content: String = chars[content_start..].iter().collect();
                result.push_str(&escape_markdown_v2(&content));
                break;
            }
            continue;
        }

        // ── Italic: *...* (must not be part of **) ──
        if chars[i] == '*' {
            i += 1;
            let content_start = i;
            let mut found = false;

            while i < len {
                // Found closing *, but skip if it's start of ** (bold)
                if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
                    let content: String = chars[content_start..i].iter().collect();
                    result.push('_');
                    result.push_str(&escape_markdown_v2(&content));
                    result.push('_');
                    i += 1;
                    found = true;
                    break;
                }
                // If we hit **, skip both so we don't misparse bold as italic
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '*' {
                    i += 2;
                    continue;
                }
                i += 1;
            }

            if !found {
                result.push('\\');
                result.push('*');
                let content: String = chars[content_start..].iter().collect();
                result.push_str(&escape_markdown_v2(&content));
                break;
            }
            continue;
        }

        // ── Link: [text](url) ──
        if chars[i] == '[' {
            let start = i;
            i += 1;

            let mut link_end = i;
            while link_end < len && chars[link_end] != ']' {
                link_end += 1;
            }

            if link_end < len && chars[link_end] == ']' {
                let url_start = link_end + 1;
                if url_start < len && chars[url_start] == '(' {
                    let mut url_end = url_start + 1;
                    while url_end < len && chars[url_end] != ')' {
                        url_end += 1;
                    }

                    if url_end < len {
                        let link_text: String = chars[start + 1..link_end].iter().collect();
                        let url: String = chars[url_start + 1..url_end].iter().collect();

                        result.push('[');
                        result.push_str(&escape_markdown_v2(&link_text));
                        result.push_str("](");
                        result.push_str(&url);
                        result.push(')');

                        i = url_end + 1;
                        continue;
                    }
                }
            }

            result.push('\\');
            result.push('[');
            i = start + 1;
            continue;
        }

        // ── Default: escape Telegram MarkdownV2 reserved chars ──
        let reserved = [
            '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.',
            '!',
        ];
        if reserved.contains(&chars[i]) {
            result.push('\\');
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markdown_v2_reserved_chars() {
        let input = "_*[]()~`>#+-=|{}.!";
        let escaped = escape_markdown_v2(input);
        assert_eq!(
            escaped,
            "\\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\-\\=\\|\\{\\}\\.\\!"
        );
    }

    #[test]
    fn escapes_html_reserved_chars() {
        let input = "<tag>&value>";
        let escaped = escape_html(input);
        assert_eq!(escaped, "&lt;tag&gt;&amp;value&gt;");
    }

    #[test]
    fn markdown_v2_message_converts_bold() {
        let message = FormattedMessage::markdown_v2("**Hello** World", "Hello World");
        assert_eq!(message.text, "*Hello* World");
        assert_eq!(message.plain_text_fallback, "Hello World");
    }

    #[test]
    fn markdown_v2_message_converts_italic() {
        let message = FormattedMessage::markdown_v2("*Hello* World", "Hello World");
        assert_eq!(message.text, "_Hello_ World");
        assert_eq!(message.plain_text_fallback, "Hello World");
    }

    #[test]
    fn markdown_v2_rendered_preserves_intentional_markup() {
        let message = FormattedMessage::markdown_v2_rendered("*Title*", "Title");
        assert_eq!(message.text, "*Title*");
        assert_eq!(message.plain_text_fallback, "Title");
    }

    #[test]
    fn bold() {
        assert_eq!(markdown_to_telegram_v2("**bold**"), "*bold*");
    }

    #[test]
    fn italic() {
        assert_eq!(markdown_to_telegram_v2("*italic*"), "_italic_");
    }

    #[test]
    fn inline_code() {
        assert_eq!(markdown_to_telegram_v2("`code`"), "`code`");
    }

    #[test]
    fn code_block() {
        assert_eq!(
            markdown_to_telegram_v2("```rust\nfn main() {}\n```"),
            "```rust\nfn main() {}\n```"
        );
    }

    #[test]
    fn link() {
        assert_eq!(
            markdown_to_telegram_v2("[click here](https://example.com)"),
            "[click here](https://example.com)"
        );
    }

    #[test]
    fn plain_text() {
        assert_eq!(markdown_to_telegram_v2("hello world"), "hello world");
    }

    #[test]
    fn reserved_chars_escaped() {
        assert_eq!(
            markdown_to_telegram_v2("price is $100."),
            "price is $100\\."
        );
    }

    #[test]
    fn mixed() {
        assert_eq!(
            markdown_to_telegram_v2("**bold** and *italic* and `code`"),
            "*bold* and _italic_ and `code`"
        );
    }

    #[test]
    fn unclosed_bold() {
        assert_eq!(markdown_to_telegram_v2("**no closing"), "\\*\\*no closing");
    }

    #[test]
    fn unclosed_italic() {
        assert_eq!(markdown_to_telegram_v2("*no closing"), "\\*no closing");
    }

    #[test]
    fn unclosed_code_block() {
        // Unclosed code block: content after ``` is escaped as plain text
        let result = markdown_to_telegram_v2("```no closing");
        assert_eq!(result, "no closing");
    }

    #[test]
    fn unclosed_code_block_with_special_chars() {
        // Special chars in unclosed code block should be escaped (; is NOT a reserved char)
        let result = markdown_to_telegram_v2("```rust\nfn main() { let x = 1.0; }");
        assert_eq!(result, "rust\nfn main\\(\\) \\{ let x \\= 1\\.0; \\}");
    }

    #[test]
    fn code_block_at_string_end() {
        // Bug fix: closing ``` at the very end of string
        assert_eq!(
            markdown_to_telegram_v2("```rust\nfn main() {}\n```"),
            "```rust\nfn main() {}\n```"
        );
    }

    #[test]
    fn code_block_with_dots_inside() {
        // Dots inside closed code block should NOT be escaped
        assert_eq!(
            markdown_to_telegram_v2("```\nprice is $10.\n```"),
            "```\nprice is $10.\n```"
        );
    }

    #[test]
    fn bold_at_string_end() {
        // Bug fix: closing ** at the very end of string
        assert_eq!(markdown_to_telegram_v2("**bold**"), "*bold*");
    }

    #[test]
    fn bold_italic_nested() {
        // **bold *italic* text** — inner * is escaped because Telegram MarkdownV2
        // doesn't support nested bold+italic; the content is bold with literal *
        assert_eq!(
            markdown_to_telegram_v2("**bold *italic* text**"),
            "*bold \\*italic\\* text*"
        );
    }

    #[test]
    fn bold_with_inner_stars() {
        // **a * b** — inner * is escaped within bold context
        assert_eq!(
            markdown_to_telegram_v2("**a * b**"),
            "*a \\* b*"
        );
    }

    #[test]
    fn unclosed_inline_code() {
        // Unclosed inline code: content after ` is escaped as plain text
        let result = markdown_to_telegram_v2("`no closing");
        assert_eq!(result, "no closing");
    }

    #[test]
    fn inline_code_with_backticks() {
        assert_eq!(markdown_to_telegram_v2("`code`"), "`code`");
    }

    #[test]
    fn inline_code_with_special_chars() {
        // Special chars inside inline code should NOT be escaped
        assert_eq!(markdown_to_telegram_v2("`price.is($10)`"), "`price.is($10)`");
    }

    #[test]
    fn empty_string() {
        assert_eq!(markdown_to_telegram_v2(""), "");
    }

    #[test]
    fn heading_escaped() {
        assert_eq!(markdown_to_telegram_v2("# Heading"), "\\# Heading");
    }

    #[test]
    fn bold_with_special_chars() {
        assert_eq!(
            markdown_to_telegram_v2("**price is $10.**"),
            "*price is $10\\.*"
        );
    }

    #[test]
    fn strips_outer_markdown_fence() {
        let input = "```markdown\n# Title\n\n**bold** text\n```\n";
        let result = markdown_to_telegram_v2(input);
        assert!(!result.contains("```markdown"));
        assert!(result.contains("\\# Title"));
        assert!(result.contains("*bold*"));
    }

    #[test]
    fn does_not_strip_plain_fence() {
        let input = "```\n# Title\n```\n";
        let result = markdown_to_telegram_v2(input);
        assert!(result.contains("```"));
    }

    #[test]
    fn does_not_strip_if_inner_has_code_fences() {
        let input = "```markdown\nsome text\n```\ncode here\n```inner```\n";
        let result = markdown_to_telegram_v2(input);
        assert!(result.contains("```"));
    }

    #[test]
    fn does_not_strip_non_fence_input() {
        let input = "**bold** text";
        let result = markdown_to_telegram_v2(input);
        assert_eq!(result, "*bold* text");
    }

    #[test]
    fn strips_fence_with_leading_trailing_whitespace() {
        let input = "\n\n```markdown\n**hello**\n```\n\n";
        let result = markdown_to_telegram_v2(input);
        assert!(result.contains("*hello*"));
        assert!(!result.contains("```markdown"));
    }

    // --- FormattedMessage constructors ---
    #[test]
    fn formatted_message_plain() {
        let msg = FormattedMessage::plain("hello");
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.plain_text_fallback, "hello");
        assert!(msg.parse_mode.is_none());
    }

    #[test]
    fn formatted_message_html() {
        let msg = FormattedMessage::html("<b>bold</b>", "bold");
        assert_eq!(msg.text, "<b>bold</b>");
        assert_eq!(msg.plain_text_fallback, "bold");
        assert_eq!(msg.parse_mode, Some(ParseMode::Html));
    }

    #[test]
    fn formatted_message_markdown_v2_rendered() {
        let msg = FormattedMessage::markdown_v2_rendered("*Title*", "Title");
        assert_eq!(msg.text, "*Title*");
        assert_eq!(msg.plain_text_fallback, "Title");
        assert_eq!(msg.parse_mode, Some(ParseMode::MarkdownV2));
    }

    // --- markdown_notice ---
    #[test]
    fn markdown_notice_builds_correct_structure() {
        let msg = markdown_notice("Title", "Body text");
        assert_eq!(msg.parse_mode, Some(ParseMode::MarkdownV2));
        assert!(msg.text.contains("Title"));
        assert!(msg.text.contains("Body text"));
        assert_eq!(msg.plain_text_fallback, "Title\n\nBody text");
    }

    #[test]
    fn markdown_notice_escapes_special_chars() {
        let msg = markdown_notice("Hello!", "Price is $10.");
        // ! and . should be escaped in MarkdownV2
        assert!(msg.text.contains("\\!"));
        assert!(msg.text.contains("\\."));
    }

    // --- TelegramMessageFormat ---
    #[test]
    fn telegram_message_format_equality() {
        assert_eq!(TelegramMessageFormat::PlainText, TelegramMessageFormat::PlainText);
        assert_ne!(TelegramMessageFormat::PlainText, TelegramMessageFormat::MarkdownV2);
        assert_ne!(TelegramMessageFormat::MarkdownV2, TelegramMessageFormat::Html);
    }

    // --- escape functions edge cases ---
    #[test]
    fn escape_markdown_v2_empty() {
        assert_eq!(escape_markdown_v2(""), "");
    }

    #[test]
    fn escape_markdown_v2_no_special() {
        assert_eq!(escape_markdown_v2("hello world"), "hello world");
    }

    #[test]
    fn escape_html_empty() {
        assert_eq!(escape_html(""), "");
    }

    #[test]
    fn escape_html_no_special() {
        assert_eq!(escape_html("hello"), "hello");
    }

    #[test]
    fn escape_html_ampersand() {
        assert_eq!(escape_html("a&b"), "a&amp;b");
    }

    // --- unclosed link ---
    #[test]
    fn unclosed_link_bracket() {
        let result = markdown_to_telegram_v2("[no closing link");
        assert!(result.starts_with("\\["));
    }

    #[test]
    fn link_with_no_url() {
        let result = markdown_to_telegram_v2("[text]no_paren");
        assert!(result.starts_with("\\["));
    }
}
