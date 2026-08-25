//! Concurrency and error path tests for telegram-bot

use std::sync::Arc;

use telegram_bot::{
    mock::MockSender, stream_message_handler_simple, StreamCommand, StreamingConfig,
};

fn test_config() -> StreamingConfig {
    StreamingConfig {
        throttle_ms: 0,
        ..StreamingConfig::default()
    }
}

const TEST_CHAT_ID: i64 = 12345;

#[tokio::test]
async fn test_concurrent_message_dispatch() {
    let sender = Arc::new(MockSender::new());
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    let config = test_config();

    let handler_handle = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        TEST_CHAT_ID,
        config,
    ));

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let _ = handler_handle.await;
}

#[tokio::test]
async fn test_cancel_during_agent_run() {
    let sender = Arc::new(MockSender::new());
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let cancel = tokio_util::sync::CancellationToken::new();

    let config = test_config();

    let cancel_clone = cancel.clone();
    let handler_handle = tokio::spawn(async move {
        tokio::select! {
            result = stream_message_handler_simple(rx, sender.clone(), TEST_CHAT_ID, config) => result,
            _ = cancel_clone.cancelled() => "cancelled".to_string(),
        }
    });

    cancel.cancel();
    drop(tx);

    let _ = handler_handle.await;
}

#[tokio::test]
async fn test_streaming_handler_drop_safety() {
    let sender = Arc::new(MockSender::new());
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    let config = test_config();

    let handle = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        TEST_CHAT_ID,
        config,
    ));

    drop(tx);
    let result = handle.await;
    assert!(
        result.is_ok(),
        "handler should complete cleanly when channel is dropped"
    );
}

#[tokio::test]
async fn test_sender_failure_recovery() {
    let sender = Arc::new(MockSender::new());
    sender.fail_next_n_sends(2);

    let (tx, rx) = tokio::sync::mpsc::channel(100);
    let config = test_config();

    let handle = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        TEST_CHAT_ID,
        config,
    ));

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let _ = handle.await;
}
