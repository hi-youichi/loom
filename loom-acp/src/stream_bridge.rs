//! Map Loom stream events to ACP SessionUpdate-equivalent structures
//!
//! [`loom::run_agent_with_options`]'s `on_event` callback receives [`loom::AnyStreamEvent`].
//! This module provides [`loom_event_to_updates`] to turn a single Loom event into zero or more [`StreamUpdate`],
//! which the upper layer sends as **session/update notifications** (no response) via the `agent_client_protocol` connection.
//! Protocol details are in [`crate::protocol`].
//!
//! ## SessionUpdate variants and Loom sources
//!
//! | Variant | Meaning | Loom source |
//! |---------|---------|-------------|
//! | **user_message_chunk** | Chunk of user message | Think node text output (so client can show it as user message). |
//! | **agent_message_chunk** | Chunk of agent reply (streamed text) | Reply node / other non-think message output. |
//! | **agent_thought_chunk** | Chunk of agent reasoning | `StreamEvent::Messages` with `chunk.kind == Thinking`, or `TaskStart` (node entry). |
//! | **tool_call** | New tool call started | Act node decides to call a tool: tool_call_id, name, input, kind, status: Pending. |
//! | **tool_call_update** | Update to existing tool call | Start -> Pending/Running; done -> Success/Failure + output/content. |
//! | plan / available_commands_update / current_mode_update | Plan, command list, mode | Optional; DUP/ToT/GoT etc. can map. |
//!
//! ## Tool call and request_permission order
//!
//! 1. Send **ToolCall** (new tool, status: Pending).
//! 2. If permission needed: call **session/request_permission**, wait for Client response.
//! 3. If allowed: send **ToolCallUpdate** (status: Running) -> execute tool -> **ToolCallUpdate** (Success/Failure + output).
//! 4. If denied or Cancelled: send **ToolCallUpdate** (Failure or denied), do not execute; on Cancelled end the turn with StopReason::Cancelled.
//!
//! [`StreamUpdate`] is a protocol-agnostic intermediate form; when wired to ACP it is converted to `SessionUpdate` and sent.
//!
//! [`stream_update_to_session_notification`] converts this module's [`StreamUpdate`] into
//! `agent_client_protocol::SessionNotification` for the upper layer to send via the connection.

