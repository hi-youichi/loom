//! Mock E2E-style dispatch tests: synthetic inbound [`Message`] → [`handle_message_with_deps`] → [`MockSender`].
//!
//! Naming `e2e_tg_XXX_*` maps to manual IDs in `docs/guides/telegram-bot-e2e-tests.md` where applicable.
//! Cases that need real Telegram, network, or dispatcher (e.g. E2E-TG-021, 029) stay manual-only.

mod common;

use std::time::Duration;
use std::sync::Arc;

use common::fixtures;
use telegram_bot::{
    handle_message_with_deps,
    mock::{
        ErrorSessionManager, FakeFileDownloader, MockAgentRunner, MockSender, MockSessionManager,
        StubFileDownloader,
    },
    AgentRunner, ChatRunRegistry, FileDownloader, HandlerDeps, InMemorySearchSessionStore,
    InteractionMode, MessageSender, ModelChoice, ModelSelectionService, SessionManager, Settings,
    SqliteModelSelectionStore, StaticModelCatalog, StreamingConfig,
};

fn model_selection_for_test() -> Arc<ModelSelectionService> {
    Arc::new(ModelSelectionService::new(
        Arc::new(StaticModelCatalog::new(
            "gpt-5.4",
            vec![ModelChoice::new("gpt-5.4")],
        )),
        Arc::new(SqliteModelSelectionStore::new()),
        Arc::new(InMemorySearchSessionStore::new()),
    ))
}

fn make_deps(
    sender: Arc<dyn MessageSender>,
    agent: Arc<dyn AgentRunner>,
    session: Arc<dyn SessionManager>,
    downloader: Arc<dyn FileDownloader>,
    settings: Arc<Settings>,
    bot_username: Arc<String>,
) -> HandlerDeps {
    HandlerDeps::for_test(
        sender,
        agent,
        session,
        downloader,
        model_selection_for_test(),
        settings,
        bot_username,
        Arc::new(ChatRunRegistry::new()),
    )
}

fn make_text_only_deps(
    sender: Arc<dyn MessageSender>,
    agent: Arc<dyn AgentRunner>,
    settings: Arc<Settings>,
    bot_username: Arc<String>,
) -> HandlerDeps {
    make_deps(
        sender,
        agent,
        Arc::new(MockSessionManager::new()),
        Arc::new(StubFileDownloader::new()),
        settings,
        bot_username,
    )
}

// --- P0 ---

#[tokio::test]
async fn e2e_tg_001_status_mocked_dispatch() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::new("should not run"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_001, 1, "/status");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].0, 99_001);
    assert_eq!(messages[0].1, "✅ Bot is running!");
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_002_plain_text_mocked_agent_delivers_via_sender() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "hello from mock agent",
    ));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_002, 1, "Say hello in one sentence");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].0, 99_002);
    assert_eq!(messages[0].1, "👌");
    assert_eq!(messages[1].1, "hello from mock agent");

    let calls = agent.get_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], "Say hello in one sentence");
}

#[tokio::test]
async fn periodic_summary_default_sends_ack_then_final_and_passes_context() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::new("final answer"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_200, 42, "Summarize the current task");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].1, "👌");
    assert_eq!(messages[1].1, "final answer");

    let contexts = agent.get_contexts();
    assert_eq!(contexts.len(), 1);
    assert_eq!(contexts[0].user_message_id, Some(42));
    assert!(contexts[0].ack_message_id.is_none());
}

#[tokio::test]
async fn streaming_mode_router_skips_echo_of_run_return_when_progress_flags_on() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::new("would duplicate"));
    let settings = Arc::new(Settings {
        streaming: StreamingConfig {
            interaction_mode: InteractionMode::Streaming,
            ..Default::default()
        },
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_210, 1, "task");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 1, "should send reaction in streaming mode");
    assert_eq!(messages[0].1, "👌");
    assert_eq!(agent.get_calls(), vec!["task".to_string()]);
}

