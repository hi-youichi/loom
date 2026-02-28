//! Runs the React agent via the server. Skipped unless OPENAI_API_KEY or LOOM_E2E_RUN_AGENT is set.

use super::common;
use futures_util::{SinkExt, StreamExt};
use loom::{AgentType, ClientRequest, ProtocolEvent, RunRequest, ServerResponse};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn assert_non_empty(field: &str, value: &str) {
    assert!(
        !value.trim().is_empty(),
        "expected non-empty {}, got {:?}",
        field,
        value
    );
}

fn assert_optional_non_empty(field: &str, value: &Option<String>) {
    if let Some(v) = value {
        assert_non_empty(field, v);
    }
}

fn is_upstream_policy_error_text(text: &str) -> bool {
    let msg = text.to_lowercase();
    msg.contains("403")
        || msg.contains("forbidden")
        || msg.contains("temporarily blocked")
        || msg.contains("content policy")
        || msg.contains("stream ended without final state")
}

fn assert_protocol_event(
    event: &ProtocolEvent,
    saw_node_enter: &mut bool,
    saw_node_exit: &mut bool,
    saw_upstream_policy_error: &mut bool,
) {
    match event {
        ProtocolEvent::NodeEnter { id } => {
            assert_non_empty("event.id", id);
            *saw_node_enter = true;
        }
        ProtocolEvent::NodeExit { id, result } => {
            assert_non_empty("event.id", id);
            assert!(
                !result.is_null(),
                "expected node_exit.result to be non-null, got {:?}",
                result
            );
            if let Some(err) = result.get("Err").and_then(|v| v.as_str()) {
                if is_upstream_policy_error_text(err) {
                    *saw_upstream_policy_error = true;
                }
            }
            *saw_node_exit = true;
        }
        ProtocolEvent::MessageChunk { content, id } => {
            assert_non_empty("event.id", id);
            assert!(
                !content.is_empty(),
                "expected non-empty raw event.content, got {:?}",
                content
            );
        }
        ProtocolEvent::Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        } => {
            assert_eq!(
                *total_tokens,
                *prompt_tokens + *completion_tokens,
                "expected usage.total_tokens == prompt_tokens + completion_tokens"
            );
        }
        ProtocolEvent::Values { state } => {
            assert!(
                !state.is_null(),
                "expected values.state to be non-null, got {:?}",
                state
            );
        }
        ProtocolEvent::Updates { id, state } => {
            assert_non_empty("event.id", id);
            assert!(
                !state.is_null(),
                "expected updates.state to be non-null, got {:?}",
                state
            );
        }
        ProtocolEvent::Custom { value } => {
            assert!(
                !value.is_null(),
                "expected custom.value to be non-null, got {:?}",
                value
            );
        }
        ProtocolEvent::Checkpoint {
            checkpoint_id,
            timestamp,
            step,
            state,
            thread_id,
            checkpoint_ns,
        } => {
            assert_non_empty("event.checkpoint_id", checkpoint_id);
            assert_non_empty("event.timestamp", timestamp);
            assert!(
                *step >= 0,
                "expected checkpoint.step >= 0, got {}",
                step
            );
            assert!(
                !state.is_null(),
                "expected checkpoint.state to be non-null, got {:?}",
                state
            );
            if let Some(thread_id) = thread_id {
                assert_non_empty("event.thread_id", thread_id);
            }
            if let Some(checkpoint_ns) = checkpoint_ns {
                assert_non_empty("event.checkpoint_ns", checkpoint_ns);
            }
        }
        ProtocolEvent::TotExpand { candidates } => {
            assert!(
                !candidates.is_empty(),
                "expected non-empty tot_expand.candidates"
            );
            for candidate in candidates {
                assert_non_empty("event.candidate", candidate);
            }
        }
        ProtocolEvent::TotEvaluate { chosen, scores } => {
            assert!(!scores.is_empty(), "expected non-empty tot_evaluate.scores");
            assert!(
                *chosen < scores.len(),
                "expected chosen index within scores bounds, chosen={}, len={}",
                chosen,
                scores.len()
            );
        }
        ProtocolEvent::TotBacktrack { reason, to_depth: _ } => {
            assert_non_empty("event.reason", reason);
        }
        ProtocolEvent::GotPlan {
            node_count,
            edge_count: _,
            node_ids,
        } => {
            assert_eq!(
                *node_count,
                node_ids.len(),
                "expected got_plan.node_count == node_ids.len()"
            );
            for node_id in node_ids {
                assert_non_empty("event.node_id", node_id);
            }
        }
        ProtocolEvent::GotNodeStart { id } => {
            assert_non_empty("event.id", id);
        }
        ProtocolEvent::GotNodeComplete { id, result_summary } => {
            assert_non_empty("event.id", id);
            assert_non_empty("event.result_summary", result_summary);
        }
        ProtocolEvent::GotNodeFailed { id, error } => {
            assert_non_empty("event.id", id);
            assert_non_empty("event.error", error);
            if is_upstream_policy_error_text(error) {
                *saw_upstream_policy_error = true;
            }
        }
        ProtocolEvent::GotExpand {
            node_id,
            nodes_added: _,
            edges_added: _,
        } => {
            assert_non_empty("event.node_id", node_id);
        }
        ProtocolEvent::ToolCallChunk {
            call_id,
            name,
            arguments_delta: _,
        } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_optional_non_empty("event.name", name);
        }
        ProtocolEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_non_empty("event.name", name);
            assert!(
                arguments.is_object(),
                "expected tool_call.arguments to be object, got {:?}",
                arguments
            );
        }
        ProtocolEvent::ToolStart { call_id, name } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_non_empty("event.name", name);
        }
        ProtocolEvent::ToolOutput {
            call_id,
            name,
            content: _,
        } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_non_empty("event.name", name);
        }
        ProtocolEvent::ToolEnd {
            call_id,
            name,
            result,
            is_error,
        } => {
            assert_optional_non_empty("event.call_id", call_id);
            assert_non_empty("event.name", name);
            assert_non_empty("event.result", result);
            if *is_error && is_upstream_policy_error_text(result) {
                *saw_upstream_policy_error = true;
            }
        }
        ProtocolEvent::ToolApproval {
            call_id: _,
            name: _,
            arguments: _,
        } => {
            panic!(
                "unexpected tool_approval event: ToolApproval is not implemented for this flow"
            );
        }
    }
}