use agent_client_protocol::{
    ContentChunk, SessionId, SessionNotification, SessionUpdate, ToolCall, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use loom::{AnyStreamEvent, MessageChunkKind, StreamEvent};
use serde_json::Value;

/// A single "sendable to Client" stream update, corresponding to ACP SessionUpdate variants.
///
/// Kept in sync with `agent_client_protocol::SessionUpdate` so the prompt callback can
/// convert to the protocol type and call `connection.send_notification(session/update)`.
#[derive(Clone, Debug)]
pub enum StreamUpdate {
    /// Chunk of user message (ACP `user_message_chunk`). Used for think node text so client shows it as user message.
    UserMessageChunk { text: String },

    /// Chunk of model output text (ACP `agent_message_chunk`).
    AgentMessageChunk { text: String },

    /// Chunk of agent reasoning / node entry (ACP `agent_thought_chunk`).
    AgentThoughtChunk { text: String },

    /// New tool call started (ACP `tool_call`, status: Pending).
    ToolCallStarted {
        /// Unique tool call id for this session; ToolCallUpdated uses the same id.
        tool_call_id: String,
        /// Tool name (e.g. "read_file").
        name: String,
        /// Raw arguments (JSON); can be turned into ToolCall input for ACP.
        input: Option<Value>,
        /// For Client icon/display; maps to ToolKind in ACP.
        kind: Option<String>,
    },

    /// Status/result update for an existing tool call (ACP `tool_call_update`).
    ToolCallUpdated {
        tool_call_id: String,
        /// e.g. "running" | "success" | "failure"; maps to ToolCallStatus in ACP.
        status: String,
        /// Result or error message.
        output: Option<String>,
    },
}

/// Convert one Loom stream event into zero or more [`StreamUpdate`]s.
///
/// If the event does not need to be pushed to the Client (e.g. some Checkpoint, Usage), returns an empty vec.
/// Within a single prompt turn, `tool_call_id` generation and consistency are the caller's responsibility (e.g. by call_id or incrementing id).
///
/// # Arguments
///
/// - `ev`: Loom's type-erased stream event (one of React/Dup/Tot/GoT).
///
/// # Returns
///
/// The list of updates for this event; may be empty.
///
/// # Example (in on_event callback)
///
/// ```ignore
/// let updates = loom_acp::loom_event_to_updates(ev);
/// for u in updates {
///     connection.send_session_update(session_id, u).await?;
/// }
/// ```
pub fn loom_event_to_updates(ev: &AnyStreamEvent) -> Vec<StreamUpdate> {
    match ev {
        AnyStreamEvent::React(e) => stream_event_to_updates_inner(e),
        AnyStreamEvent::Dup(e) => stream_event_to_updates_inner(e),
        AnyStreamEvent::Tot(e) => stream_event_to_updates_inner(e),
        AnyStreamEvent::Got(e) => stream_event_to_updates_inner(e),
    }
}

/// Uniform mapping for any `StreamEvent<S>` (uses only S-independent fields).
fn stream_event_to_updates_inner<S>(ev: &StreamEvent<S>) -> Vec<StreamUpdate>
where
    S: std::fmt::Debug + Clone + Send + Sync + 'static,
{
    match ev {
        StreamEvent::TaskStart { node_id } => vec![StreamUpdate::AgentThoughtChunk {
            text: format!("Entering {}", node_id),
        }],
        StreamEvent::Messages { chunk, metadata } => {
            // Only chunk.kind == Thinking (e.g. <think> tags) → thought.
            if chunk.kind == MessageChunkKind::Thinking {
                vec![StreamUpdate::AgentThoughtChunk {
                    text: chunk.content.clone(),
                }]
            } else if metadata.loom_node == "think" {
                // Think node text → user_message_chunk (so client shows it as user message).
                vec![StreamUpdate::UserMessageChunk {
                    text: chunk.content.clone(),
                }]
            } else {
                vec![StreamUpdate::AgentMessageChunk {
                    text: chunk.content.clone(),
                }]
            }
        }
        StreamEvent::ToolStart { call_id, name } => {
            let id = call_id
                .clone()
                .unwrap_or_else(|| format!("tool-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            vec![StreamUpdate::ToolCallStarted {
                tool_call_id: id,
                name: name.clone(),
                input: None,
                kind: None,
            }]
        }
        StreamEvent::ToolOutput { content, .. } => {
            vec![StreamUpdate::ToolCallUpdated {
                tool_call_id: String::new(), // caller fills from context
                status: "running".to_string(),
                output: Some(content.clone()),
            }]
        }
        StreamEvent::ToolEnd {
            call_id, result, is_error, ..
        } => {
            let id = call_id
                .clone()
                .unwrap_or_default();
            vec![StreamUpdate::ToolCallUpdated {
                tool_call_id: id,
                status: if *is_error {
                    "failure".to_string()
                } else {
                    "success".to_string()
                },
                output: Some(result.clone()),
            }]
        }
        _ => vec![],
    }
}

/// Convert this crate's [`StreamUpdate`] into ACP's [`SessionNotification`] for sending via the connection.
///
/// Returns `None` for `ToolCallUpdated` with empty `tool_call_id` (Loom ToolOutput may lack call_id).
pub fn stream_update_to_session_notification(
    session_id: &SessionId,
    u: &StreamUpdate,
) -> Option<SessionNotification> {
    let update = match u {
        StreamUpdate::UserMessageChunk { text } => {
            SessionUpdate::UserMessageChunk(ContentChunk::new(text.clone().into()))
        }
        StreamUpdate::AgentMessageChunk { text } => {
            SessionUpdate::AgentMessageChunk(ContentChunk::new(text.clone().into()))
        }
        StreamUpdate::AgentThoughtChunk { text } => {
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(text.clone().into()))
        }
        StreamUpdate::ToolCallStarted {
            tool_call_id,
            name,
            input,
            kind,
        } => {
            let id = ToolCallId::new(tool_call_id.as_str());
            let mut tc = ToolCall::new(id.clone(), name.clone()).status(ToolCallStatus::Pending);
            if let Some(ref v) = input {
                tc = tc.raw_input(v.clone());
            }
            if let Some(ref k) = kind {
                tc = tc.kind(name_to_tool_kind(k));
            }
            SessionUpdate::ToolCall(tc)
        }
        StreamUpdate::ToolCallUpdated {
            tool_call_id,
            status,
            output,
        } => {
            if tool_call_id.is_empty() {
                return None;
            }
            let status = match status.as_str() {
                "running" => ToolCallStatus::InProgress,
                "success" => ToolCallStatus::Completed,
                "failure" => ToolCallStatus::Failed,
                _ => ToolCallStatus::InProgress,
            };
            let mut fields = ToolCallUpdateFields::new().status(status);
            if let Some(ref s) = output {
                fields = fields.content(vec![s.clone().into()]);
            }
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(tool_call_id.as_str()),
                fields,
            ))
        }
    };
    Some(SessionNotification::new(session_id.clone(), update))
}

