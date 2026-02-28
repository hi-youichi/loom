//! E2E: run with thread_id then user_messages returns stored user and assistant messages.
//! Requires USER_MESSAGE_DB to be set (server must use SqliteUserMessageStore). We set it to a temp file before spawning.

use super::common;
use futures_util::StreamExt;
use loom::{AgentType, ClientRequest, RunRequest, ServerResponse, UserMessagesRequest};
use std::time::Duration;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn e2e_user_messages_after_run() {
    common::load_dotenv();
    let run_e2e =
        std::env::var("OPENAI_API_KEY").is_ok() || std::env::var("LOOM_E2E_RUN_AGENT").is_ok();
    if !run_e2e {
        eprintln!("skipping e2e_user_messages_after_run (set OPENAI_API_KEY or LOOM_E2E_RUN_AGENT to run)");
        return;
    }

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db_path = tmp.path().to_string_lossy().to_string();
    let prev_user_message_db = std::env::var("USER_MESSAGE_DB").ok();
    std::env::set_var("USER_MESSAGE_DB", &db_path);

    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let thread_id = "e2e-um-thread-1";
    let user_msg = "Say exactly: hello from e2e user_messages test.";

    let run_req = ClientRequest::Run(RunRequest {
        id: None,
        message: user_msg.to_string(),
        agent: AgentType::React,
        thread_id: Some(thread_id.to_string()),
        workspace_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
    });

    let read_timeout = Duration::from_secs(90);
    let (final_resp, _) = common::send_run_and_recv_end(&mut write, &mut read, &run_req, read_timeout)
        .await
        .expect("run should complete");

    match &final_resp {
        ServerResponse::RunEnd(r) => assert!(!r.reply.is_empty(), "run_end reply should be non-empty"),
        ServerResponse::Error(e) => panic!("run failed: {}", e.error),
        _ => panic!("expected RunEnd or Error, got {:?}", final_resp),
    }

    let um_req = ClientRequest::UserMessages(UserMessagesRequest {
        id: "um-1".to_string(),
        thread_id: thread_id.to_string(),
        before: None,
        limit: Some(50),
    });
    let (um_resp, _) = common::send_and_recv(&mut write, &mut read, &um_req)
        .await
        .expect("user_messages request should get response");

    match &um_resp {
        ServerResponse::UserMessages(r) => {
            assert_eq!(r.thread_id, thread_id);
            let has_user = r.messages.iter().any(|m| m.role == "user" && m.content.contains(user_msg));
            let has_assistant = r.messages.iter().any(|m| m.role == "assistant");
            assert!(has_user, "user_messages should contain the run's user message");
            assert!(has_assistant, "user_messages should contain at least one assistant message");
        }
        ServerResponse::Error(e) => panic!("user_messages failed: {}", e.error),
        _ => panic!("expected UserMessages or Error, got {:?}", um_resp),
    }

    drop(write);
    let _ = server_handle.await;

    if let Some(p) = prev_user_message_db {
        std::env::set_var("USER_MESSAGE_DB", p);
    } else {
        std::env::remove_var("USER_MESSAGE_DB");
    }
}
