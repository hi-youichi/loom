//! Tests for handler module

use crate::download::{DownloadConfig, FileMetadata, FileType};
use crate::utils::truncate_text;
use std::path::PathBuf;

#[test]
fn test_download_config_default() {
    let config = DownloadConfig::default();
    assert_eq!(config.dir, std::path::PathBuf::from("downloads"));
    assert!(!config.save_metadata);
}

#[test]
fn test_download_config_new() {
    let config = DownloadConfig::new("/custom/path");
    assert_eq!(config.dir, std::path::PathBuf::from("/custom/path"));
    assert!(!config.save_metadata);
}

#[test]
fn test_download_config_clone() {
    let config = DownloadConfig::new("/test");
    let cloned = config.clone();
    assert_eq!(config.dir, cloned.dir);
    assert_eq!(config.save_metadata, cloned.save_metadata);
}

#[test]
fn test_download_config_file_path() {
    let config = DownloadConfig::new("/tmp/bot_downloads");
    let path = config.get_file_path(123456789, 42, "AgACAgIAAxkBAAI", "jpg");
    assert_eq!(
        path,
        std::path::PathBuf::from("/tmp/bot_downloads/123456789/42_AgACAgIAAxkBAAI.jpg")
    );
}

#[test]
fn test_download_config_file_path_truncated() {
    let config = DownloadConfig::default();
    let long_file_id = "AgACAgIAAxkBAAIRbGQyAAMAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let path = config.get_file_path(123, 1, long_file_id, "jpg");
    let filename = path.file_name().unwrap().to_str().unwrap();
    assert!(filename.starts_with("1_"));
    assert!(filename.ends_with(".jpg"));
    assert!(filename.len() < 50);
}

#[test]
fn test_download_config_file_path_short_id() {
    let config = DownloadConfig::default();
    let short_id = "abc";
    let path = config.get_file_path(123, 1, short_id, "pdf");
    let filename = path.file_name().unwrap().to_str().unwrap();
    assert_eq!(filename, "1_abc.pdf");
}

#[test]
fn test_download_config_file_path_negative_ids() {
    let config = DownloadConfig::default();
    let path = config.get_file_path(-1001234567890, 999, "fid", "mp4");
    assert!(path.to_str().unwrap().contains("-1001234567890"));
    assert!(path.to_str().unwrap().contains("999_fid.mp4"));
}

#[test]
fn test_download_config_metadata_path() {
    let config = DownloadConfig::default();
    let file_path = PathBuf::from("downloads/123/42_photo.jpg");
    let meta_path = config.get_metadata_path(&file_path);
    assert_eq!(meta_path, PathBuf::from("downloads/123/42_photo.json"));
}

#[test]
fn test_download_config_metadata_path_nested() {
    let config = DownloadConfig::default();
    let file_path = PathBuf::from("downloads/123/456/78_video.mp4");
    let meta_path = config.get_metadata_path(&file_path);
    assert_eq!(meta_path, PathBuf::from("downloads/123/456/78_video.json"));
}

#[test]
fn test_file_type_serialization() {
    let types = vec![
        FileType::Photo,
        FileType::Document,
        FileType::Video,
        FileType::Audio,
        FileType::Other,
    ];
    for ft in types {
        let json = serde_json::to_string(&ft).unwrap();
        let parsed: FileType = serde_json::from_str(&json).unwrap();
        assert_eq!(ft, parsed);
    }
}

#[test]
fn test_file_metadata_serialization() {
    let metadata = FileMetadata {
        chat_id: 123456789,
        message_id: 42,
        file_id: "AgACAgIAAxkBAAI".to_string(),
        file_unique_id: "AQADeN1x".to_string(),
        file_type: FileType::Photo,
        mime_type: Some("image/jpeg".to_string()),
        file_size: Some(102400),
        original_name: None,
        user_id: Some(987654321),
        downloaded_at: "2026-03-21T09:00:00Z".to_string(),
    };

    let json = serde_json::to_string(&metadata).unwrap();
    let parsed: FileMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(metadata.chat_id, parsed.chat_id);
    assert_eq!(metadata.message_id, parsed.message_id);
    assert_eq!(metadata.file_id, parsed.file_id);
    assert_eq!(metadata.file_unique_id, parsed.file_unique_id);
    assert_eq!(metadata.file_type, parsed.file_type);
    assert_eq!(metadata.mime_type, parsed.mime_type);
    assert_eq!(metadata.file_size, parsed.file_size);
    assert_eq!(metadata.original_name, parsed.original_name);
    assert_eq!(metadata.user_id, parsed.user_id);
    assert_eq!(metadata.downloaded_at, parsed.downloaded_at);
}

