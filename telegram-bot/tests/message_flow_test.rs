//! Integration tests for message handling flow
//!
//! Tests the message processing logic without requiring Telegram API.

use std::sync::Arc;
use telegram_bot::{InteractionMode, Settings, StreamingConfig};

#[test]
fn test_settings_with_mention_required() {
    let settings = Settings {
        only_respond_when_mentioned: true,
        streaming: StreamingConfig::default(),
        ..Default::default()
    };

    assert!(settings.only_respond_when_mentioned);
}

#[test]
fn test_settings_without_mention_required() {
    let settings = Settings::default();
    assert!(!settings.only_respond_when_mentioned);
}

#[test]
fn test_streaming_config_custom() {
    let config = StreamingConfig {
        
        max_act_chars: 800,
        
        show_act_phase: true,
        act_emoji: "🚀".to_string(),
        throttle_ms: 200,
        max_retries: 5,
        ..Default::default()
    };

    assert_eq!(config.max_act_chars, 800);
    assert!(config.show_act_phase);
    assert_eq!(config.act_emoji, "🚀");
    assert_eq!(config.throttle_ms, 200);
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.interaction_mode, InteractionMode::PeriodicSummary);
}

#[test]
fn test_text_cleaning_removes_mention() {
    let text = "@mybot Tell me a joke";
    let bot_username = "mybot";

    let mention = format!("@{} ", bot_username);
    let clean_text = text.replace(&mention, "").replace(&format!("@{}", bot_username), "");

    assert_eq!(clean_text, "Tell me a joke");
}

#[test]
fn test_text_cleaning_handles_mention_at_end() {
    let text = "Hello @mybot";
    let bot_username = "mybot";

    let mention = format!("@{} ", bot_username);
    let clean_text = text.replace(&mention, "").replace(&format!("@{}", bot_username), "");

    assert_eq!(clean_text, "Hello ");
}

#[test]
fn test_text_cleaning_handles_case_insensitive() {
    let text = "@MyBot What is this?";
    let bot_username = "mybot";

    // Note: The actual implementation does case-sensitive removal
    let mention = format!("@{} ", bot_username);
    let clean_text = text.replace(&mention, "").replace(&format!("@{}", bot_username), "");

    // This shows that case-sensitive matching may miss mentions
    assert_ne!(clean_text, "What is this?");
}

#[test]
fn test_prompt_formatting_for_reply() {
    let replied_text = "Original message";
    let user_reply = "My reply";

    let prompt = format!(
        "[Replying to this message]:\n{}\n\n[User's reply]:\n{}",
        replied_text, user_reply
    );

    assert!(prompt.contains("[Replying to this message]:"));
    assert!(prompt.contains(replied_text));
    assert!(prompt.contains("[User's reply]:"));
    assert!(prompt.contains(user_reply));
}

#[test]
fn test_thread_id_format() {
    let chat_id = 123456789i64;
    let thread_id = format!("telegram_{}", chat_id);

    assert_eq!(thread_id, "telegram_123456789");
}

#[test]
fn test_command_detection() {
    // Test /reset command detection
    let text1 = "/reset";
    assert!(text1.trim() == "/reset" || text1.trim().starts_with("/reset "));

    let text2 = "/reset session1";
    assert!(text2.trim() == "/reset" || text2.trim().starts_with("/reset "));

    let text3 = "/status";
    assert!(text3.trim() == "/status");

    let text4 = "/other";
    assert!(text4.trim() != "/status");
    assert!(!text4.trim().starts_with("/reset"));
}

#[test]
fn test_bot_mention_detection() {
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
fn test_arc_settings_sharing() {
    let settings = Arc::new(Settings::default());
    let settings_clone = Arc::clone(&settings);

    assert_eq!(
        settings.only_respond_when_mentioned,
        settings_clone.only_respond_when_mentioned
    );
}

#[test]
fn test_arc_username_sharing() {
    let username = Arc::new("mybot".to_string());
    let username_clone = Arc::clone(&username);

    assert_eq!(*username, *username_clone);
}

// ============================================================================
// Async Tests (using tokio-test)
// ============================================================================

#[tokio::test]
async fn test_settings_clone_async() {
    let settings = Arc::new(Settings::default());
    let cloned = Arc::clone(&settings);

    // Simulate async operation
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    assert_eq!(
        settings.only_respond_when_mentioned,
        cloned.only_respond_when_mentioned
    );
}

#[tokio::test]
async fn test_streaming_config_defaults_async() {
    let config = StreamingConfig::default();

    assert_eq!(config.interaction_mode, InteractionMode::PeriodicSummary);
    assert_eq!(config.max_act_chars, 500);
    assert!(config.show_act_phase);
    assert_eq!(config.act_emoji, "⚡");
    assert_eq!(config.summary_interval_secs, 300);
    assert!(config.ack_placeholder_text.contains("已收到"));
}
