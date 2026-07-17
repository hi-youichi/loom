//! Stream-event translator (task P0.5, LS-008).
//!
//! Translates loom [`TypedAnyStreamEvent`]s into opencode v1+v2 SSE events
//! consumed by the chat panel.  Only the **React** typed variant is handled;
//! Dup, Tot, and Got variants are silently dropped because the chat panel
//! has no rendering path for them yet.
//!
//! ## TypedAnyStreamEvent coverage
//!
//! | typed variant                       | handled? | reason                                                       |
//! |-------------------------------------|----------|--------------------------------------------------------------|
//! | `React(StreamEvent<ReActState>)`    | **yes**  | Primary chat / agent loop.                                   |
//! | `Dup(StreamEvent<DupState>)`        | ignored  | Dup (debate) mode has no chat-panel rendering path yet.      |
//! | `Tot(StreamEvent<TotState>)`        | ignored  | ToT (Tree of Thoughts) node-tree view not implemented in TUI.|
//! | `Got(StreamEvent<GotState>)`        | ignored  | GoT (Graph of Thoughts) DAG view not implemented in TUI.     |
//!
//! ## StreamEvent<S> mapping (React inner events)
//!
//! | loom variant                                   | opencode event         | detail                                             |
//! |------------------------------------------------|------------------------|----------------------------------------------------|
//! | `Messages { kind: Message }`                   | `message.part.updated` | Cumulative **text** part (`text-0`).               |
//! | `Messages { kind: Thinking }`                  | `message.part.updated` | Cumulative **reasoning** part (`reasoning-0`).     |
//! | `TaskStart { node_id }`                        | `message.part.updated` | **tool** part, `state.status = pending` (spinner). |
//! | `TaskEnd { node_id, result: Ok }`              | `message.part.updated` | **tool** part, `state.status = completed`.         |
//! | `TaskEnd { node_id, result: Err(msg) }`        | `message.part.updated` | **tool** part, `state.status = error`.             |
//! | `Usage { prompt_tokens, completion_tokens }`   | `message.tokens`       | Token usage `{ input, output }`.                   |
//! | `Values(S)`                                    | *ignored*              | Full graph-state snapshot — internal, not chat-visible.   |
//! | `Updates { node_id, state }`                   | *ignored*              | Incremental graph state — internal, not chat-visible.     |
//! | `Custom(Value)`                                | *ignored*              | Arbitrary JSON payload — no defined OpenCode mapping.     |
//! | `Checkpoint(CheckpointEvent<S>)`               | *ignored*              | Checkpoint persistence is internal; no revert UI yet.     |
//! | `TotExpand { .. }`                             | *ignored*              | ToT node-tree not rendered in chat.                       |
//! | `TotEvaluate { .. }`                           | *ignored*              | ToT node-tree not rendered in chat.                       |
//! | `TotBacktrack { .. }`                          | *ignored*              | ToT node-tree not rendered in chat.                       |
//! | `GotPlan { .. }`                               | *ignored*              | GoT DAG not rendered in chat.                             |
//! | `GotNodeStart { .. }`                          | *ignored*              | GoT DAG not rendered in chat.                             |
//! | `GotNodeComplete { .. }`                       | *ignored*              | GoT DAG not rendered in chat.                             |
//! | `GotNodeFailed { .. }`                         | *ignored*              | GoT DAG not rendered in chat.                             |
//! | `GotExpand { .. }`                             | *ignored*              | GoT DAG not rendered in chat.                             |
//! | `ToolCall { .. }`                              | *ignored*              | Superseded by `TaskStart`/`TaskEnd` for tool lifecycle.   |
//! | `ToolStart { .. }`                              | *ignored*              | Superseded by `TaskStart`/`TaskEnd`.                      |
//! | `ToolOutput { .. }`                             | *ignored*              | Superseded by `TaskStart`/`TaskEnd`.                      |
//! | `ToolEnd { .. }`                               | *ignored*              | Superseded by `TaskStart`/`TaskEnd`.                      |
//!
//! ## Run-lifecycle events (NOT emitted by this translator)
//!
//! `session.status` (`busy`/`idle`) and the final `message.updated` (finish
//! reason) are emitted by the **session handler** (`run_prompt` / `run_shell`),
//! which wraps the entire run — not per-task.  The translator only handles
//! individual stream events inside the run.
//!
//! ## Conventions
//!
//! - One cumulative `message.part.updated` per `(part_type, node_id)`.
//!   Repeated emissions overwrite the same part id so the TUI's reactive
//!   store coalesces in place.
//! - Non-ReAct typed variants and all ToT/GoT/Custom/Checkpoint/Values/
//!   Updates/Tool* events are intentionally ignored (documented above).

