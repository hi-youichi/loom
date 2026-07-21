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
use crate::state::{emit, new_part_id, SharedState};

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
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            // LLM decided to invoke a tool. Materialise a pending tool part
            // carrying the tool name + arguments as `input`. Subsequent
            // ToolStart / ToolOutput / ToolEnd for the same `call_id`
            // coalesce onto this part.
            create_or_update_tool_part(
                state,
                assistant_msg_id,
                session_id,
                call_id.as_deref(),
                name,
                ToolTransition::Create {
                    input: arguments.clone(),
                },
            );
        }
        StreamEvent::ToolStart { call_id, name } => {
            // Tool execution started (Act node entered). Transition
            // pending → running.
            create_or_update_tool_part(
                state,
                assistant_msg_id,
                session_id,
                call_id.as_deref(),
                name,
                ToolTransition::Start,
            );
        }
        StreamEvent::ToolOutput {
            call_id,
            name,
            content,
        } => {
            // Tool incremental output during execution. Many events per
            // tool call — accumulate onto `state.output`.
            create_or_update_tool_part(
                state,
                assistant_msg_id,
                session_id,
                call_id.as_deref(),
                name,
                ToolTransition::AppendOutput(content.clone()),
            );
        }
        StreamEvent::ToolEnd {
            call_id,
            name,
            result,
            is_error,
            raw_result,
        } => {
            // Final overwrite. `raw_result` (when present) is the
            // un-normalised output that the TUI should render;
            // `result` is a head/tail excerpt used only as a fallback.
            create_or_update_tool_part(
                state,
                assistant_msg_id,
                session_id,
                call_id.as_deref(),
                name,
                ToolTransition::Finish {
                    output: raw_result.clone().unwrap_or_else(|| result.clone()),
                    is_error: *is_error,
                },
            );
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

/// State machine for a tool part over its lifecycle. Each `Tool*` event
/// transitions the part into a new shape.
#[derive(Debug)]
enum ToolTransition {
    /// First event for this tool call: create the part with `input`.
    Create { input: serde_json::Value },
    /// Execution actually started. Transitions pending → running.
    Start,
    /// Append one chunk of output. Many events per tool call.
    AppendOutput(String),
    /// Final state. `output` overwrites whatever was accumulated.
    Finish { output: String, is_error: bool },
}

/// Materialise (or update in place) a tool part for the given call.
///
/// Part id strategy:
/// - Prefer the explicit `call_id` (stable across the full lifecycle).
/// - Fall back to `tool-{name}-{now_ms}` so concurrent tools of the same
///   name without a `call_id` still get unique parts.
fn create_or_update_tool_part(
    state: &SharedState,
    assistant_msg_id: &str,
    session_id: &str,
    call_id: Option<&str>,
    tool_name: &str,
    transition: ToolTransition,
) {
    let part_id = match call_id {
        Some(id) => format!("tool-{id}"),
        None => format!("tool-{tool_name}-{}", chrono::Utc::now().timestamp_millis()),
    };

    // Try to update an existing part first. Hot path for ToolOutput.
    let updated = {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(assistant_msg_id) {
            if let Some(p) = list.iter_mut().find(|p| p.id == part_id) {
                apply_transition(&mut p.data, &transition);
                let payload = p.data.clone();
                drop(parts);
                Some(payload)
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(payload) = updated {
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

    // No existing part. Materialise one — orphan events still get
    // surfaced so the output isn't silently swallowed.
    let now = chrono::Utc::now().timestamp_millis();
    let mut data = json!({
        "id": part_id,
        "type": "tool",
        "callID": call_id.unwrap_or(part_id.as_str()),
        "tool": tool_name,
        "time": { "start": now },
    });
    if matches!(transition, ToolTransition::Create { .. }) {
        apply_transition(&mut data, &transition);
    } else {
        data["state"] = json!({
            "status": "pending",
            "input": {},
            "output": "",
            "metadata": {},
            "time": { "start": now },
        });
        apply_transition(&mut data, &transition);
    }
    push_part(state, assistant_msg_id, session_id, "tool", data);
}

/// Mutate a tool part's `state` (and `part.time`) per the transition.
fn apply_transition(data: &mut serde_json::Value, transition: &ToolTransition) {
    match transition {
        ToolTransition::Create { input } => {
            data["state"] = json!({
                "status": "pending",
                "input": input,
                "output": "",
                "title": data.get("tool").cloned().unwrap_or(json!("tool")),
                "metadata": {},
                "time": { "start": chrono::Utc::now().timestamp_millis() },
            });
        }
        ToolTransition::Start => {
            let obj = data["state"].as_object_mut().expect("state object");
            obj.insert("status".into(), json!("running"));
        }
        ToolTransition::AppendOutput(content) => {
            let obj = data["state"].as_object_mut().expect("state object");
            let existing = obj
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            obj.insert("output".into(), json!(format!("{existing}{content}")));
        }
        ToolTransition::Finish { output, is_error } => {
            let end = chrono::Utc::now().timestamp_millis();
            {
                let obj = data["state"].as_object_mut().expect("state object");
                obj.insert(
                    "status".into(),
                    json!(if *is_error { "error" } else { "completed" }),
                );
                obj.insert("output".into(), json!(output));
                if *is_error {
                    obj.insert("error".into(), json!(output));
                }
                if let Some(state_time) = obj.get_mut("time").and_then(|v| v.as_object_mut())
                {
                    state_time.insert("end".into(), json!(end));
                }
            }
            if let Some(time) = data.get_mut("time").and_then(|v| v.as_object_mut()) {
                time.insert("end".into(), json!(end));
            }
        }
    }
}
/// Close any open (streaming/pending) text and reasoning parts on the
/// assistant message. Called at the end of a run to stamp `time.end`
/// and flip status to `completed`, so the TUI's duration renderer
/// (`part.time.end`) no longer sees an undefined field.
///
/// Tool parts are intentionally **not** touched here — they have their
/// own lifecycle (`ToolCall` → `ToolStart` → `ToolOutput*` → `ToolEnd`)
/// and transition status via `create_or_update_tool_part`. A run that
/// ends without `ToolEnd` (e.g. crash, cancellation) leaves the tool
/// part in its current state for the user to inspect.
pub fn close_open_text_parts(
    state: &SharedState,
    session_id: &str,
    assistant_msg_id: &str,
    ended_at_ms: i64,
) {
    let updated_parts: Vec<serde_json::Value> = {
        let mut parts = state.parts.write();
        let Some(list) = parts.get_mut(assistant_msg_id) else {
            return;
        };
        let mut updated = Vec::new();
        for p in list.iter_mut() {
            // Only touch streaming text/reasoning parts — tools keep
            // their own state machine (`ToolEnd` already stamps `time.end`
            // via `apply_transition`).
            //
            // NOTE: text/reasoning parts created by `translate_chunk` carry
            // top-level `time` only (no `state.status`). We deliberately do
            // NOT require a `state` object here so the existing logic that
            // was previously only reachable for tool parts is exercised
            // for text/reasoning too. The previous `continue` early-out
            // (when `state` was absent) meant `time.end` was never stamped
            // for streaming text/reasoning and the TUI's duration counter
            // saw `undefined` — see `routes/session/index.tsx:1585,1588`.
            if p.part_type != "text" && p.part_type != "reasoning" {
                continue;
            }
            // Stamp v1 `time.end` and v2 `time.completed` on the top-level
            // `time` field. Both schemas leave them optional; consumers on
            // either version compute duration from these.
            if let Some(part_time) = p.data.get_mut("time").and_then(|v| v.as_object_mut()) {
                if !part_time.contains_key("end") {
                    part_time.insert("end".into(), json!(ended_at_ms));
                }
                if !part_time.contains_key("completed") {
                    part_time.insert("completed".into(), json!(ended_at_ms));
                }
            } else {
                // No `time` object at all — synthesize one so consumers
                // never see `props.part.time` undefined.
                p.data["time"] = json!({
                    "start": ended_at_ms,
                    "end": ended_at_ms,
                    "created": ended_at_ms,
                    "completed": ended_at_ms,
                });
            }
            updated.push(p.data.clone());
        }
        updated
    };
    // Emit one message.part.updated per changed part so the TUI's
    // reactive store re-renders them with the new time.end.
    for payload in updated_parts {
        emit(
            state,
            "message.part.updated",
            json!({
                "sessionID": session_id,
                "part": payload,
                "time": ended_at_ms,
            }),
        );
    }
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

    // Append-in-place: find existing streaming text/reasoning part in this
    // message by `part_type` (not by id) and append. Using `part_type` as the
    // match key lets us mint a fresh `prt_<uuid>` id on first creation
    // (satisfies opencode v1 schema `Schema.isStartsWith("prt")`) without
    // fragmenting the same logical part across multiple id's on subsequent
    // chunks — the TUI state machine would otherwise treat each as a new
    // part and discard the previous text.
    {
        let mut parts = state.parts.write();
        if let Some(list) = parts.get_mut(assistant_msg_id) {
            for p in list.iter_mut() {
                if p.part_type == part_type {
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
//
// Top-level `time` is required: the opencode TUI reads
// `props.part.time.end` for reasoning/text `isDone` / `duration` memos
// in `packages/tui/src/routes/session/index.tsx:1585,1588` and crashes
// with `undefined is not an object (evaluating 'props.part.time.end')`
// if the field is absent. `close_open_text_parts` only stamps `time.end`
// at run completion; the `start` timestamp must exist from the moment
// the part is created so the TUI's duration counter is well-defined.
//
// Both v1 (`start` / `end`) and v2 (`created` / `completed`) time field
// names are emitted with the same timestamp so consumers on either schema
// version can compute duration without conditional handling.
    let now_ms = chrono::Utc::now().timestamp_millis();
    push_part(
        state,
        assistant_msg_id,
        session_id,
        part_type,
        json!({
            "id": new_part_id(),
            "type": part_type,
            "text": chunk.content,
            "time": {
                "start": now_ms,
                "created": now_ms,
            },
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
    use super::{close_open_text_parts, translate_chunk, translate_stream_event};
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
        // IDs are `prt_<uuid>` (opencode v1 schema `Schema.isStartsWith("prt")`),
        // minted fresh on first chunk creation. Verify the prefix only.
        assert!(parts[0].id.starts_with("prt_"), "text part id must satisfy opencode v1 schema `prt_` prefix (got {})", parts[0].id);
        assert!(parts[1].id.starts_with("prt_"), "reasoning part id must satisfy opencode v1 schema `prt_` prefix (got {})", parts[1].id);
        assert_eq!(parts[0].part_type, "text");
        assert_eq!(parts[0].data["text"], "hello world");
        assert_eq!(parts[1].part_type, "reasoning");
        assert_eq!(parts[1].data["text"], "plan");
        // P0 #3: dual time fields — both v1 `start` and v2 `created` must be
        // stamped from creation so consumers on either schema version compute
        // duration from t=0.
        assert!(parts[0].data["time"]["start"].as_i64().is_some());
        assert!(parts[0].data["time"]["created"].as_i64().is_some());
        assert_eq!(
            parts[0].data["time"]["start"], parts[0].data["time"]["created"],
            "v1 start and v2 created must share the same millisecond stamp"
        );
        assert_eq!(
            parts[1].data["time"]["start"], parts[1].data["time"]["created"],
            "v1 start and v2 created must share the same millisecond stamp"
        );
    }

    /// P0 #3: at run close, `close_open_text_parts` mirrors `time.end` to
    /// `time.completed` so v2 consumers can read either field.
    #[test]
    fn close_stamps_both_time_end_and_time_completed() {
        let state = new_state();
        translate_chunk(&MessageChunk::message("hello"), "sess", "msg", &state);
        translate_chunk(&MessageChunk::thinking("plan"), "sess", "msg", &state);
        close_open_text_parts(&state, "sess", "msg", 9876543210);
        let parts = state.parts.read();
        for p in parts.get("msg").unwrap() {
            assert_eq!(p.data["time"]["end"], 9876543210i64);
            assert_eq!(p.data["time"]["completed"], 9876543210i64);
        }
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
                "ToolCall",
                StreamEvent::ToolCall {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                    arguments: json!({"cmd": "ls"}),
                },
                vec!["message.part.updated"],
            ),
            (
                "ToolStart",
                StreamEvent::ToolStart {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                },
                vec!["message.part.updated"],
            ),
            (
                "ToolOutput",
                StreamEvent::ToolOutput {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                    content: "first chunk\n".into(),
                },
                vec!["message.part.updated"],
            ),
            (
                "ToolEnd(ok)",
                StreamEvent::ToolEnd {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                    result: "first chunk\n".into(),
                    is_error: false,
                    raw_result: Some("first chunk\n".into()),
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
                "TaskStart",
                StreamEvent::TaskStart {
                    node_id: "n1".to_string(),
                    namespace: None,
                },
            ),
            (
                "TaskEnd(ok)",
                StreamEvent::TaskEnd {
                    node_id: "n1".to_string(),
                    result: Ok(()),
                    namespace: None,
                },
            ),
            (
                "TaskEnd(err)",
                StreamEvent::TaskEnd {
                    node_id: "n1".to_string(),
                    result: Err("boom".to_string()),
                    namespace: None,
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

    /// Regression: the opencode TUI reads `props.part.time.end` for every
    /// part type (see `routes/session/index.tsx:1585,1588`) and crashes
    /// with `undefined is not an object (evaluating 'props.part.time.end')`
    /// if the field is absent. Lock the contract: text/reasoning parts
    /// MUST carry `time.start` from creation.
    #[test]
    fn messages_chunks_stamp_top_level_time_start_on_creation() {
        let state = new_state();
        translate_chunk(&MessageChunk::message("hello"), "sess", "msg", &state);
        translate_chunk(&MessageChunk::thinking("plan"), "sess", "msg", &state);

        let parts = state.parts.read();
        let list = parts.get("msg").expect("translated parts");
        for part in list {
            assert!(
                part.data.get("time").is_some(),
                "{} part must carry top-level `time` for TUI duration rendering (got {})",
                part.part_type,
                part.data,
            );
            assert!(
                part.data["time"]["start"].as_i64().is_some(),
                "{} part must carry `time.start` (got {})",
                part.part_type,
                part.data["time"],
            );
        }
    }

    /// Regression: subsequent chunks on an already-existing part must NOT
    /// clobber the original `time.start` (or strip `time` altogether).
    /// The first create path stamps `start`; the append-in-place path
    /// (`translate_chunk`) must preserve it so the TUI's duration
    /// counter stays consistent across the run.
    #[test]
    fn appending_to_text_part_preserves_top_level_time_start() {
        let state = new_state();
        translate_chunk(&MessageChunk::message("hello "), "sess", "msg", &state);
        let start_first = state.parts.read().get("msg").unwrap()[0].data["time"]["start"]
            .as_i64()
            .expect("first chunk must stamp time.start");
        translate_chunk(&MessageChunk::message("world"), "sess", "msg", &state);
        let start_second = state.parts.read().get("msg").unwrap()[0].data["time"]["start"]
            .as_i64()
            .expect("append must preserve time.start");
        assert_eq!(
            start_first, start_second,
            "appending chunks must not overwrite time.start"
        );
    }

    #[test]
    fn tool_call_to_end_coalesces_into_one_part_with_input_and_output() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ToolCall {
                call_id: Some("c-read".into()),
                name: "read_file".into(),
                arguments: json!({"path": "/tmp/x"}),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ToolOutput {
                call_id: Some("c-read".into()),
                name: "read_file".into(),
                content: "the file".into(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ToolEnd {
                call_id: Some("c-read".into()),
                name: "read_file".into(),
                result: "head/tail".into(),
                is_error: false,
                raw_result: Some("the file content".into()),
            },
            "sess",
            "msg",
            &state,
        );

        let parts = state.parts.read();
        let list = parts.get("msg").expect("parts exist");
        assert_eq!(list.len(), 1, "tool lifecycle must coalesce onto one part");
        let part = &list[0];
        assert_eq!(part.id, "tool-c-read");
        assert_eq!(part.data["state"]["status"], "completed");
        assert_eq!(part.data["state"]["input"]["path"], "/tmp/x");
        assert_eq!(
            part.data["state"]["output"], "the file content",
            "ToolEnd prefers raw_result over result (head/tail excerpt)"
        );
        assert!(part.data["time"]["end"].as_i64().is_some());
    }

    #[test]
    fn tool_call_input_is_stored_verbatim_for_tui_argument_rendering() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ToolCall {
                call_id: Some("c-bash".into()),
                name: "bash".into(),
                arguments: json!({"command": "ls -la", "timeout": 5000}),
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let part = parts
            .get("msg")
            .and_then(|l| l.first())
            .expect("tool part");
        assert_eq!(part.data["tool"], "bash");
        assert_eq!(part.data["state"]["input"]["command"], "ls -la");
        assert_eq!(part.data["state"]["input"]["timeout"], 5000);
    }

    #[test]
    fn task_start_and_task_end_are_intentionally_dropped() {
        // The agent layer (agent::map_stream_event) already filters these
        // out for its own consumers because `node_id` like "think"/"observe"
        // is graph-internal orchestration, not a real tool. The translator
        // mirrors that filtering here so the chat panel doesn't render empty
        // `tool-{node_id}` blocks with `output: ""`.
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TaskStart {
                node_id: "think".into(),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TaskEnd {
                node_id: "think".into(),
                result: Ok(()),
                namespace: None,
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let list = parts.get("msg");
        assert!(
            list.map(|l| l.is_empty()).unwrap_or(true),
            "TaskStart/TaskEnd must not create tool parts"
        );
        let events = snapshot_replay(&state, None);
        assert!(
            events.is_empty(),
            "TaskStart/TaskEnd must not emit any SSE events, got {:?}",
            events.iter().map(|e| &e.payload.event_type).collect::<Vec<_>>()
        );
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

    }
