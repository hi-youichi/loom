//! Stream-event translator (task P0.5, LS-008).
//!
//! Translates loom [`TypedAnyStreamEvent`]s into opencode v1+v2 SSE events
//! consumed by the chat panel. Only the **React** typed variant is handled;
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
//! | loom variant                                   | opencode event(s)          | detail                                                      |
//! |------------------------------------------------|----------------------------|-------------------------------------------------------------|
//! | `TextBlockStart`                               | `message.part.updated`     | Open a new text part; track it as `active_text`.            |
//! | `TextDelta`                                    | `message.part.updated`     | Append to `active_text[msg_id]`.                            |
//! | `TextBlockEnd`                                 | `message.part.updated`     | Finalize text part (stamp `time.end`/`time.completed`).     |
//! | `ReasoningBlockStart { id }`                   | `message.part.updated`     | Open a reasoning part; track under `id`.                    |
//! | `ReasoningDelta { id }`                        | `message.part.updated`     | Append to `active_reasoning[msg_id][id]`.                   |
//! | `ReasoningBlockEnd { id }`                     | `message.part.updated`     | Finalize reasoning part under `id`.                         |
//! | `TurnStart`                                    | `message.part.updated`     | `step-start` marker part.                                   |
//! | `TurnFinish { reason, usage }`                 | `message.part.updated` +   | `step-finish` marker part, finalize text/reasoning parts,   |
//! |                                                | `message.tokens`           | emit `message.tokens` for TUI consumption.                  |
//! | `ToolCall`                                     | `message.part.updated`     | Create pending tool part with `input`.                      |
//! | `ToolStart`                                    | `message.part.updated`     | pending → running.                                          |
//! | `ToolOutput`                                   | `message.part.updated`     | Append output chunk.                                        |
//! | `ToolEnd`                                      | `message.part.updated`     | Finalize tool part.                                         |
//! | `ToolError { call_id, error }`                 | `message.part.updated`     | Mark tool part `tool-{call_id}` as error.                   |
//! | `ProviderError { message }`                    | `session.error`            | Surface provider-level failure.                             |
//! | `Finish`                                       | *(none)*                   | Explicit no-op; run finish handled by session handler.      |
//! | `Values(S)` / `Updates{..}` / `Custom(..)` /   |                            |                                                             |
//! | `Checkpoint(..)` / `TaskStart` / `TaskEnd` /   | *ignored*                  | Internal / non-chat events.                                 |
//! | `Tot*` / `Got*`                                |                            |                                                             |
//!
//! ## Run-lifecycle events (NOT emitted by this translator)
//!
//! `session.status` (`busy`/`idle`) and the final `message.updated` (finish
//! reason) are emitted by the **session handler** (`run_prompt` / `run_shell`),
//! which wraps the entire run — not per-task. The translator only handles
//! individual stream events inside the run.
//!
//! ## Conventions
//!
//! - Each `Text*` and `Reasoning*` block opens and closes its own part id; the
//!   `*Delta` events coalesce onto that part via the active map on `AppState`.
//! - `TextDelta` / `ReasoningDelta` arriving without a preceding
//!   `*BlockStart` are silently dropped — the part id would be unknown and
//!   emitting an orphan part would break the TUI's reactive coalescing.

use agent::run::{RunCompletion, TypedAnyStreamEvent};
use serde_json::json;
use std::collections::HashMap;
use stream_event::{StreamEvent, Usage};