fn name_to_tool_kind(name: &str) -> ToolKind {
    let n = name.to_lowercase();
    if n.contains("read") || n.contains("file") {
        ToolKind::Read
    } else if n.contains("write") || n.contains("edit") {
        ToolKind::Edit
    } else if n.contains("delete") {
        ToolKind::Delete
    } else if n.contains("search") {
        ToolKind::Search
    } else if n.contains("run") || n.contains("command") || n.contains("exec") {
        ToolKind::Execute
    } else if n.contains("think") || n.contains("reason") {
        ToolKind::Think
    } else if n.contains("fetch") {
        ToolKind::Fetch
    } else {
        ToolKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom::{MessageChunk, StreamMetadata};

    fn react_messages(chunk: MessageChunk) -> AnyStreamEvent {
        AnyStreamEvent::React(StreamEvent::Messages {
            chunk,
            metadata: StreamMetadata {
                loom_node: "think".to_string(),
            },
        })
    }

    /// Messages + Thinking kind -> AgentThoughtChunk
    #[test]
    fn loom_event_to_updates_messages_thinking_produces_agent_thought_chunk() {
        let ev = react_messages(MessageChunk::thinking("先分析语法再检查类型"));
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::AgentThoughtChunk { text } => assert_eq!(text, "先分析语法再检查类型"),
            _ => panic!("应为 AgentThoughtChunk，得到 {:?}", updates[0]),
        }
    }

    /// Messages + Message kind from think node -> UserMessageChunk（think 节点文本作为用户消息返回）
    #[test]
    fn loom_event_to_updates_messages_think_node_produces_user_message_chunk() {
        let ev = react_messages(MessageChunk::message("我会先检查语法。"));
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::UserMessageChunk { text } => assert_eq!(text, "我会先检查语法。"),
            _ => panic!("应为 UserMessageChunk，得到 {:?}", updates[0]),
        }
    }

    /// Messages + Message kind from non-think node -> AgentMessageChunk
    #[test]
    fn loom_event_to_updates_messages_reply_node_produces_agent_message_chunk() {
        let ev = AnyStreamEvent::React(StreamEvent::Messages {
            chunk: MessageChunk::message("这是最终回复。"),
            metadata: StreamMetadata {
                loom_node: "reply".to_string(),
            },
        });
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::AgentMessageChunk { text } => assert_eq!(text, "这是最终回复。"),
            _ => panic!("应为 AgentMessageChunk，得到 {:?}", updates[0]),
        }
    }

    /// TaskStart -> 一条 AgentThoughtChunk，内容为 "Entering {node_id}"
    #[test]
    fn loom_event_to_updates_task_start_produces_agent_thought_chunk() {
        let ev = AnyStreamEvent::React(StreamEvent::TaskStart {
            node_id: "think".to_string(),
        });
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::AgentThoughtChunk { text } => {
                assert_eq!(text, "Entering think");
            }
            _ => panic!("应为 AgentThoughtChunk，得到 {:?}", updates[0]),
        }
    }

    /// ToolStart 带 call_id -> ToolCallStarted 使用该 id
    #[test]
    fn loom_event_to_updates_tool_start_with_call_id_uses_it() {
        let ev = AnyStreamEvent::React(StreamEvent::ToolStart {
            call_id: Some("call_abc".to_string()),
            name: "read_file".to_string(),
        });
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::ToolCallStarted {
                tool_call_id,
                name,
                ..
            } => {
                assert_eq!(tool_call_id, "call_abc");
                assert_eq!(name, "read_file");
            }
            _ => panic!("应为 ToolCallStarted，得到 {:?}", updates[0]),
        }
    }

    /// ToolStart 无 call_id -> ToolCallStarted 仍生成非空 id
    #[test]
    fn loom_event_to_updates_tool_start_without_call_id_generates_id() {
        let ev = AnyStreamEvent::React(StreamEvent::ToolStart {
            call_id: None,
            name: "run_cmd".to_string(),
        });
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::ToolCallStarted {
                tool_call_id,
                name,
                ..
            } => {
                assert!(!tool_call_id.is_empty(), "应生成非空 tool_call_id");
                assert!(tool_call_id.starts_with("tool-"));
                assert_eq!(name, "run_cmd");
            }
            _ => panic!("应为 ToolCallStarted，得到 {:?}", updates[0]),
        }
    }

    /// ToolEnd 带 call_id -> ToolCallUpdated 使用该 id，且可转为 SessionNotification
    #[test]
    fn loom_event_to_updates_tool_end_with_call_id_produces_notification() {
        let ev = AnyStreamEvent::React(StreamEvent::ToolEnd {
            call_id: Some("call_xyz".to_string()),
            name: "read_file".to_string(),
            result: "file content".to_string(),
            is_error: false,
        });
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        match &updates[0] {
            StreamUpdate::ToolCallUpdated {
                tool_call_id,
                status,
                output,
            } => {
                assert_eq!(tool_call_id, "call_xyz");
                assert_eq!(status, "success");
                assert_eq!(output.as_deref(), Some("file content"));
            }
            _ => panic!("应为 ToolCallUpdated，得到 {:?}", updates[0]),
        }
        let session_id = SessionId::new("test-session".to_string());
        let notif = stream_update_to_session_notification(&session_id, &updates[0]);
        assert!(notif.is_some(), "ToolEnd 带 call_id 应产生 SessionNotification");
    }

    /// ToolOutput 当前产生空 tool_call_id -> stream_update_to_session_notification 返回 None
    #[test]
    fn stream_update_to_session_notification_tool_output_empty_id_returns_none() {
        let ev = AnyStreamEvent::React(StreamEvent::ToolOutput {
            call_id: Some("call_123".to_string()),
            name: "run_cmd".to_string(),
            content: "running...".to_string(),
        });
        let updates = loom_event_to_updates(&ev);
        assert_eq!(updates.len(), 1);
        let session_id = SessionId::new("sess-1".to_string());
        // 当前实现 ToolOutput 映射为 tool_call_id: ""，故 notification 为 None
        let notif = stream_update_to_session_notification(&session_id, &updates[0]);
        assert!(notif.is_none(), "当前 ToolOutput 映射空 id，应返回 None");
    }

    /// AgentThoughtChunk / UserMessageChunk / AgentMessageChunk 均可转为 SessionNotification
    #[test]
    fn stream_update_to_session_notification_thought_and_message_produce_some() {
        let session_id = SessionId::new("sess-2".to_string());
        let thought = StreamUpdate::AgentThoughtChunk {
            text: "推理片段".to_string(),
        };
        assert!(stream_update_to_session_notification(&session_id, &thought).is_some());
        let user_msg = StreamUpdate::UserMessageChunk {
            text: "用户消息片段".to_string(),
        };
        assert!(stream_update_to_session_notification(&session_id, &user_msg).is_some());
        let message = StreamUpdate::AgentMessageChunk {
            text: "回复片段".to_string(),
        };
        assert!(stream_update_to_session_notification(&session_id, &message).is_some());
    }
}