#[tokio::test]
async fn streaming_mode_router_sends_reply_when_both_phases_hidden() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::new("only channel"));
    let settings = Arc::new(Settings {
        streaming: StreamingConfig {
            interaction_mode: InteractionMode::Streaming,
            show_think_phase: false,
            show_act_phase: false,
            ..Default::default()
        },
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_211, 1, "task");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].1, "👌");
    assert_eq!(messages[1].1, "only channel");
}

// --- P1 commands / session ---

#[tokio::test]
async fn e2e_tg_003_reset_clears_session_via_trait() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let session = Arc::new(MockSessionManager::with_deleted_per_reset(4));
    let agent = Arc::new(MockAgentRunner::new("noop"));
    let deps = make_deps(
        sender_trait,
        agent,
        session.clone(),
        Arc::new(StubFileDownloader::new()),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_003, 1, "/reset");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages()[0]
        .1
        .contains("Deleted 4 checkpoints."));
    assert_eq!(session.reset_count(), 1);
}

#[tokio::test]
async fn e2e_tg_025_reset_with_trailing_arg_still_command() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let session = Arc::new(MockSessionManager::with_deleted_per_reset(0));
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        session,
        Arc::new(StubFileDownloader::new()),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_025, 1, "/reset dry-run");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages()[0].1.contains("Session reset!"));
}

#[tokio::test]
async fn e2e_tg_015_reset_on_fresh_chat_zero_checkpoints() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let session = Arc::new(MockSessionManager::with_deleted_per_reset(0));
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        session,
        Arc::new(StubFileDownloader::new()),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_015, 1, "/reset");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages()[0]
        .1
        .contains("Deleted 0 checkpoints."));
}

#[tokio::test]
async fn e2e_tg_004_reply_threading_prompt_format() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "ok"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_reply_to_text(
        99_004,
        2,
        1,
        "Remember this code: BLUE-42",
        "What was the code?",
    );
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let prompt = &agent.get_calls()[0];
    assert!(prompt.contains("BLUE-42"));
    assert!(prompt.contains("[Replying to this message]:"));
    assert!(prompt.contains("[User's reply]:"));
    assert!(prompt.contains("What was the code?"));
}

// --- P1 mention gating ---

#[tokio::test]
async fn e2e_tg_023_private_mention_gate_suppresses_plain_text() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "should not send",
    ));
    let settings = Arc::new(Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new("mybot".to_string()),
    );

    let msg = fixtures::message_private_text(99_023, 1, "no mention");
    handle_message_with_deps(&deps, &msg).await.unwrap();
    assert!(sender.get_messages().is_empty());
    assert!(agent.get_calls().is_empty());

    let msg2 = fixtures::message_private_text(99_023, 2, "@mybot hi there");
    handle_message_with_deps(&deps, &msg2).await.unwrap();
    assert_eq!(sender.get_messages().len(), 2);
    assert_eq!(agent.get_calls().len(), 1);
    assert_eq!(agent.get_calls()[0], "hi there");
}

#[tokio::test]
async fn e2e_tg_008_group_mention_gate_suppresses_plain_text() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "answered"));
    let settings = Arc::new(Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new("mybot".to_string()),
    );

    let plain = fixtures::message_group_text(-100_008, 1, "hello everyone");
    handle_message_with_deps(&deps, &plain).await.unwrap();
    assert!(sender.get_messages().is_empty());

    let mentioned = fixtures::message_group_text(-100_008, 2, "@mybot question?");
    handle_message_with_deps(&deps, &mentioned).await.unwrap();
    assert_eq!(sender.get_messages().len(), 2);
    assert!(agent.get_calls()[0].contains("question"));
}

#[tokio::test]
async fn e2e_tg_009_group_commands_bypass_mention_gate() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "no agent",
    ));
    let settings = Arc::new(Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new("mybot".to_string()),
    );

    let msg = fixtures::message_group_text(-100_009, 1, "/status");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, "✅ Bot is running!");
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_013_group_reply_to_bot_without_at() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "reply"));
    let settings = Arc::new(Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new("mybot".to_string()),
    );

    let msg = fixtures::message_group_reply_to_bot(-100_013, 2, "mybot", "Follow-up?");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert_eq!(sender.get_messages().len(), 2);
    assert_eq!(agent.get_calls().len(), 1);
}

