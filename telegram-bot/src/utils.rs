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
}
