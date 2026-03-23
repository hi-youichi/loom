//! Integration tests using Mock implementations

use telegram_bot::{
    mock::{MockAgentRunner, MockSender, MockSessionManager},
    AgentRunner, MessageSender, SessionManager, Settings,
};
use std::sync::Arc;

// ============================================================================
// P0: Command Tests
// ============================================================================

#[tokio::test]
async fn test_mock_sender_records_messages() {
    let concrete = Arc::new(MockSender::new());
    let sender: Arc<dyn MessageSender> = concrete.clone();

    sender.send_text(123, "Hello").await.unwrap();
    sender.send_text(456, "World").await.unwrap();

    let messages = concrete.get_messages();
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn test_mock_sender_clear() {
    let sender = MockSender::new();
    
    sender.send_text(123, "Test").await.unwrap();
    assert_eq!(sender.get_messages().len(), 1);
    
    sender.clear();
    assert_eq!(sender.get_messages().len(), 0);
}

#[tokio::test]
async fn test_mock_agent_runner_returns_response() {
    let agent = MockAgentRunner::new("Test response");
    
    let result = agent.run("What is 2+2?", 123, None).await.unwrap();
    
    assert_eq!(result, "Test response");
    assert_eq!(agent.get_calls().len(), 1);
    assert_eq!(agent.get_calls()[0], "What is 2+2?");
}

#[tokio::test]
async fn test_mock_agent_runner_failing() {
    let agent = MockAgentRunner::failing();
    
    let result = agent.run("test", 123, None).await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_session_manager() {
    let session = MockSessionManager::new();
    
    let count = session.reset("telegram_123").await.unwrap();
    assert_eq!(count, 1);

    let count = session.reset("telegram_456").await.unwrap();
    assert_eq!(count, 1);

    assert_eq!(session.reset_count(), 2);
}

// ============================================================================
// P1: Configuration Tests
// ============================================================================

#[test]
fn test_settings_with_mention_required() {
    let settings = Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    };
    
    assert!(settings.only_respond_when_mentioned);
}

#[test]
fn test_settings_without_mention_required() {
    let settings = Settings::default();
    
    assert!(!settings.only_respond_when_mentioned);
}

// ============================================================================
// P2: Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_empty_message() {
    let sender = MockSender::new();
    
    sender.send_text(123, "").await.unwrap();
    
    let messages = sender.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, "");
}

#[tokio::test]
async fn test_long_message() {
    use telegram_bot::MessageSender;
    let sender = MockSender::new();
    let long_text = "a".repeat(10000);
    
    sender.send_text(123, &long_text).await.unwrap();
    
    let messages = sender.get_messages();
    assert_eq!(messages[0].1.len(), 10000);
}

#[tokio::test]
async fn test_special_characters() {
    let sender = MockSender::new();
    
    sender.send_text(123, "🎉🔥💥 Test 中文 العربية").await.unwrap();
    
    let messages = sender.get_messages();
    assert!(messages[0].1.contains("🎉"));
    assert!(messages[0].1.contains("中文"));
}

// ============================================================================
// Test Statistics
// ============================================================================

// Total tests: 11
// P0: 5 tests (Mock implementations)
// P1: 2 tests (Configuration)
// P2: 3 tests (Edge cases)