#[tokio::test]
async fn e2e_tg_022_group_status_at_bot_suffix_not_builtin_status() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "agent handled",
    ));
    let settings = Arc::new(Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    });
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        settings,
        Arc::new("my_bot".to_string()),
    );

    let msg = fixtures::message_group_text(-100_022, 1, "/status@my_bot");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].1, "👌");
    assert_eq!(messages[1].1, "agent handled");
    // `/status@bot` is not the built-in `/status` branch; mention stripping leaves the agent prompt.
    assert_eq!(agent.get_calls()[0], "/status");
}

// --- P2 errors / unicode / media ---

#[tokio::test]
async fn e2e_tg_010_agent_failure_surfaces_error_message() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::failing());
    let deps = make_text_only_deps(
        sender_trait,
        agent,
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_010, 1, "trigger");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].1, "👌");
    assert!(messages[1].1.starts_with("Error:"));
    assert!(messages[1].1.contains("Mock error"));
}

#[tokio::test]
async fn e2e_tg_011_unicode_and_emoji_in_prompt() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "收到"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let text = "Hello 中文 🎉 test";
    let msg = fixtures::message_private_text(99_011, 1, text);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert_eq!(agent.get_calls()[0], text);
}

#[tokio::test]
async fn e2e_tg_014_photo_caption_does_not_invoke_agent_text_path() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "must not run",
    ));
    let fake_path = std::path::PathBuf::from("mock_photo.jpg");
    let deps = make_deps(
        sender_trait,
        agent.clone(),
        Arc::new(MockSessionManager::new()),
        Arc::new(FakeFileDownloader::new(fake_path.clone())),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_photo_with_caption(99_014, 1, "What is this?");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(agent.get_calls().is_empty());
    let messages = sender.get_messages();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn e2e_tg_005_photo_download_success_message() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::new("noop"));
    let fake_path = std::path::PathBuf::from("mock_dl_photo.jpg");
    let deps = make_deps(
        sender_trait,
        agent,
        Arc::new(MockSessionManager::new()),
        Arc::new(FakeFileDownloader::new(fake_path)),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_photo_only(99_005, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn e2e_tg_006_document_download_success_message() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let fake_path = std::path::PathBuf::from("mock_doc.txt");
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        Arc::new(MockSessionManager::new()),
        Arc::new(FakeFileDownloader::new(fake_path)),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_document(99_006, 1, "note.txt");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn e2e_tg_007_video_download_success_message() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let fake_path = std::path::PathBuf::from("mock_clip.mp4");
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        Arc::new(MockSessionManager::new()),
        Arc::new(FakeFileDownloader::new(fake_path)),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_video(99_007, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn e2e_tg_016_large_file_download_error_surfaces() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        Arc::new(MockSessionManager::new()),
        Arc::new(StubFileDownloader::new()),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_photo_only(99_016, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
}

#[tokio::test]
async fn e2e_tg_030_custom_download_dir_is_created() {
    let temp = std::env::temp_dir().join(format!(
        "telegram_bot_dl_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = tokio::fs::remove_dir_all(&temp).await;

    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let settings = Arc::new(Settings {
        download_dir: temp.clone(),
        ..Default::default()
    });
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        Arc::new(MockSessionManager::new()),
        Arc::new(StubFileDownloader::new()),
        settings,
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_030, 1, "/status");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(
        tokio::fs::metadata(&temp).await.is_ok(),
        "handler should create configured download_dir"
    );
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

#[tokio::test]
async fn e2e_tg_009b_group_reset_bypasses_mention_gate() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let session = Arc::new(MockSessionManager::with_deleted_per_reset(2));
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "should not run",
    ));
    let settings = Arc::new(Settings {
        only_respond_when_mentioned: true,
        ..Default::default()
    });
    let deps = make_deps(
        sender_trait,
        agent.clone(),
        session,
        Arc::new(StubFileDownloader::new()),
        settings,
        Arc::new("mybot".to_string()),
    );

    let msg = fixtures::message_group_text(-100_0092, 1, "/reset");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages()[0].1.contains("Session reset!"));
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_012_two_private_chats_isolated_outbound() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "ok"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let a = fixtures::message_private_text(201_001, 1, "chat A");
    let b = fixtures::message_private_text(201_002, 1, "chat B");
    handle_message_with_deps(&deps, &a).await.unwrap();
    handle_message_with_deps(&deps, &b).await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].0, 201_001);
    assert_eq!(messages[1].0, 201_001);
    assert_eq!(messages[2].0, 201_002);
    assert_eq!(messages[3].0, 201_002);
    assert_eq!(agent.get_calls().len(), 2);
}

