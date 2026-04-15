//! Utility functions for telegram-bot
//!
//! This module provides common utility functions used across handlers.

use std::time::{Duration, Instant};

/// Truncate text to max characters, adding "..." if truncated
///
/// Note: Uses char count (not bytes) for proper UTF-8 handling
///
/// # Arguments
/// * `text` - Text to truncate
/// * `max_chars` - Maximum number of characters (0 = no limit)
pub fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Split text into chunks that fit Telegram's single-message limit.
///
/// Prefers splitting at newlines, punctuation, or spaces near the boundary.
pub fn split_text_for_telegram(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() || max_chars == 0 || text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let remaining = chars.len() - start;
        if remaining <= max_chars {
            let chunk: String = chars[start..].iter().collect();
            if !chunk.is_empty() {
                chunks.push(chunk);
            }
            break;
        }

        let hard_end = start + max_chars;
        let soft_start = start + (max_chars / 2).max(1);
        let split_at = find_split_index(&chars, soft_start, hard_end).unwrap_or(hard_end);

        let chunk: String = chars[start..split_at]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_string();
        if chunk.is_empty() {
            let forced_chunk: String = chars[start..hard_end].iter().collect();
            chunks.push(forced_chunk);
            start = hard_end;
        } else {
            chunks.push(chunk);
            start = split_at;
        }

        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }

    chunks
}

fn find_split_index(chars: &[char], start: usize, end: usize) -> Option<usize> {
    for idx in (start..end).rev() {
        if chars[idx] == '\n' {
            return Some(idx + 1);
        }
    }
    for idx in (start..end).rev() {
        if matches!(
            chars[idx],
            '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；'
        ) {
            return Some(idx + 1);
        }
    }
    for idx in (start..end).rev() {
        if chars[idx].is_whitespace() {
            return Some(idx + 1);
        }
    }
    None
}

/// Check if enough time has elapsed since last update (throttle helper)
///
/// # Arguments
/// * `last_update` - Time of last update
/// * `min_interval_ms` - Minimum interval in milliseconds
pub fn should_update(last_update: Instant, min_interval_ms: u64) -> bool {
    last_update.elapsed() >= Duration::from_millis(min_interval_ms)
}

/// Extract file extension from filename or MIME type
///
/// # Arguments
/// * `filename` - Optional filename
/// * `mime_type` - Optional MIME type
pub fn get_file_extension(filename: Option<&str>, mime_type: Option<&str>) -> String {
    // Try to get extension from filename
    if let Some(fname) = filename {
        if let Some(ext) = fname.rsplit('.').next() {
            if ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                return ext.to_lowercase();
            }
        }
    }

    // Fall back to MIME type
    if let Some(mime) = mime_type {
        return match mime {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            "audio/mpeg" => "mp3",
            "audio/ogg" => "ogg",
            "application/pdf" => "pdf",
            "application/zip" => "zip",
            _ => "bin",
        }
        .to_string();
    }

    "bin".to_string()
}

/// Generate ISO 8601 timestamp for current time
pub fn current_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Sanitize text for display (remove or escape special characters)
///
/// # Arguments
/// * `text` - Text to sanitize
/// * `max_length` - Maximum length (0 = no limit)
pub fn sanitize_for_display(text: &str, max_length: usize) -> String {
    let sanitized = text
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");

    if max_length > 0 {
        truncate_text(&sanitized, max_length)
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_text() {
        assert_eq!(truncate_text("Hello", 10), "Hello");
        assert_eq!(truncate_text("Hello World", 5), "He...");
        assert_eq!(truncate_text("Hello", 0), "Hello");
        assert_eq!(truncate_text("你好世界", 3), "...");
        assert_eq!(truncate_text("你好世界测试", 5), "你好...");
        assert_eq!(truncate_text("你好世界", 4), "你好世界");
    }

    #[test]
    fn test_get_file_extension() {
        assert_eq!(get_file_extension(Some("photo.jpg"), None), "jpg");
        assert_eq!(get_file_extension(Some("document.PDF"), None), "pdf");
        assert_eq!(get_file_extension(None, Some("image/png")), "png");
        assert_eq!(get_file_extension(None, None), "bin");
    }

    #[test]
    fn test_sanitize_for_display() {
        assert_eq!(sanitize_for_display("Line1\nLine2", 0), "Line1\\nLine2");
        assert_eq!(sanitize_for_display("Tab\there", 10), "Tab\\there");
    }

    #[test]
    fn split_text_for_telegram_keeps_short_message_intact() {
        let chunks = split_text_for_telegram("hello", 10);
        assert_eq!(chunks, vec!["hello".to_string()]);
    }

    #[test]
    fn split_text_for_telegram_splits_long_message_by_limit() {
        let text = "a".repeat(25);
        let chunks = split_text_for_telegram(&text, 10);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 10));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn split_text_for_telegram_prefers_newline_boundary() {
        let text = "first line\nsecond line\nthird line";
        let chunks = split_text_for_telegram(text, 15);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].contains("first line"));
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 15));
    }
}
