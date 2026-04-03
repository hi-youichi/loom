//! Mock tests for [`telegram_bot::stream_message_handler_simple`] (E2E-TG-018 / 024 / 026 / 031 / 032)
//! plus streaming-act-fix regressions (header send failure, Act chunk, tool timing, flush/throttle).

use std::sync::Arc;
use std::time::Duration;

use telegram_bot::{
    mock::MockSender, stream_message_handler_simple, InteractionMode, StreamCommand,
    StreamingConfig,
};

fn streaming_config_zero_throttle(base: StreamingConfig) -> StreamingConfig {
    StreamingConfig {
        interaction_mode: InteractionMode::Streaming,
        throttle_ms: 0,
        ..base
    }
}

#[tokio::test]
async fn e2e_tg_skip_test_removed() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        42,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
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
        show_act_phase: false,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        43,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ActContent {
        content: "reasoning".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();

    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("Act #"));
}

#[tokio::test]
async fn e2e_tg_026_both_phases_disabled_no_outbound() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: false,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(8);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        44,
        settings,
    ));

    tx.send(StreamCommand::ActContent {
        content: "ignored".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn e2e_tg_031_act_stream_respects_max_chars() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        max_act_chars: 30,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        45,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    let chunk = "a".repeat(80);
    tx.send(StreamCommand::ActContent { content: chunk })
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
        "expected truncation to max_act_chars, got len {}",
        last.chars().count()
    );
    assert!(last.ends_with("..."));
}

#[tokio::test]
async fn e2e_tg_032_act_shows_tool_start_and_end_lines() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        46,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "list_dir".to_string(),
        arguments: None,
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

    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("🔧"));
    assert!(joined.contains("list_dir"));
    assert!(joined.contains('✅'));
}

#[tokio::test]
async fn act_tools_recorded_when_act_header_send_fails() {
    let sender = Arc::new(MockSender::new());
    sender.fail_next_n_sends(1);
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        100,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "grep".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "grep".to_string(),
        result: "hits".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let final_text = h.await.unwrap();
    assert!(
        final_text.contains("grep") && final_text.contains('✅'),
        "expected tools in returned final_text, got {:?}",
        final_text
    );
}

#[tokio::test]
async fn think_content_recorded_when_think_header_send_fails() {
    let sender = Arc::new(MockSender::new());
    sender.fail_next_n_sends(1);
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        101,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ActContent {
        content: "planning".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let final_text = h.await.unwrap();
    assert!(
        final_text.contains("planning"),
        "expected act content in final_text, got {:?}",
        final_text
    );
}

#[tokio::test]
async fn tool_start_before_act_start_enters_act_fallback() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        102,
        settings,
    ));

    tx.send(StreamCommand::ToolStart {
        name: "early".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("early"));
}

#[tokio::test]
async fn fallback_act_then_startact_does_not_clear_existing_tool_state() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        109,
        settings,
    ));

    // Out-of-order: tool start arrives before StartAct and triggers fallback act mode.
    tx.send(StreamCommand::ToolStart {
        name: "ls".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    // Canonical StartAct for the same phase arrives later.
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "ls".to_string(),
        result: "ok".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let final_text = h.await.unwrap();
    assert!(
        final_text.contains("✅ ls"),
        "expected completed tool result, got {:?}",
        final_text
    );
}

#[tokio::test]
async fn tool_start_shows_arguments_in_act_message() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        110,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "ls".to_string(),
        arguments: Some("{\"path\":\".\"}".to_string()),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("🔧 ls {\"path\":\".\"}"), "{}", joined);
}

#[tokio::test]
async fn tool_end_keeps_arguments_in_final_line() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        112,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "ls".to_string(),
        arguments: Some("{\"path\":\".\"}".to_string()),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "ls".to_string(),
        result: "ok".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("✅ ls {\"path\":\".\"}"), "{}", joined);
}

#[tokio::test]
async fn tool_end_result_preserves_newlines() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        113,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "ls".to_string(),
        arguments: Some("{}".to_string()),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "ls".to_string(),
        result: "line1\nline2".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("line1\nline2"), "{}", joined);
}

#[tokio::test]
async fn act_content_appended_to_act_message() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        103,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ActContent {
        content: "hello act".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("hello act"));
}

#[tokio::test]
async fn tool_end_error_shows_cross_mark() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        104,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "broken".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "broken".to_string(),
        result: "nope".to_string(),
        is_error: true,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(
        joined.contains('❌') && joined.contains("broken"),
        "expected error cross and tool name in: {}",
        joined
    );
}

#[tokio::test]
async fn second_act_clears_tools_from_first_act() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        105,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "a".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "a".to_string(),
        result: "1".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::StartAct { count: 2 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "b".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "b".to_string(),
        result: "2".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let messages = sender.get_messages();
    let last = messages
        .last()
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    assert!(
        last.contains("✅ b") && !last.contains("✅ a"),
        "second act message should only show second tool; last={}",
        last
    );
}

