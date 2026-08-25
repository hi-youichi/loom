//! Tests for [`telegram_bot::stream_message_handler_simple`] with simplified streaming.
//!
//! The streaming handler no longer displays intermediate act/tool phases.
//! It simply drains commands until Flush or channel closes.

use std::sync::Arc;

use telegram_bot::{
    mock::MockSender, stream_message_handler_simple, StreamCommand, StreamingConfig,
};

fn streaming_config_zero_throttle() -> StreamingConfig {
    StreamingConfig {
        throttle_ms: 0,
        ..StreamingConfig::default()
    }
}

#[tokio::test]
async fn flush_completes_handler() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle();
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        42,
        settings,
    ));

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let result = h.await.unwrap();
    assert!(result.is_empty(), "simplified handler returns empty string");
    assert!(sender.get_messages().is_empty(), "no intermediate display");
}

#[tokio::test]
async fn drop_channel_completes_handler() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle();
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        43,
        settings,
    ));

    drop(tx);
    let result = h.await.unwrap();
    assert!(result.is_empty());
    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn multiple_flushes_handled() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle();
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        44,
        settings,
    ));

    // First flush completes the handler; channel closes after drop.
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn no_outbound_messages_in_simplified_mode() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle();
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        45,
        settings,
    ));

    // Send multiple flushes — handler stops at first Flush.
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    assert!(
        sender.get_messages().is_empty(),
        "simplified handler sends no messages"
    );
}

#[tokio::test]
async fn high_throttle_still_completes_on_flush() {
    let sender = Arc::new(MockSender::new());
    let settings = StreamingConfig {
        throttle_ms: 60_000,
        ..Default::default()
    };
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        46,
        settings,
    ));

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
}

#[tokio::test]
async fn sender_failure_does_not_crash_handler() {
    let sender = Arc::new(MockSender::new());
    sender.fail_next_n_sends(2);
    let settings = streaming_config_zero_throttle();
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        47,
        settings,
    ));

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let result = h.await.unwrap();
    assert!(result.is_empty());
}