use agent::run::{RunCompletion, TypedAnyStreamEvent};
use serde_json::json;
use stream_event::{types::message::MessageChunk, StreamEvent};

use crate::agent_runner::push_part;
use crate::state::{emit, SharedState};

/// Translate a `TypedAnyStreamEvent` into opencode SSE events on `state`.
/// `assistant_msg_id` is the message id the agent loop assigned.
pub fn translate_and_emit(
    ev: &TypedAnyStreamEvent,
    session_id: &str,
    assistant_msg_id: &str,
    state: &SharedState,
) {
    if let TypedAnyStreamEvent::React(stream_ev) = ev {
        translate_stream_event(stream_ev, session_id, assistant_msg_id, state);
    }
}

fn translate_stream_event<S: Clone + Send + Sync + std::fmt::Debug + 'static>(
    ev: &StreamEvent<S>,
    session_id: &str,
    assistant_msg_id: &str,
    state: &SharedState,
) {
    match ev {
        StreamEvent::Messages { chunk, .. } => {
            translate_chunk(chunk, session_id, assistant_msg_id, state);
        }
        StreamEvent::TaskStart { node_id, .. } => {
            // Tool/node start → emit a pending tool part so TUI shows a spinner.
            emit_tool_part(
                state,
                assistant_msg_id,
                session_id,
                node_id,
                json!({ "status": "pending", "input": {} }),
            );
        }
        StreamEvent::TaskEnd {
            node_id, result, ..
        } => {
            let state_payload = if result.is_ok() {
                json!({
                    "status": "completed",
                    "input": {},
                    "output": "",
                    "title": node_id,
                    "metadata": {},
                    "time": {"start": 0, "end": chrono::Utc::now().timestamp_millis()},
                })
            } else {
                json!({
                    "status": "error",
                    "input": {},
                    "error": result.as_ref().err().cloned().unwrap_or_default(),
                    "metadata": {},
                    "time": {"start": 0, "end": chrono::Utc::now().timestamp_millis()},
                })
            };
            emit_tool_part(state, assistant_msg_id, session_id, node_id, state_payload);
        }
        StreamEvent::Usage {
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            tracing::debug!(
                session_id,
                assistant_msg_id,
                prompt_tokens,
                completion_tokens,
                "usage reported"
            );
            emit(
                state,
                "message.tokens",
                json!({
                    "sessionID": session_id,
                    "messageID": assistant_msg_id,
                    "input": prompt_tokens,
                    "output": completion_tokens,
                }),
            );
        }
        // Anything else — checkpoint, custom, ToT, GoT-specific —
        // silently ignored. The chat panel doesn't surface them.
        _ => {}
    }
}

/// Emit (or update in place) a tool part for the given graph node id.
///
/// The part id is a stable `tool-{node_id}` so that a `TaskStart` followed
/// by a `TaskEnd` for the **same** node coalesce onto one part whose `state`
/// transitions pending → completed/error — instead of leaving a stale pending
/// part behind. This mirrors the cumulative-update pattern used by
/// `translate_chunk`.
fn emit_tool_part(
    state: &SharedState,
    assistant_msg_id: &str,
    session_id: &str,
    node_id: &str,
    part_state: serde_json::Value,
) {
    let part_id = format!("tool-{node_id}");

    // Coalesce: transition an existing part with the same stable id.
    {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(assistant_msg_id) {
            for p in list.iter_mut() {
                if p.id == part_id {
                    p.data["state"] = part_state;
                    let payload = p.data.clone();
                    drop(parts);
                    emit(
                        state,
                        "message.part.updated",
                        json!({
                            "sessionID": session_id,
                            "part": payload,
                            "time": chrono::Utc::now().timestamp_millis(),
                        }),
                    );
                    return;
                }
            }
        }
    }

    // No existing part — create a fresh tool part carrying tool name + state.
    push_part(
        state,
        assistant_msg_id,
        session_id,
        "tool",
        json!({
            "id": part_id,
            "type": "tool",
            "callID": node_id,
            "tool": node_id,
            "state": part_state,
        }),
    );
}

