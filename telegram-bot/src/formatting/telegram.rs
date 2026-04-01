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
            text: escape_markdown_v2(&text),
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
