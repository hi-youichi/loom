//! Mock tests for [`telegram_bot::stream_message_handler`] (E2E-TG-018 / 024 / 026 / 031 / 032).

use std::sync::Arc;

use telegram_bot::{
    mock::MockSender,
    stream_message_handler, StreamCommand, StreamingConfig,
};

fn streaming_config_zero_throttle(base: StreamingConfig) -> StreamingConfig {
    StreamingConfig {
        throttle_ms: 0,
        ..base
    }
}

#[tokio::test]
async fn e2e_tg_018_show_think_phase_false_skips_think_header() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_think_phase: false,
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    let h = tokio::spawn(stream_message_handler(
        rx,
        sender.clone(),
        42,
        settings,
    ));

    tx.send(StreamCommand::StartThink { count: 1 })
        .await
        .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();

    let msgs = sender.get_messages();
    let joined: String = msgs.iter().map(|(_, t)| t.as_str()).collect();
    assert!(!joined.contains("Think #"));
}

#[tokio::test]
async fn e2e_tg_024_show_act_phase_false_skips_act_header() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_think_phase: true,
        show_act_phase: false,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler(
        rx,
        sender.clone(),
        43,
        settings,
    ));

    tx.send(StreamCommand::StartThink { count: 1 })
        .await
        .unwrap();
    tx.send(StreamCommand::ThinkContent {
        content: "reasoning".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();

    let joined: String = sender.get_messages().iter().map(|(_, t)| t.as_str()).collect();
    assert!(joined.contains("Think #"));
    assert!(!joined.contains("Act #"));
}

#[tokio::test]
async fn e2e_tg_026_both_phases_disabled_no_outbound() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_think_phase: false,
        show_act_phase: false,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    let h = tokio::spawn(stream_message_handler(
        rx,
        sender.clone(),
        44,
        settings,
    ));

    tx.send(StreamCommand::StartThink { count: 1 })
        .await
        .unwrap();
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn e2e_tg_031_think_stream_respects_max_think_chars() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_think_phase: true,
        max_think_chars: 30,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler(
        rx,
        sender.clone(),
        45,
        settings,
    ));

    tx.send(StreamCommand::StartThink { count: 1 })
        .await
        .unwrap();
    let chunk = "a".repeat(80);
    tx.send(StreamCommand::ThinkContent { content: chunk })
        .await
        .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();

    let last = sender
        .get_messages()
        .last()
        .map(|(_, t)| t.clone())
        .unwrap_or_default();
    assert!(
        last.chars().count() <= 30,
        "expected truncation to max_think_chars, got len {}",
        last.chars().count()
    );
    assert!(last.ends_with("..."));
}

#[tokio::test]
async fn e2e_tg_032_act_shows_tool_start_and_end_lines() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_think_phase: false,
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler(
        rx,
        sender.clone(),
        46,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "list_dir".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "list_dir".to_string(),
        result: "ok".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();

    let joined: String = sender.get_messages().iter().map(|(_, t)| t.as_str()).collect();
    assert!(joined.contains("🔧"));
    assert!(joined.contains("list_dir"));
    assert!(joined.contains('✅'));
}