fn translate_chunk(
    chunk: &MessageChunk,
    session_id: &str,
    assistant_msg_id: &str,
    state: &SharedState,
) {
    let part_type = if chunk.is_thinking() {
        "reasoning"
    } else {
        "text"
    };
    let part_id = if chunk.is_thinking() {
        "reasoning-0"
    } else {
        "text-0"
    };

    // Append-in-place: find existing text/reasoning part and append.
    {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(assistant_msg_id) {
            for p in list.iter_mut() {
                if p.id == part_id {
                    let existing = p.data["text"].as_str().unwrap_or("").to_string();
                    p.data["text"] = json!(format!("{existing}{}", chunk.content));
                    let payload = p.data.clone();
                    drop(parts);
                    emit(
                        state,
                        "message.part.updated",
                        json!({
                            "sessionID": session_id,
                            "part": payload,
                            "time": chrono::Utc::now().timestamp_millis(),
                        }),
                    );
                    return;
                }
            }
        }
    }
    // Or create a fresh streaming part.
    push_part(
        state,
        assistant_msg_id,
        session_id,
        part_type,
        json!({
            "id": part_id,
            "type": part_type,
            "text": chunk.content,
        }),
    );
}

/// Helper used by tests / agent_runner — emit the final `RunCompletion`
/// envelope as `message.updated` (assistant finish reason).
pub fn emit_run_completion(
    state: &SharedState,
    session_id: &str,
    assistant_msg_id: &str,
    result: &Result<RunCompletion, agent::run::RunError>,
) {
    let finish = match result {
        Ok(RunCompletion::Finished(_)) => "stop",
        Ok(RunCompletion::Cancelled) => "cancelled",
        Err(_) => "error",
    };
    emit(
        state,
        "message.updated",
        json!({
            "sessionID": session_id,
            "info": {
                "id": assistant_msg_id,
                "role": "assistant",
                "finish": finish,
            },
        }),
    );
}

#[allow(dead_code)]
pub(crate) fn last_assistant_reply(_state: &SharedState) -> Option<String> {
    // MVP: nothing to expose — the actual reply comes back through the
    // final `message.updated` SSE event; HTTP callers get the cumulative
    // text from the assistant message's text part via
    // `state.parts[asst_msg_id]["text-0"]`.
    None
}

#[cfg(test)]
mod tests {
    use super::{translate_chunk, translate_stream_event};
    use crate::state::{new_state, snapshot_replay};
    use serde_json::json;
    use stream_event::types::message::MessageChunk;
    use stream_event::{CheckpointEvent, StreamEvent, StreamMetadata};

    /// Minimal state type so we can construct `StreamEvent<S>` in tests.
    #[derive(Clone, Debug)]
    struct TestState;

    fn meta() -> StreamMetadata {
        StreamMetadata {
            loom_node: "think".to_string(),
            namespace: None,
        }
    }

    /// Translate a single event against a fresh state and return the
    /// ordered list of event-type names emitted to the replay buffer.
    fn translate_and_collect_types(event: &StreamEvent<TestState>) -> Vec<String> {
        let state = new_state();
        translate_stream_event(event, "sess", "msg", &state);
        snapshot_replay(&state, None)
            .into_iter()
            .map(|ev| ev.payload.event_type)
            .collect()
    }

    // ─────────────── existing test (preserved) ───────────────