use crate::agent_runner::push_part;
use crate::state::{SharedState, emit, new_part_id};

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
        StreamEvent::TextBlockStart { metadata } => {
            let part_id = new_part_id();
            let now = chrono::Utc::now().timestamp_millis();
            push_part(
                state,
                assistant_msg_id,
                session_id,
                "text",
                json!({
                    "id": part_id,
                    "type": "text",
                    "text": "",
                    "time": { "start": now, "created": now },
                    "metadata": metadata,
                }),
            );
            state
                .active_text
                .write()
                .insert(assistant_msg_id.to_string(), part_id);
        }
        StreamEvent::TextDelta { content, .. } => {
            if let Some(part_id) = state.active_text.read().get(assistant_msg_id).cloned() {
                append_to_part(state, session_id, assistant_msg_id, &part_id, content);
            }
        }
        StreamEvent::TextBlockEnd { .. } => {
            finalize_text_part(state, session_id, assistant_msg_id);
        }
        StreamEvent::ReasoningBlockStart { id, metadata } => {
            let part_id = new_part_id();
            let now = chrono::Utc::now().timestamp_millis();
            push_part(
                state,
                assistant_msg_id,
                session_id,
                "reasoning",
                json!({
                    "id": part_id,
                    "type": "reasoning",
                    "text": "",
                    "time": { "start": now, "created": now },
                    "metadata": metadata,
                }),
            );
            state
                .active_reasoning
                .write()
                .entry(assistant_msg_id.to_string())
                .or_default()
                .insert(id.clone(), part_id);
        }
        StreamEvent::ReasoningDelta { id, content, .. } => {
            let part_id = state
                .active_reasoning
                .read()
                .get(assistant_msg_id)
                .and_then(|parts| parts.get(id))
                .cloned();
            if let Some(part_id) = part_id {
                append_to_part(state, session_id, assistant_msg_id, &part_id, content);
            }
        }
        StreamEvent::ReasoningBlockEnd { id, .. } => {
            finalize_reasoning_part(state, session_id, assistant_msg_id, id);
        }
        StreamEvent::TurnStart => {
            let now = chrono::Utc::now().timestamp_millis();
            push_part(
                state,
                assistant_msg_id,
                session_id,
                "step-start",
                json!({
                    "id": new_part_id(),
                    "type": "step-start",
                    "time": { "start": now, "created": now },
                }),
            );
        }
        StreamEvent::TurnFinish { reason, usage } => {
            finalize_text_part(state, session_id, assistant_msg_id);
            finalize_all_reasoning_parts(state, session_id, assistant_msg_id);
            let now = chrono::Utc::now().timestamp_millis();
            push_part(
                state,
                assistant_msg_id,
                session_id,
                "step-finish",
                json!({
                    "id": new_part_id(),
                    "type": "step-finish",
                    "reason": reason,
                    "tokens": {
                        "prompt": usage.prompt_tokens,
                        "completion": usage.completion_tokens,
                        "total": usage.total_tokens,
                        "cached": usage.cached_tokens,
                    },
                    "time": { "start": now, "end": now, "created": now, "completed": now },
                }),
            );
            emit_usage(state, session_id, assistant_msg_id, usage);
        }
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            // LLM decided to invoke a tool. Materialise a pending tool part
            // carrying the tool name + arguments as `input`. Subsequent
            // ToolStart / ToolOutput / ToolEnd for the same `call_id`
            // coalesce onto this part. No finalize is invoked here.
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
        StreamEvent::ToolError { call_id, error } => {
            // Mark the existing `tool-{call_id}` part as errored without
            // requiring the tool name. No-op when call_id is missing or
            // the tool part was never created.
            fail_tool_call(
                state,
                assistant_msg_id,
                session_id,
                call_id.as_deref(),
                error,
            );
        }
        StreamEvent::ProviderError { message } => {
            emit(
                state,
                "session.error",
                json!({
                    "sessionID": session_id,
                    "error": {
                        "name": "ProviderError",
                        "data": { "message": message },
                    },
                }),
            );
        }
        StreamEvent::Finish => {
            // Explicit no-op: the session handler emits `message.updated`
            // with the final finish reason and `session.status: idle`.
        }
        // Anything else — checkpoint, custom, ToT, GoT-specific —
        // silently ignored. The chat panel doesn't surface them.
        _ => {}
    }
}

fn append_to_part(
    state: &SharedState,
    session_id: &str,
    assistant_msg_id: &str,
    part_id: &str,
    content: &str,
) {
    let payload = {
        let mut parts = state.parts.write();
        parts.get_mut(assistant_msg_id).and_then(|list| {
            list.iter_mut().find(|part| part.id == part_id).map(|part| {
                let existing = part.data["text"].as_str().unwrap_or_default();
                part.data["text"] = json!(format!("{existing}{content}"));
                part.data.clone()
            })
        })
    };
    if let Some(payload) = payload {
        emit(
            state,
            "message.part.updated",
            json!({
                "sessionID": session_id,
                "part": payload,
                "time": chrono::Utc::now().timestamp_millis(),
            }),
        );
    }
}