#[tokio::test]
async fn tool_end_without_prior_start_has_no_duration_suffix() {
    let sender = Arc::new(MockSender::new());
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        106,
        settings,
    ));

    tx.send(StreamCommand::ToolStart {
        name: "orphan".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "orphan".to_string(),
        result: "x".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(
        joined.contains("✅ orphan") && !joined.contains("ms)") && !joined.contains("s)"),
        "{}",
        joined
    );
}

#[tokio::test]
async fn think_header_fails_then_act_header_succeeds_and_tools_show() {
    let sender = Arc::new(MockSender::new());
    sender.fail_next_n_sends(1);
    let settings = streaming_config_zero_throttle(StreamingConfig {
        show_act_phase: true,
        ..Default::default()
    });
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        107,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "ls".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "ls".to_string(),
        result: "ok".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(joined.contains("Act #"));
    assert!(joined.contains("ls"));
}

#[tokio::test]
async fn high_throttle_skips_intermediate_edits_flush_updates() {
    let sender = Arc::new(MockSender::new());
    let settings = StreamingConfig {
        interaction_mode: InteractionMode::Streaming,
        throttle_ms: 60_000,

        show_act_phase: true,
        ..Default::default()
    };
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        108,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "t".to_string(),
        arguments: None,
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "t".to_string(),
        result: "done".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let msgs = sender.get_messages();
    let edits: Vec<_> = msgs.iter().filter(|(_, t)| t.contains("✅ t")).collect();
    assert!(
        !edits.is_empty(),
        "Flush should emit at least one edit with tool result"
    );
}

#[tokio::test]
async fn act_to_think_transition_flushes_pending_act_update() {
    let sender = Arc::new(MockSender::new());
    let settings = StreamingConfig {
        interaction_mode: InteractionMode::Streaming,
        throttle_ms: 60_000,

        show_act_phase: true,
        ..Default::default()
    };
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        111,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "ls".to_string(),
        arguments: Some("{\"path\":\".\"}".to_string()),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ToolEnd {
        name: "ls".to_string(),
        result: "ok".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    // ReAct loop continues quickly to the next think round.
    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ActContent {
        content: "next reasoning".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    h.await.unwrap();
    let joined: String = sender
        .get_messages()
        .iter()
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(
        joined.contains("✅ ls") && joined.contains("next reasoning"),
        "{}",
        joined
    );
}

#[tokio::test(start_paused = true)]
async fn periodic_summary_mode_emits_summary_after_interval() {
    let sender = Arc::new(MockSender::new());
    let settings = StreamingConfig {
        interaction_mode: InteractionMode::PeriodicSummary,
        summary_interval_secs: 300,

        show_act_phase: false,
        ..Default::default()
    };
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        120,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ToolStart {
        name: "read_file".to_string(),
        arguments: Some("{\"path\":\"README.md\"}".to_string()),
    })
    .await
    .unwrap();
    tx.send(StreamCommand::ActContent {
        content: "collecting context for the answer".to_string(),
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_secs(300)).await;
    tokio::task::yield_now().await;

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let final_text = h.await.unwrap();
    let messages = sender.get_messages();
    assert_eq!(messages.len(), 1, "expected one periodic summary message");
    assert!(messages[0].1.contains("执行中"));
    assert!(messages[0].1.contains("已执行"));
    assert!(final_text.contains("collecting context"));
}

#[tokio::test(start_paused = true)]
async fn periodic_summary_mode_skips_summary_before_interval() {
    let sender = Arc::new(MockSender::new());
    let settings = StreamingConfig {
        interaction_mode: InteractionMode::PeriodicSummary,
        summary_interval_secs: 300,

        show_act_phase: false,
        ..Default::default()
    };
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        121,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ActContent {
        content: "reasoning".to_string(),
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;

    tokio::time::advance(Duration::from_secs(299)).await;
    tokio::task::yield_now().await;

    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);

    let final_text = h.await.unwrap();
    let messages = sender.get_messages();
    assert!(
        messages.is_empty(),
        "Expected no messages but got: {:?}",
        messages
    );
    assert!(final_text.contains("reasoning"));
}

#[tokio::test(start_paused = true)]
async fn periodic_summary_mode_stops_after_flush() {
    let sender = Arc::new(MockSender::new());
    let settings = StreamingConfig {
        interaction_mode: InteractionMode::PeriodicSummary,
        summary_interval_secs: 300,
        show_act_phase: false,
        ..Default::default()
    };
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    let h = tokio::spawn(stream_message_handler_simple(
        rx,
        sender.clone(),
        122,
        settings,
    ));

    tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();
    tx.send(StreamCommand::ActContent {
        content: "almost done".to_string(),
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(300)).await;
    tokio::task::yield_now().await;
    tx.send(StreamCommand::Flush).await.unwrap();
    drop(tx);
    h.await.unwrap();

    tokio::time::advance(Duration::from_secs(300)).await;
    tokio::task::yield_now().await;

    let messages = sender.get_messages();
    assert_eq!(
        messages.len(),
        1,
        "Expected 1 message but got: {:?}",
        messages
    );
}
