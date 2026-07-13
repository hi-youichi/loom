//! Stream-event translator (task P0.5).
//!
//! Translates loom `TypedAnyStreamEvent`s into opencode v1+v2 SSE events.
//! Mapping table:
//!
//! | loom variant                         | opencode event         | part-type   |
//! |--------------------------------------|------------------------|-------------|
//! | `Messages { kind: Message }`         | `message.part.updated` | `text`      |
//! | `Messages { kind: Thinking }`        | `message.part.updated` | `reasoning` |
//! | `TaskStart { node_id }`              | `message.part.updated` | `tool`      |
//! | `TaskEnd { node_id, result }`        | `message.part.updated` | `tool`      |
//! | `Usage`                              | log only               | -           |
//!
//! Conventions:
//! - One cumulative `message.part.updated` per (part_type, node_id).
//!   Repeated emissions overwrite the same part id so the TUI's
//!   reactive store coalesces in place.
//! - ToT/GoT-specific events are ignored in MVP — chat panel doesn't
//!   have a node-tree view.

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
            push_part(
                state,
                assistant_msg_id,
                session_id,
                "tool",
                json!({
                    "id": format!("tool-{node_id}"),
                    "type": "tool",
                    "callID": node_id,
                    "tool": node_id,
                    "state": { "status": "pending", "input": {} },
                }),
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
            push_part(
                state,
                assistant_msg_id,
                session_id,
                "tool",
                json!({
                    "id": format!("tool-{node_id}"),
                    "type": "tool",
                    "callID": node_id,
                    "tool": node_id,
                    "state": state_payload,
                }),
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
    use super::translate_chunk;
    use crate::state::new_state;
    use stream_event::types::message::MessageChunk;

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
}