fn emit_usage(state: &SharedState, session_id: &str, assistant_msg_id: &str, usage: &Usage) {
    emit(
        state,
        "message.tokens",
        json!({
            "sessionID": session_id,
            "messageID": assistant_msg_id,
            "input": usage.prompt_tokens,
            "output": usage.completion_tokens,
        }),
    );
}

fn fail_tool_call(
    state: &SharedState,
    assistant_msg_id: &str,
    session_id: &str,
    call_id: Option<&str>,
    error: &str,
) {
    let Some(call_id) = call_id else { return };
    let part_id = format!("tool-{call_id}");
    let updated = {
        let mut parts = state.parts.write();
        parts.get_mut(assistant_msg_id).and_then(|list| {
            list.iter_mut().find(|part| part.id == part_id).map(|part| {
                apply_transition(
                    &mut part.data,
                    &ToolTransition::Finish {
                        output: error.to_string(),
                        is_error: true,
                    },
                );
                part.data.clone()
            })
        })
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
            let raw = serde_json::to_string(input).unwrap_or_default();
            data["state"] = json!({
                "status": "pending",
                "input": input,
                "raw": raw,
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
            let existing = obj.get("output").and_then(|v| v.as_str()).unwrap_or("");
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
                if let Some(state_time) = obj.get_mut("time").and_then(|v| v.as_object_mut()) {
                    state_time.insert("end".into(), json!(end));
                }
            }
            if let Some(time) = data.get_mut("time").and_then(|v| v.as_object_mut()) {
                time.insert("end".into(), json!(end));
            }
            tracing::info!(
                tool = %data.get("tool").and_then(|v| v.as_str()).unwrap_or("?"),
                output_len = output.len(),
                output_preview = %output.chars().take(200).collect::<String>(),
                is_error = is_error,
                "ToolEnd Finish transition"
            );
        }
    }
}

pub fn finalize_text_part(state: &SharedState, session_id: &str, assistant_msg_id: &str) {
    let part_id = state.active_text.write().remove(assistant_msg_id);
    if let Some(part_id) = part_id {
        finalize_part_by_id(state, session_id, assistant_msg_id, &part_id);
    }
}

pub fn finalize_reasoning_part(
    state: &SharedState,
    session_id: &str,
    assistant_msg_id: &str,
    reasoning_id: &str,
) {
    let part_id = {
        let mut active = state.active_reasoning.write();
        let part_id = active
            .get_mut(assistant_msg_id)
            .and_then(|parts| parts.remove(reasoning_id));
        if active.get(assistant_msg_id).is_some_and(HashMap::is_empty) {
            active.remove(assistant_msg_id);
        }
        part_id
    };
    if let Some(part_id) = part_id {
        finalize_part_by_id(state, session_id, assistant_msg_id, &part_id);
    }
}

pub fn finalize_all_reasoning_parts(state: &SharedState, session_id: &str, assistant_msg_id: &str) {
    let part_ids = state
        .active_reasoning
        .write()
        .remove(assistant_msg_id)
        .unwrap_or_default()
        .into_values()
        .collect::<Vec<_>>();
    for part_id in part_ids {
        finalize_part_by_id(state, session_id, assistant_msg_id, &part_id);
    }
}