#[test]
fn test_file_metadata_json_format() {
    let metadata = FileMetadata {
        chat_id: 123,
        message_id: 1,
        file_id: "fid".to_string(),
        file_unique_id: "uid".to_string(),
        file_type: FileType::Document,
        mime_type: None,
        file_size: None,
        original_name: Some("test.pdf".to_string()),
        user_id: None,
        downloaded_at: "2026-03-21T09:00:00Z".to_string(),
    };

    let json = serde_json::to_string_pretty(&metadata).unwrap();
    assert!(json.contains("\"chat_id\": 123"));
    assert!(json.contains("\"message_id\": 1"));
    assert!(json.contains("\"file_id\": \"fid\""));
    assert!(json.contains("\"file_unique_id\": \"uid\""));
    assert!(json.contains("\"file_type\": \"Document\""));
}

#[test]
fn test_check_text_for_bot_mention() {
    fn check_mention(text: &str, bot_username: &str) -> bool {
        if bot_username.is_empty() {
            return false;
        }
        let mention = format!("@{}", bot_username.to_lowercase());
        text.to_lowercase().contains(&mention)
    }

    assert!(check_mention("@testbot hello", "testbot"));
    assert!(check_mention("hello @TestBot world", "testbot"));
    assert!(check_mention("@TESTBOT", "testbot"));
    assert!(!check_mention("hello world", "testbot"));
    assert!(!check_mention("@otherbot hello", "testbot"));
    assert!(!check_mention("@testbot hello", ""));
}

#[test]
fn test_truncate_text_no_limit() {
    let text = "Hello, world!";
    let result = truncate_text(text, 0);
    assert_eq!(result, text);
}

#[test]
fn test_truncate_text_short_enough() {
    let text = "Hello";
    let result = truncate_text(text, 10);
    assert_eq!(result, text);
}

#[test]
fn test_truncate_text_needs_truncation() {
    let text = "Hello, world!";
    let result = truncate_text(text, 8);
    assert_eq!(result, "Hello...");
    assert!(result.len() <= 8);
}

#[test]
fn test_truncate_text_exact_length() {
    let text = "Hello";
    let result = truncate_text(text, 5);
    assert_eq!(result, "Hello");
}

#[test]
fn test_truncate_text_utf8_ascii() {
    let text = "Hello, world!";
    let result = truncate_text(text, 5);
    assert_eq!(result, "He...");
}

#[test]
fn test_truncate_text_utf8_chinese() {
    let text = "你好世界测试";
    let result = truncate_text(text, 3);
    assert_eq!(result, "...");
}

#[test]
fn test_truncate_text_utf8_chinese_more() {
    let text = "你好世界测试";
    let result = truncate_text(text, 5);
    assert_eq!(result, "你好...");
}

#[test]
fn test_truncate_text_utf8_mixed() {
    let text = "Hello世界";
    let result = truncate_text(text, 6);
    assert_eq!(result, "Hel...");
}

#[test]
fn test_truncate_text_empty() {
    let text = "";
    let result = truncate_text(text, 10);
    assert_eq!(result, "");
}

#[test]
fn test_streaming_config_default() {
    use crate::config::StreamingConfig;
    let config = StreamingConfig::default();
    assert_eq!(config.max_act_chars, 500);
    assert!(config.show_act_phase);
    assert_eq!(config.act_emoji, "⚡");
}

#[test]
fn test_settings_default_streaming() {
    use crate::config::Settings;
    let settings = Settings::default();
    assert_eq!(settings.streaming.max_act_chars, 500);
    assert!(settings.streaming.show_act_phase);
}
