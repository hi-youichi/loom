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

    pub fn markdown_v2_rendered(rendered_text: impl Into<String>, plain_text_fallback: impl Into<String>) -> Self {
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
    let rendered = format!("*{}*\n\n{}", escape_markdown_v2(title), escape_markdown_v2(body));
    let fallback = format!("{title}\n\n{body}");
    FormattedMessage::markdown_v2_rendered(rendered, fallback)
}

/// Convert markdown to Telegram MarkdownV2 format.
/// Supports: **bold**, *italic*, `code`, ```code blocks```, [links](url)
pub fn markdown_to_telegram_v2(markdown: &str) -> String {
    let mut result = String::with_capacity(markdown.len() * 2);
    let mut i = 0;
    let chars: Vec<char> = markdown.chars().collect();
    
    while i < chars.len() {
        // Check for code blocks ``` (triple backticks)
        if i + 2 < chars.len() && chars[i] == '`' && chars[i+1] == '`' && chars[i+2] == '`' {
            let start = i;
            i += 3;
            
            // Find closing ```
            while i + 2 < chars.len() {
                if chars[i] == '`' && chars[i+1] == '`' && chars[i+2] == '`' {
                    i += 3;
                    // Extract code block content (keep as-is, no escaping)
                    let code: String = chars[start..i].iter().collect();
                    result.push_str(&code);
                    break;
                }
                i += 1;
            }
            continue;
        }
        
        // Check for inline code ` (single backtick)
        if chars[i] == '`' {
            let start = i;
            i += 1;
            
            // Find closing `
            while i < chars.len() && chars[i] != '`' {
                i += 1;
            }
            
            if i < chars.len() {
                i += 1;
                // Extract inline code content (keep as-is)
                let code: String = chars[start..i].iter().collect();
                result.push_str(&code);
            }
            continue;
        }
        
        // Check for **bold**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i+1] == '*' {
            let start = i;
            i += 2;
            
            // Find closing **
            while i + 1 < chars.len() {
                if chars[i] == '*' && chars[i+1] == '*' {
                    i += 2;
                    // Extract bold content and convert to single * for Telegram
                    let content: String = chars[start+2..i-2].iter().collect();
                    result.push('*');
                    result.push_str(&escape_markdown_v2(&content));
                    result.push('*');
                    break;
                }
                i += 1;
            }
            continue;
        }
        
        // Check for *italic* (single asterisk, but not **)
        if chars[i] == '*' && (i == 0 || chars[i-1] != '*') {
            let start = i;
            i += 1;
            
            // Find closing *
            while i < chars.len() {
                if chars[i] == '*' && (i + 1 >= chars.len() || chars[i+1] != '*') {
                    i += 1;
                    // Extract italic content and convert to _ for Telegram
                    let content: String = chars[start+1..i-1].iter().collect();
                    result.push('_');
                    result.push_str(&escape_markdown_v2(&content));
                    result.push('_');
                    break;
                }
                i += 1;
            }
            continue;
        }
        
        // Check for [link](url)
        if chars[i] == '[' {
            let start = i;
            i += 1;
            
            // Find closing ]
            let mut link_end = i;
            while link_end < chars.len() && chars[link_end] != ']' {
                link_end += 1;
            }
            
            // Check for (url) after ]
            if link_end < chars.len() && chars[link_end] == ']' {
                let url_start = link_end + 1;
                if url_start < chars.len() && chars[url_start] == '(' {
                    let mut url_end = url_start + 1;
                    while url_end < chars.len() && chars[url_end] != ')' {
                        url_end += 1;
                    }
                    
                    if url_end < chars.len() {
                        let link_text: String = chars[start+1..link_end].iter().collect();
                        let url: String = chars[url_start+1..url_end].iter().collect();
                        
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
            
            // Not a valid link, escape [
            result.push('\\');
            result.push('[');
            i = start + 1;
            continue;
        }
        
        // Escape reserved characters in normal text
        let reserved = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];
        if reserved.contains(&chars[i]) {
            result.push('\\');
        }
        result.push(chars[i]);
        i += 1;
    }
    
    result
}