    #[test]
    fn message_chunks_are_cumulative_and_reasoning_is_separate() {
        let state = new_state();
        translate_chunk(&MessageChunk::message("hello "), "sess", "msg", &state);
        translate_chunk(&MessageChunk::message("world"), "sess", "msg", &state);
        translate_chunk(&MessageChunk::thinking("plan"), "sess", "msg", &state);

        let parts = state.parts.read();
        let parts = parts.get("msg").expect("translated parts");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].id, "text-0");
        assert_eq!(parts[0].data["text"], "hello world");
        assert_eq!(parts[1].id, "reasoning-0");
        assert_eq!(parts[1].data["text"], "plan");
    }

    // ─────────────── table-driven: handled events ───────────────
    //
    // Each row verifies that a handled `StreamEvent` variant emits the
    // expected opencode event type(s) and nothing else.

    #[test]
    fn handled_events_emit_expected_opencode_events() {
        let cases: Vec<(&str, StreamEvent<TestState>, Vec<&str>)> = vec![
            (
                "Messages(message)",
                StreamEvent::Messages {
                    chunk: MessageChunk::message("hello"),
                    metadata: meta(),
                },
                vec!["message.part.updated"],
            ),
            (
                "Messages(thinking)",
                StreamEvent::Messages {
                    chunk: MessageChunk::thinking("plan"),
                    metadata: meta(),
                },
                vec!["message.part.updated"],
            ),
            (
                "TaskStart",
                StreamEvent::TaskStart {
                    node_id: "n1".to_string(),
                    namespace: None,
                },
                vec!["message.part.updated"],
            ),
            (
                "TaskEnd(ok)",
                StreamEvent::TaskEnd {
                    node_id: "n1".to_string(),
                    result: Ok(()),
                    namespace: None,
                },
                vec!["message.part.updated"],
            ),
            (
                "TaskEnd(err)",
                StreamEvent::TaskEnd {
                    node_id: "n1".to_string(),
                    result: Err("boom".to_string()),
                    namespace: None,
                },
                vec!["message.part.updated"],
            ),
            (
                "Usage",
                StreamEvent::Usage {
                    prompt_tokens: 100,
                    completion_tokens: 200,
                    total_tokens: 300,
                    cached_tokens: Some(50),
                    prefill_duration: None,
                    decode_duration: None,
                },
                vec!["message.tokens"],
            ),
        ];

        for (name, event, expected) in &cases {
            let got = translate_and_collect_types(event);
            assert_eq!(got, *expected, "event-type mismatch for case '{name}'");
        }
    }

    // ─────────────── table-driven: intentionally ignored events ───────────────
    //
    // None of these variants should produce any SSE event output.

    #[test]
    fn ignored_events_produce_no_output() {
        let cases: Vec<(&str, StreamEvent<TestState>)> = vec![
            ("Values", StreamEvent::Values(TestState)),
            (
                "Updates",
                StreamEvent::Updates {
                    node_id: "n1".to_string(),
                    state: TestState,
                    namespace: None,
                },
            ),
            ("Custom", StreamEvent::Custom(json!({}))),
            (
                "Checkpoint",
                StreamEvent::Checkpoint(CheckpointEvent {
                    checkpoint_id: "cp1".to_string(),
                    timestamp: "2025-01-01".to_string(),
                    step: 0,
                    state: TestState,
                    thread_id: None,
                    checkpoint_ns: None,
                }),
            ),
            (
                "TotExpand",
                StreamEvent::TotExpand {
                    candidates: vec!["a".to_string()],
                },
            ),
            (
                "TotEvaluate",
                StreamEvent::TotEvaluate {
                    chosen: 0,
                    scores: vec![0.5],
                },
            ),
            (
                "TotBacktrack",
                StreamEvent::TotBacktrack {
                    reason: "bad".to_string(),
                    to_depth: 1,
                },
            ),
            (
                "GotPlan",
                StreamEvent::GotPlan {
                    node_count: 1,
                    edge_count: 0,
                    node_ids: vec![],
                },
            ),
            (
                "GotNodeStart",
                StreamEvent::GotNodeStart {
                    node_id: "n1".to_string(),
                },
            ),
            (
                "GotNodeComplete",
                StreamEvent::GotNodeComplete {
                    node_id: "n1".to_string(),
                    result_summary: "ok".to_string(),
                },
            ),
            (
                "GotNodeFailed",
                StreamEvent::GotNodeFailed {
                    node_id: "n1".to_string(),
                    error: "err".to_string(),
                },
            ),
            (
                "GotExpand",
                StreamEvent::GotExpand {
                    node_id: "n1".to_string(),
                    nodes_added: 1,
                    edges_added: 0,
                },
            ),
            (
                "ToolCall",
                StreamEvent::ToolCall {
                    call_id: None,
                    name: "bash".to_string(),
                    arguments: json!({}),
                },
            ),
            (
                "ToolStart",
                StreamEvent::ToolStart {
                    call_id: None,
                    name: "bash".to_string(),
                },
            ),
            (
                "ToolOutput",
                StreamEvent::ToolOutput {
                    call_id: None,
                    name: "bash".to_string(),
                    content: "out".to_string(),
                },
            ),
            (
                "ToolEnd",
                StreamEvent::ToolEnd {
                    call_id: None,
                    name: "bash".to_string(),
                    result: "done".to_string(),
                    is_error: false,
                    raw_result: None,
                },
            ),
        ];

        for (name, event) in &cases {
            let got = translate_and_collect_types(event);
            assert!(
                got.is_empty(),
                "ignored event '{name}' should produce no output but emitted {got:?}",
            );
        }
    }

    // ─────────────── content-verification tests ───────────────

    #[test]
    fn messages_text_event_creates_text_part_with_content() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::Messages {
                chunk: MessageChunk::message("hello world"),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let part = parts
            .get("msg")
            .and_then(|l| l.first())
            .expect("text part created");
        assert_eq!(part.part_type, "text");
        assert_eq!(part.data["text"], "hello world");
    }

    #[test]
    fn task_start_creates_pending_tool_part() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TaskStart {
                node_id: "my-tool".to_string(),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let part = parts
            .get("msg")
            .and_then(|l| l.first())
            .expect("tool part created");
        assert_eq!(part.part_type, "tool");
        assert_eq!(part.data["state"]["status"], "pending");
        assert_eq!(part.data["tool"], "my-tool");
    }

    #[test]
    fn task_end_ok_creates_completed_tool_part() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TaskEnd {
                node_id: "my-tool".to_string(),
                result: Ok(()),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let part = parts
            .get("msg")
            .and_then(|l| l.first())
            .expect("tool part created");
        assert_eq!(part.data["state"]["status"], "completed");
    }

    #[test]
    fn task_end_err_creates_error_tool_part() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TaskEnd {
                node_id: "my-tool".to_string(),
                result: Err("tool failed".to_string()),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let part = parts
            .get("msg")
            .and_then(|l| l.first())
            .expect("tool part created");
        assert_eq!(part.data["state"]["status"], "error");
        assert_eq!(part.data["state"]["error"], "tool failed");
    }

    #[test]
    fn usage_emits_correct_token_counts() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::Usage {
                prompt_tokens: 150,
                completion_tokens: 250,
                total_tokens: 400,
                cached_tokens: Some(10),
                prefill_duration: None,
                decode_duration: None,
            },
            "sess",
            "msg",
            &state,
        );
        let events = snapshot_replay(&state, None);
        let tokens_ev = events
            .iter()
            .find(|ev| ev.payload.event_type == "message.tokens")
            .expect("message.tokens event");
        let props = &tokens_ev.payload.properties;
        assert_eq!(props["input"], 150);
        assert_eq!(props["output"], 250);
        assert_eq!(props["sessionID"], "sess");
        assert_eq!(props["messageID"], "msg");
    }

    // ─────────────── tool-part lifecycle (LS-009) ───────────────
    //
    // TaskStart → TaskEnd for the same node must coalesce onto a single
    // stable tool part whose state transitions pending → completed/error.

    #[test]
    fn task_lifecycle_start_then_end_coalesces_into_one_part() {
        let state = new_state();
        // Start: pending.
        translate_stream_event(
            &StreamEvent::<TestState>::TaskStart {
                node_id: "read_file".to_string(),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        // End: completed — same node id.
        translate_stream_event(
            &StreamEvent::<TestState>::TaskEnd {
                node_id: "read_file".to_string(),
                result: Ok(()),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let list = parts.get("msg").expect("parts exist");
        assert_eq!(
            list.len(),
            1,
            "start+end must coalesce into a single tool part"
        );
        let part = &list[0];
        assert_eq!(part.id, "tool-read_file", "stable id derived from node id");
        assert_eq!(part.part_type, "tool");
        assert_eq!(part.data["state"]["status"], "completed");
        assert_eq!(part.data["tool"], "read_file");
    }

    #[test]
    fn task_lifecycle_start_then_error_coalesces_into_one_part() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TaskStart {
                node_id: "write_file".to_string(),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TaskEnd {
                node_id: "write_file".to_string(),
                result: Err("disk full".to_string()),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let list = parts.get("msg").expect("parts exist");
        assert_eq!(list.len(), 1, "start+error must coalesce into one part");
        let part = &list[0];
        assert_eq!(part.id, "tool-write_file");
        assert_eq!(part.data["state"]["status"], "error");
        assert_eq!(part.data["state"]["error"], "disk full");
    }

    #[test]
    fn distinct_node_ids_produce_distinct_stable_tool_parts() {
        let state = new_state();
        for node in ["alpha", "beta"] {
            translate_stream_event(
                &StreamEvent::<TestState>::TaskStart {
                    node_id: node.to_string(),
                    namespace: None,
                },
                "sess",
                "msg",
                &state,
            );
        }
        let parts = state.parts.read();
        let list = parts.get("msg").expect("parts exist");
        assert_eq!(list.len(), 2, "two distinct tools → two distinct parts");
        assert_eq!(list[0].id, "tool-alpha");
        assert_eq!(list[1].id, "tool-beta");
    }
}