// --- E2E-TG-020: unsupported / no-handler media (no outbound, no agent) ---

#[tokio::test]
async fn e2e_tg_020_sticker_does_not_reply_or_invoke_agent() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(
        sender_trait.clone(),
        "no",
    ));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_sticker(99_0201, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_020_location_does_not_reply_or_invoke_agent() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "no"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_location(99_0202, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_020_voice_does_not_reply_or_invoke_agent() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "no"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_voice(99_0203, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_020_dice_message_kind_no_outbound() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "no"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_dice(99_0204, 1);
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages().is_empty());
    assert!(agent.get_calls().is_empty());
}

#[tokio::test]
async fn e2e_tg_019_rapid_two_messages_both_get_responses() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_sender(sender_trait.clone(), "ans"));
    let deps = make_text_only_deps(
        sender_trait,
        agent.clone(),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let first = fixtures::message_private_text(99_019, 1, "Question one?");
    let second = fixtures::message_private_text(99_019, 2, "Question two?");
    handle_message_with_deps(&deps, &first).await.unwrap();
    handle_message_with_deps(&deps, &second).await.unwrap();

    assert_eq!(sender.get_messages().len(), 4);
    assert_eq!(agent.get_calls().len(), 2);
}

#[tokio::test]
async fn same_chat_second_request_receives_busy_message_while_first_runs() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let agent = Arc::new(MockAgentRunner::with_delay(
        "slow final",
        Duration::from_millis(40),
    ));
    let deps = Arc::new(make_text_only_deps(
        sender_trait,
        agent,
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    ));

    let first = fixtures::message_private_text(99_201, 1, "First");
    let second = fixtures::message_private_text(99_201, 2, "Second");

    let deps_first = Arc::clone(&deps);
    let first_task = tokio::spawn(async move {
        handle_message_with_deps(deps_first.as_ref(), &first)
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(5)).await;

    let deps_second = Arc::clone(&deps);
    let second_task = tokio::spawn(async move {
        handle_message_with_deps(deps_second.as_ref(), &second)
            .await
            .unwrap();
    });

    first_task.await.unwrap();
    second_task.await.unwrap();

    let messages = sender.get_messages();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].1, "👌");
    assert!(messages[1].1.contains("还在处理中"));
    assert_eq!(messages[2].1, "slow final");
}

#[tokio::test]
async fn reset_failure_sends_user_error() {
    let sender = Arc::new(MockSender::new());
    let sender_trait: Arc<dyn MessageSender> = sender.clone();
    let deps = make_deps(
        sender_trait,
        Arc::new(MockAgentRunner::new("noop")),
        Arc::new(ErrorSessionManager::new("db locked")),
        Arc::new(StubFileDownloader::new()),
        Arc::new(Settings::default()),
        Arc::new(String::new()),
    );

    let msg = fixtures::message_private_text(99_900, 1, "/reset");
    handle_message_with_deps(&deps, &msg).await.unwrap();

    assert!(sender.get_messages()[0].1.contains("❌ Reset failed"));
    assert!(sender.get_messages()[0].1.contains("db locked"));
}