fn finalize_part_by_id(
    state: &SharedState,
    session_id: &str,
    assistant_msg_id: &str,
    part_id: &str,
) {
    let now = chrono::Utc::now().timestamp_millis();
    let payload = {
        let mut parts = state.parts.write();
        parts.get_mut(assistant_msg_id).and_then(|list| {
            list.iter_mut().find(|part| part.id == part_id).map(|part| {
                if let Some(time) = part
                    .data
                    .get_mut("time")
                    .and_then(|value| value.as_object_mut())
                {
                    time.entry("end").or_insert_with(|| json!(now));
                    time.entry("completed").or_insert_with(|| json!(now));
                } else {
                    part.data["time"] = json!({
                        "start": now,
                        "end": now,
                        "created": now,
                        "completed": now,
                    });
                }
                part.data.clone()
            })
        })
    };
    if let Some(payload) = payload {
        emit(
            state,
            "message.part.updated",
            json!({ "sessionID": session_id, "part": payload, "time": now }),
        );
    }
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
    use super::{
        append_to_part, finalize_all_reasoning_parts, finalize_part_by_id, finalize_reasoning_part,
        finalize_text_part, translate_stream_event,
    };
    use crate::state::{new_state, snapshot_replay};
    use serde_json::json;
    use stream_event::{CheckpointEvent, StreamEvent, StreamMetadata, Usage};

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

    /// Translate `seed` then `event` against a fresh state, returning only
    /// the event types emitted by `event` (seed emissions are discarded).
    fn translate_after_seed(
        seed: &[StreamEvent<TestState>],
        event: &StreamEvent<TestState>,
    ) -> Vec<String> {
        let state = new_state();
        for ev in seed {
            translate_stream_event(ev, "sess", "msg", &state);
        }
        let before = state.event_buffer.read().len();
        translate_stream_event(event, "sess", "msg", &state);
        let buf = state.event_buffer.read();
        buf.iter()
            .skip(before)
            .map(|ev| ev.payload.event_type.clone())
            .collect()
    }

    // ─────────────── text + reasoning block lifecycle ───────────────

    #[test]
    fn text_and_reasoning_blocks_create_separate_parts() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TextBlockStart { metadata: meta() },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TextDelta {
                content: "hello ".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TextDelta {
                content: "world".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TextBlockEnd { metadata: meta() },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r1".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningDelta {
                id: "r1".into(),
                content: "plan".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );

        let parts = state.parts.read();
        let list = parts.get("msg").expect("translated parts");
        assert_eq!(list.len(), 2);
        assert!(
            list[0].id.starts_with("prt_"),
            "text part id must satisfy opencode v1 schema `prt_` prefix (got {})",
            list[0].id
        );
        assert!(
            list[1].id.starts_with("prt_"),
            "reasoning part id must satisfy opencode v1 schema `prt_` prefix (got {})",
            list[1].id
        );
        assert_eq!(list[0].part_type, "text");
        assert_eq!(list[0].data["text"], "hello world");
        assert_eq!(list[1].part_type, "reasoning");
        assert_eq!(list[1].data["text"], "plan");
        assert!(list[0].data["time"]["start"].as_i64().is_some());
        assert!(list[0].data["time"]["created"].as_i64().is_some());
        assert_eq!(
            list[0].data["time"]["start"], list[0].data["time"]["created"],
            "v1 start and v2 created must share the same millisecond stamp"
        );
        assert_eq!(
            list[1].data["time"]["start"], list[1].data["time"]["created"],
            "v1 start and v2 created must share the same millisecond stamp"
        );
    }

    /// Two parallel reasoning blocks (different ids) keep their own parts;
    /// deltas addressed to one block do not bleed into the other.
    #[test]
    fn reasoning_deltas_for_different_blocks_are_separate() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r1".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningDelta {
                id: "r1".into(),
                content: "first ".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r2".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningDelta {
                id: "r2".into(),
                content: "second".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );

        let parts = state.parts.read();
        let list = parts.get("msg").expect("parts");
        let reasoning: Vec<_> = list.iter().filter(|p| p.part_type == "reasoning").collect();
        assert_eq!(reasoning.len(), 2);
        assert_eq!(reasoning[0].data["text"], "first ");
        assert_eq!(reasoning[1].data["text"], "second");
    }

    /// `TextDelta` arriving without a preceding `TextBlockStart` is a
    /// silent no-op — emitting an orphan part would break the TUI's
    /// reactive coalescing.
    #[test]
    fn text_delta_without_active_part_is_noop() {
        let got = translate_and_collect_types(&StreamEvent::<TestState>::TextDelta {
            content: "orphan".into(),
            metadata: meta(),
        });
        assert!(
            got.is_empty(),
            "TextDelta without an active text part must emit nothing, got {got:?}"
        );
    }

    /// `ReasoningDelta` addressed to an unknown id is a silent no-op.
    #[test]
    fn reasoning_delta_for_unknown_block_is_noop() {
        let got = translate_and_collect_types(&StreamEvent::<TestState>::ReasoningDelta {
            id: "unknown".into(),
            content: "orphan".into(),
            metadata: meta(),
        });
        assert!(
            got.is_empty(),
            "ReasoningDelta for unknown id must emit nothing, got {got:?}"
        );
    }

    // ─────────────── finalize helpers ───────────────

    /// `finalize_text_part` stamps both v1 `time.end` and v2 `time.completed`
    /// so consumers on either schema version compute duration.
    #[test]
    fn finalize_text_part_stamps_time_end_and_completed() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TextBlockStart { metadata: meta() },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TextDelta {
                content: "hello".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        finalize_text_part(&state, "sess", "msg");
        let parts = state.parts.read();
        let p = parts.get("msg").and_then(|l| l.first()).unwrap();
        assert!(p.data["time"]["end"].as_i64().is_some());
        assert_eq!(
            p.data["time"]["end"], p.data["time"]["completed"],
            "finalize_text_part must mirror time.end to time.completed"
        );
        assert!(
            state.active_text.read().get("msg").is_none(),
            "finalize_text_part must clear the active_text pointer"
        );
    }

    /// `finalize_reasoning_part` closes only the addressed id; siblings
    /// remain open.
    #[test]
    fn finalize_reasoning_part_only_closes_addressed_id() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r1".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r2".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );

        finalize_reasoning_part(&state, "sess", "msg", "r1");

        let p2_id = {
            let active = state.active_reasoning.read();
            let msg_active = active.get("msg").expect("reasoning map survives");
            assert!(msg_active.get("r1").is_none(), "r1 must be removed from active map");
            assert!(msg_active.get("r2").is_some(), "r2 must remain active");
            msg_active.get("r2").cloned().expect("r2 part id present")
        };

        let parts = state.parts.read();
        let list = parts.get("msg").expect("parts exist");

        let p2 = list.iter().find(|p| p.id == p2_id).expect("r2 part exists");
        assert!(
            p2.data.get("time").and_then(|t| t.get("end")).is_none(),
            "r2 part must NOT have time.end stamped yet"
        );

        let p1 = list
            .iter()
            .find(|p| p.id != p2_id)
            .expect("r1 part still present in parts list");
        assert!(
            p1.data["time"]["end"].as_i64().is_some(),
            "r1 part must have time.end stamped"
        );
    }

    /// `finalize_all_reasoning_parts` closes every active reasoning part
    /// and clears the map for the message.
    #[test]
    fn finalize_all_reasoning_parts_stamps_each_part() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r1".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningDelta {
                id: "r1".into(),
                content: "plan".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r2".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );

        finalize_all_reasoning_parts(&state, "sess", "msg");

        assert!(
            state.active_reasoning.read().get("msg").is_none(),
            "finalize_all_reasoning_parts must drop the per-message map"
        );
        let parts = state.parts.read();
        for p in parts.get("msg").unwrap() {
            assert!(
                p.data["time"]["end"].as_i64().is_some(),
                "{} part must carry time.end after finalize_all_reasoning_parts",
                p.part_type
            );
        }
    }

    /// `finalize_part_by_id` is a no-op when the part isn't on the message.
    #[test]
    fn finalize_part_by_id_for_unknown_part_is_noop() {
        let state = new_state();
        let before = state.event_buffer.read().len();
        finalize_part_by_id(&state, "sess", "msg", "prt_does_not_exist");
        assert_eq!(state.event_buffer.read().len(), before);
    }

    // ─────────────── time.start preservation ───────────────

    /// Regression: the opencode TUI reads `props.part.time.end` for every
    /// part type and crashes if the field is absent. Lock the contract:
    /// text/reasoning parts MUST carry `time.start` from creation.
    #[test]
    fn text_and_reasoning_blocks_stamp_top_level_time_start_on_creation() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TextBlockStart { metadata: meta() },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r1".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );

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

    /// Regression: subsequent deltas on an already-open part must NOT
    /// clobber the original `time.start`. The first create path stamps
    /// `start`; the append path must preserve it so the TUI's duration
    /// counter stays consistent across the run.
    #[test]
    fn appending_text_delta_preserves_top_level_time_start() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TextBlockStart { metadata: meta() },
            "sess",
            "msg",
            &state,
        );
        let start_first = state.parts.read().get("msg").unwrap()[0].data["time"]["start"]
            .as_i64()
            .expect("first event must stamp time.start");
        translate_stream_event(
            &StreamEvent::<TestState>::TextDelta {
                content: "world".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        let start_second = state.parts.read().get("msg").unwrap()[0].data["time"]["start"]
            .as_i64()
            .expect("append must preserve time.start");
        assert_eq!(
            start_first, start_second,
            "appending deltas must not overwrite time.start"
        );
    }

    // ─────────────── tool lifecycle ───────────────

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
        let part = parts.get("msg").and_then(|l| l.first()).expect("tool part");
        assert_eq!(part.data["tool"], "bash");
        assert_eq!(part.data["state"]["input"]["command"], "ls -la");
        assert_eq!(part.data["state"]["input"]["timeout"], 5000);
    }

    #[test]
    fn task_start_and_task_end_are_intentionally_dropped() {
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
            events
                .iter()
                .map(|e| &e.payload.event_type)
                .collect::<Vec<_>>()
        );
    }

    // ─────────────── ToolError ───────────────

    /// `ToolError` marks the existing `tool-{call_id}` part as errored
    /// using only `call_id` — no tool name required.
    #[test]
    fn tool_error_marks_existing_tool_part_as_error_without_tool_name() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ToolCall {
                call_id: Some("c-1".into()),
                name: "bash".into(),
                arguments: json!({"cmd": "ls"}),
            },
            "sess",
            "msg",
            &state,
        );
        let before = state.event_buffer.read().len();
        translate_stream_event(
            &StreamEvent::<TestState>::ToolError {
                call_id: Some("c-1".into()),
                error: "tool blew up".into(),
            },
            "sess",
            "msg",
            &state,
        );

        let parts = state.parts.read();
        let tool = parts
            .get("msg")
            .and_then(|l| l.iter().find(|p| p.id == "tool-c-1"))
            .expect("tool part exists");
        assert_eq!(tool.data["state"]["status"], "error");
        assert_eq!(tool.data["state"]["error"], "tool blew up");
        assert!(tool.data["time"]["end"].as_i64().is_some());

        let buf = state.event_buffer.read();
        let new_events: Vec<_> = buf.iter().skip(before).collect();
        assert_eq!(new_events.len(), 1);
        assert_eq!(new_events[0].payload.event_type, "message.part.updated");
    }

    /// `ToolError` without a `call_id` cannot locate any part and emits
    /// nothing — orphan errors must not pollute the chat panel.
    #[test]
    fn tool_error_without_call_id_is_noop() {
        let got = translate_and_collect_types(&StreamEvent::<TestState>::ToolError {
            call_id: None,
            error: "orphan".into(),
        });
        assert!(
            got.is_empty(),
            "ToolError without call_id must not emit anything, got {got:?}"
        );
    }

    /// `ToolError` for a `call_id` that was never started is a silent no-op.
    #[test]
    fn tool_error_for_unknown_call_id_is_noop() {
        let got = translate_and_collect_types(&StreamEvent::<TestState>::ToolError {
            call_id: Some("nonexistent".into()),
            error: "orphan".into(),
        });
        assert!(
            got.is_empty(),
            "ToolError for unknown call_id must not emit anything, got {got:?}"
        );
    }

    // ─────────────── ProviderError ───────────────

    #[test]
    fn provider_error_emits_session_error() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::ProviderError {
                message: "rate limit".into(),
            },
            "sess",
            "msg",
            &state,
        );
        let events = snapshot_replay(&state, None);
        let err_ev = events
            .iter()
            .find(|ev| ev.payload.event_type == "session.error")
            .expect("session.error event");
        let props = &err_ev.payload.properties;
        assert_eq!(props["sessionID"], "sess");
        assert_eq!(props["error"]["name"], "ProviderError");
        assert_eq!(props["error"]["data"]["message"], "rate limit");
    }

    // ─────────────── Finish ───────────────

    #[test]
    fn finish_is_explicit_noop() {
        let state = new_state();
        translate_stream_event(&StreamEvent::<TestState>::Finish, "sess", "msg", &state);
        let events = snapshot_replay(&state, None);
        assert!(
            events.is_empty(),
            "Finish must not emit any SSE events, got {:?}",
            events
                .iter()
                .map(|e| &e.payload.event_type)
                .collect::<Vec<_>>()
        );
        let parts = state.parts.read();
        assert!(
            parts.get("msg").map(|l| l.is_empty()).unwrap_or(true),
            "Finish must not create any parts"
        );
    }

    // ─────────────── TurnStart / TurnFinish ───────────────

    #[test]
    fn turn_start_emits_step_start_part() {
        let state = new_state();
        translate_stream_event(&StreamEvent::<TestState>::TurnStart, "sess", "msg", &state);
        let parts = state.parts.read();
        let p = parts
            .get("msg")
            .and_then(|l| l.first())
            .expect("step-start part");
        assert_eq!(p.part_type, "step-start");
        assert!(p.data["time"]["start"].as_i64().is_some());
    }

    #[test]
    fn turn_finish_emits_step_finish_part_with_usage() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TurnFinish {
                reason: "stop".into(),
                usage: Usage {
                    prompt_tokens: 11,
                    completion_tokens: 22,
                    total_tokens: 33,
                    cached_tokens: Some(4),
                },
            },
            "sess",
            "msg",
            &state,
        );
        let parts = state.parts.read();
        let p = parts
            .get("msg")
            .and_then(|l| l.iter().find(|p| p.part_type == "step-finish"))
            .expect("step-finish part");
        assert_eq!(p.data["reason"], "stop");
        assert_eq!(p.data["tokens"]["prompt"], 11);
        assert_eq!(p.data["tokens"]["completion"], 22);
        assert_eq!(p.data["tokens"]["total"], 33);
        assert_eq!(p.data["tokens"]["cached"], 4);
    }

    #[test]
    fn turn_finish_emits_message_tokens_with_counts() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TurnFinish {
                reason: "stop".into(),
                usage: Usage {
                    prompt_tokens: 150,
                    completion_tokens: 250,
                    total_tokens: 400,
                    cached_tokens: Some(10),
                },
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

    /// `TurnFinish` finalizes any still-open text and reasoning parts on
    /// the same message so the run doesn't leak streaming parts without
    /// `time.end`.
    #[test]
    fn turn_finish_finalizes_open_text_and_reasoning_parts() {
        let state = new_state();
        translate_stream_event(
            &StreamEvent::<TestState>::TextBlockStart { metadata: meta() },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TextDelta {
                content: "hi".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::ReasoningBlockStart {
                id: "r1".into(),
                metadata: meta(),
            },
            "sess",
            "msg",
            &state,
        );
        translate_stream_event(
            &StreamEvent::<TestState>::TurnFinish {
                reason: "stop".into(),
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cached_tokens: None,
                },
            },
            "sess",
            "msg",
            &state,
        );

        assert!(state.active_text.read().get("msg").is_none());
        assert!(state.active_reasoning.read().get("msg").is_none());

        let parts = state.parts.read();
        for p in parts.get("msg").unwrap() {
            if matches!(p.part_type.as_str(), "text" | "reasoning") {
                assert!(
                    p.data["time"]["end"].as_i64().is_some(),
                    "{} part must be finalized by TurnFinish (time.end missing)",
                    p.part_type
                );
            }
        }
    }

    // ─────────────── table-driven: handled events ───────────────
    //
    // Each row verifies that a handled `StreamEvent` variant emits the
    // expected opencode event type(s) and nothing else. Variants that
    // require a prior block-start seed (e.g. TextDelta/End, ReasoningDelta/
    // End) use `translate_after_seed` so they can be exercised in isolation.

    #[test]
    fn handled_events_emit_expected_opencode_events() {
        let cases: Vec<(&str, Vec<StreamEvent<TestState>>, Vec<&str>)> = vec![
            (
                "TextBlockStart",
                vec![StreamEvent::TextBlockStart { metadata: meta() }],
                vec!["message.part.updated"],
            ),
            (
                "ReasoningBlockStart",
                vec![StreamEvent::ReasoningBlockStart {
                    id: "r".into(),
                    metadata: meta(),
                }],
                vec!["message.part.updated"],
            ),
            (
                "TurnStart",
                vec![StreamEvent::TurnStart],
                vec!["message.part.updated"],
            ),
            (
                "TurnFinish",
                vec![StreamEvent::TurnFinish {
                    reason: "stop".into(),
                    usage: Usage {
                        prompt_tokens: 10,
                        completion_tokens: 20,
                        total_tokens: 30,
                        cached_tokens: None,
                    },
                }],
                vec!["message.part.updated", "message.tokens"],
            ),
            (
                "ToolCall",
                vec![StreamEvent::ToolCall {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                    arguments: json!({"cmd": "ls"}),
                }],
                vec!["message.part.updated"],
            ),
            (
                "ToolStart",
                vec![StreamEvent::ToolStart {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                }],
                vec!["message.part.updated"],
            ),
            (
                "ToolOutput",
                vec![StreamEvent::ToolOutput {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                    content: "first chunk\n".into(),
                }],
                vec!["message.part.updated"],
            ),
            (
                "ToolEnd(ok)",
                vec![StreamEvent::ToolEnd {
                    call_id: Some("c1".into()),
                    name: "bash".into(),
                    result: "first chunk\n".into(),
                    is_error: false,
                    raw_result: Some("first chunk\n".into()),
                }],
                vec!["message.part.updated"],
            ),
            (
                "ToolError",
                vec![
                    StreamEvent::ToolCall {
                        call_id: Some("c1".into()),
                        name: "bash".into(),
                        arguments: json!({}),
                    },
                    StreamEvent::ToolError {
                        call_id: Some("c1".into()),
                        error: "boom".into(),
                    },
                ],
                vec!["message.part.updated"],
            ),
            (
                "ProviderError",
                vec![StreamEvent::ProviderError {
                    message: "rate limit".into(),
                }],
                vec!["session.error"],
            ),
            ("Finish", vec![StreamEvent::Finish], vec![]),
            (
                "TextDelta (seeded)",
                vec![
                    StreamEvent::TextBlockStart { metadata: meta() },
                    StreamEvent::TextDelta {
                        content: "x".into(),
                        metadata: meta(),
                    },
                ],
                vec!["message.part.updated"],
            ),
            (
                "TextBlockEnd (seeded)",
                vec![
                    StreamEvent::TextBlockStart { metadata: meta() },
                    StreamEvent::TextBlockEnd { metadata: meta() },
                ],
                vec!["message.part.updated"],
            ),
            (
                "ReasoningDelta (seeded)",
                vec![
                    StreamEvent::ReasoningBlockStart {
                        id: "r".into(),
                        metadata: meta(),
                    },
                    StreamEvent::ReasoningDelta {
                        id: "r".into(),
                        content: "y".into(),
                        metadata: meta(),
                    },
                ],
                vec!["message.part.updated"],
            ),
            (
                "ReasoningBlockEnd (seeded)",
                vec![
                    StreamEvent::ReasoningBlockStart {
                        id: "r".into(),
                        metadata: meta(),
                    },
                    StreamEvent::ReasoningBlockEnd {
                        id: "r".into(),
                        metadata: meta(),
                    },
                ],
                vec!["message.part.updated"],
            ),
        ];

        for (name, events, expected) in &cases {
            let seed: Vec<StreamEvent<TestState>> = events[..events.len() - 1].to_vec();
            let last = events.last().expect("non-empty");
            let got = translate_after_seed(&seed, last);
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

    // ─────────────── unit test for `append_to_part` ───────────────

    /// `append_to_part` mutates only the named part and emits nothing
    /// when the part is missing.
    #[test]
    fn append_to_part_emits_nothing_when_part_missing() {
        let state = new_state();
        let before = state.event_buffer.read().len();
        append_to_part(&state, "sess", "msg", "prt_does_not_exist", "x");
        assert_eq!(state.event_buffer.read().len(), before);
    }
}