/// Sends a Run request then immediately drops the connection so the server hits
/// send failure when trying to stream the first event. Covers handle_run_stream send_err path.
#[tokio::test]
async fn e2e_run_then_disconnect() {
    common::load_dotenv();
    let (url, server_handle) = common::spawn_server_once().await;

    let (ws, _) = connect_async(&url).await.unwrap();
    let (mut write, read) = ws.split();

    let req = ClientRequest::Run(RunRequest {
        id: None,
        message: "hi".to_string(),
        agent: AgentType::React,
        thread_id: None,
        workspace_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
    });
    let req_json = serde_json::to_string(&req).unwrap();
    write.send(Message::Text(req_json)).await.unwrap();
    drop(write);
    drop(read);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = timeout(Duration::from_secs(5), server_handle).await;
}

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

    let req = ClientRequest::Run(RunRequest {
        id: None,
        message: "Search the web for recent Rust programming language news and summarize one or two items in a short reply.".to_string(),
        agent: AgentType::React,
        thread_id: None,
        workspace_id: None,
        working_folder: None,
        got_adaptive: None,
        verbose: Some(false),
    });
    let read_timeout = Duration::from_secs(120);
    let req_json = serde_json::to_string(&req).unwrap();
    write.send(Message::Text(req_json)).await.unwrap();

    let mut run_id: Option<String> = None;
    let mut run_event_count = 0usize;
    let mut last_event_id: Option<u64> = None;
    let mut saw_node_enter = false;
    let mut saw_node_exit = false;
    let mut saw_upstream_policy_error = false;

    let (resp, received) = loop {
        let opt = timeout(read_timeout, read.next())
            .await
            .expect("timeout waiting for run response");
        let msg = opt.expect("run response stream ended").unwrap();
        if !msg.is_text() {
            continue;
        }

        let text = msg.to_text().unwrap();
        let received = text.to_string();
        eprintln!("[e2e] received: {}", received);

        let resp: ServerResponse = serde_json::from_str(text).unwrap();
        match resp {
            ServerResponse::RunStreamEvent(ev) => {
                if run_id.is_none() {
                    run_id = Some(ev.id.clone());
                }
                if run_id.as_deref() == Some(ev.id.as_str()) {
                    assert_eq!(ev.id, run_id.as_deref().unwrap(), "stream event run id mismatch");

                    assert_eq!(
                        ev.event.session_id.as_deref(),
                        run_id.as_deref(),
                        "stream event session_id should match run id"
                    );
                    let node_id = ev
                        .event
                        .node_id
                        .as_deref()
                        .expect("stream event should include node_id");
                    assert_non_empty("event.node_id", node_id);

                    let event_id = ev
                        .event
                        .event_id
                        .expect("stream event should include event_id");
                    match last_event_id {
                        Some(prev) => assert_eq!(
                            event_id,
                            prev + 1,
                            "event_id should increase by 1, prev={}, current={}",
                            prev,
                            event_id
                        ),
                        None => assert_eq!(event_id, 1, "first stream event_id should be 1"),
                    }
                    last_event_id = Some(event_id);
                    run_event_count += 1;

                    assert_protocol_event(
                        &ev.event.event,
                        &mut saw_node_enter,
                        &mut saw_node_exit,
                        &mut saw_upstream_policy_error,
                    );
                }
            }
            ServerResponse::RunEnd(r) => {
                if run_id.is_none() {
                    run_id = Some(r.id.clone());
                }
                if run_id.as_deref() == Some(r.id.as_str()) {
                    break (ServerResponse::RunEnd(r), received);
                }
            }
            ServerResponse::Error(e) => {
                if let Some(err_id) = e.id.as_deref() {
                    if run_id.is_none() {
                        run_id = Some(err_id.to_string());
                    }
                    if run_id.as_deref() != Some(err_id) {
                        continue;
                    }
                }
                break (ServerResponse::Error(e), received);
            }
            _ => continue,
        }
    };

    eprintln!("e2e_run_react received:\n{}", received);
    let run_id = run_id.expect("run_id should be set once run response is received");
    let last_event_id = last_event_id.unwrap_or(0);

    match &resp {
        ServerResponse::RunEnd(r) => {
            assert!(
                run_event_count > 0,
                "expected at least one run_stream_event before run_end"
            );
            assert!(saw_node_enter, "expected at least one node_enter event");
            assert!(saw_node_exit, "expected at least one node_exit event");
            assert!(
                r.id.starts_with("run-"),
                "expected server-generated run id, got {:?}",
                r.id
            );
            assert_eq!(
                r.id, run_id,
                "run_end id should match stream event run id"
            );
            if let Some(session_id) = r.session_id.as_deref() {
                assert_eq!(
                    session_id, run_id,
                    "run_end session_id should match stream event run id"
                );
            }
            if let Some(event_id) = r.event_id {
                assert_eq!(
                    event_id,
                    last_event_id + 1,
                    "run_end event_id should be next after last stream event id, last={}, got={}",
                    last_event_id,
                    event_id
                );
            }
            if r.reply.is_empty() && saw_upstream_policy_error {
                eprintln!(
                    "skipping e2e_run_react due to upstream/provider policy error observed in stream events"
                );
                return;
            }
            assert!(
                !r.reply.is_empty(),
                "expected non-empty reply, got {:?}",
                r.reply
            );
            assert!(
                r.reply.to_lowercase().contains("rust"),
                "expected reply to mention Rust (from web search), got {:?}",
                r.reply
            );
        }
        ServerResponse::Error(e) => {
            let msg = format!("{} {}", e.error, received).to_lowercase();
            if msg.contains("403")
                || msg.contains("forbidden")
                || msg.contains("temporarily blocked")
                || msg.contains("content policy")
                || msg.contains("stream ended without final state")
            {
                eprintln!(
                    "skipping e2e_run_react due to upstream/provider policy error: {}",
                    e.error
                );
                return;
            }
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
