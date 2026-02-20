//! Runs the React agent via the server. Skipped unless OPENAI_API_KEY or LOOM_E2E_RUN_AGENT is set.

use super::common;
use futures_util::StreamExt;
use loom::{AgentType, ClientRequest, RunRequest, ServerResponse};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn e2e_run_react() {
    common::load_dotenv();
    let run_e2e =
        std::env::var("OPENAI_API_KEY").is_ok() || std::env::var("LOOM_E2E_RUN_AGENT").is_ok();
    if !run_e2e {
        eprintln!("skipping e2e_run_react (set OPENAI_API_KEY or LOOM_E2E_RUN_AGENT to run)");
        return;
    }

    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, mut read) = ws.split();

    let id = "run-react-1".to_string();
    let req = ClientRequest::Run(RunRequest {
        id: id.clone(),
        message: "Reply with exactly the word: OK".to_string(),
        agent: AgentType::React,
        thread_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
    });
    let read_timeout = Duration::from_secs(120);
    let (resp, received) = common::send_run_and_recv_end(&mut write, &mut read, &req, read_timeout)
        .await
        .unwrap();

    match &resp {
        ServerResponse::RunEnd(r) => {
            assert!(
                received.contains("\"type\":\"run_end\"") && received.contains("\"reply\""),
                "expected run_end with reply, received: {}",
                received
            );
            assert_eq!(r.id, id);
            assert!(
                !r.reply.is_empty(),
                "expected non-empty reply, got {:?}",
                r.reply
            );
            assert!(
                r.reply.to_uppercase().contains("OK"),
                "expected reply to contain OK, got {:?}",
                r.reply
            );
        }
        ServerResponse::Error(e) => {
            panic!(
                "server run error (check OPENAI_API_KEY / config): {} (id={:?})",
                e.error, e.id
            );
        }
        _ => panic!("expected RunEnd or Error, got {:?}", resp),
    }

    drop(write);
    drop(read);
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}
