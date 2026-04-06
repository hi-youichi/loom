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

/// Convert markdown to Telegram MarkdownV2 format.
/// Supports: **bold**, *italic*, `code`, ```code blocks```, [links](url)
pub fn markdown_to_telegram_v2(markdown: &str) -> String {
    let mut result = String::with_capacity(markdown.len() * 2);
    let mut i = 0;
    let chars: Vec<char> = markdown.chars().collect();
    let len = chars.len();

    while i < len {
        if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            let start = i;
            i += 3;
            let mut found = false;

            while i + 2 < len {
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
                let remaining: String = chars[start..].iter().collect();
                result.push_str(&remaining);
                break;
            }
            continue;
        }

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
                let code: String = chars[start..].iter().collect();
                result.push_str(&code);
                break;
            }
            continue;
        }

        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            i += 2;
            let content_start = i;
            let mut found = false;

            while i + 1 < len {
                if chars[i] == '*' && chars[i + 1] == '*' {
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

        if chars[i] == '*' && (i == 0 || chars[i - 1] != '*') {
            i += 1;
            let content_start = i;
            let mut found = false;

            while i < len {
                if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
                    let content: String = chars[content_start..i].iter().collect();
                    result.push('_');
                    result.push_str(&escape_markdown_v2(&content));
                    result.push('_');
                    i += 1;
                    found = true;
                    break;
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

        let reserved = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
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
        assert_eq!(escaped, "\\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\-\\=\\|\\{\\}\\.\\!");
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
        assert_eq!(markdown_to_telegram_v2("price is $100."), "price is $100\\.");
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
        assert_eq!(markdown_to_telegram_v2("```no closing"), "```no closing");
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
        assert_eq!(markdown_to_telegram_v2("**price is $10.**"), "*price is $10\\.*");
    }
}
