//! Map Loom stream events to ACP SessionUpdate-equivalent structures
//!
//! [`agent::run::run_agent_with_options`]'s `on_event` callback receives [`agent::TypedAnyStreamEvent`].
//! This module provides [`loom_event_to_updates`] to turn a single Loom event into zero or more [`StreamUpdate`],
//! which the upper layer sends as **session/update notifications** (no response) via the `agent_client_protocol` connection.
//! Protocol details are in [`crate::protocol`].
//!
//! ## SessionUpdate variants and Loom sources
//!
//! | Variant | Meaning | Loom source |
//! |---------|---------|-------------|
//! | **user_message_chunk** | Chunk of user message | History replay only (`Message::User`). |
//! | **agent_message_chunk** | Chunk of agent reply (streamed text) | Any node's non-Thinking text output. |
//! | **agent_thought_chunk** | Chunk of agent reasoning | `StreamEvent::Messages` with `chunk.kind == Thinking`, or `TaskStart` (node entry). |
//! | **tool_call** | New tool call started | Act node decides to call a tool: tool_call_id, name, input, kind, status: Pending. |
//! | **tool_call_update** | Update to existing tool call | Start -> Pending/Running; done -> Success/Failure + output/content. |
//! | plan / available_commands_update / current_mode_update | Plan, command list, mode | Optional; DUP/ToT/GoT etc. can map. |
//! | **session_info_update** | Session metadata (title) update | Agent pushes title or other metadata to client. |
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

use crate::content::extract_locations;
use loom_util::text::truncate::truncate_tail;
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, CurrentModeUpdate, Diff, MessageId, Plan, PlanEntry,
    PlanEntryPriority, PlanEntryStatus, SessionId, SessionInfoUpdate, SessionModeId,
    SessionNotification, SessionUpdate, Terminal, TerminalId, TextContent, ToolCall, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    ToolCallContent, UsageUpdate,
};
use loom_llm::message::Message;
use loom_stream::{MessageChunkKind, StreamEvent};
use agent::run::TypedAnyStreamEvent;
use serde_json::Value;
use std::sync::Mutex;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A single "sendable to Client" stream update, corresponding to ACP SessionUpdate variants.
///
/// Kept in sync with `agent_client_protocol::SessionUpdate` so the prompt callback can
/// convert to the protocol type and call `connection.send_notification(session/update)`.
#[derive(Clone, Debug)]
pub enum StreamUpdate {
    /// Chunk of user message (ACP `user_message_chunk`). History replay only; streaming never produces this variant.
    UserMessageChunk {
        text: String,
        message_id: Option<String>,
    },

    /// Chunk of model output text (ACP `agent_message_chunk`).
    AgentMessageChunk {
        text: String,
        message_id: Option<String>,
    },

    /// Chunk of agent reasoning / node entry (ACP `agent_thought_chunk`).
    AgentThoughtChunk {
        text: String,
        message_id: Option<String>,
    },

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
        /// Result or error message (possibly normalized/truncated).
        output: Option<String>,
        /// Full un-normalized result. When set, used for ACP `raw_output` instead of `output`.
        raw_output: Option<String>,
    },

    /// File diff update (ACP `tool_call_update` with diff content).
    /// Shows file modifications in a format suitable for client display.
    Diff {
        /// The tool call that produced this diff.
        tool_call_id: String,
        /// File path
        path: String,
        /// Previous content (optional)
        old_text: Option<String>,
        /// New content
        new_text: String,
    },

    /// Session metadata update (ACP `session_info_update`).
    /// Used to push title and related metadata changes to the client in real time.
    SessionInfoUpdate { title: String },

    /// Agent execution plan (ACP `plan`).
    /// Reports the agent's planned tasks with their priority and status.
    Plan { entries: Vec<PlanEntry> },

    /// Context window usage update (ACP `usage_update`).
    /// Reports current context token usage and total window size.
    UsageUpdate {
        /// Tokens currently in context.
        used: u64,
        /// Total context window size in tokens.
        size: u64,
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
pub fn loom_event_to_updates(ev: &TypedAnyStreamEvent) -> Vec<StreamUpdate> {
    match ev {
        TypedAnyStreamEvent::React(e) => {
            let mut updates = stream_event_to_updates_inner(e);
            if let Some(title_update) = extract_title_from_react_event(e) {
                updates.push(title_update);
            }
            updates
        }
        TypedAnyStreamEvent::Dup(e) => stream_event_to_updates_inner(e),
        TypedAnyStreamEvent::Tot(e) => stream_event_to_updates_inner(e),
        TypedAnyStreamEvent::Got(e) => stream_event_to_updates_inner(e),
    }
}

fn extract_title_from_react_event(ev: &StreamEvent<agent::state::ReActState>) -> Option<StreamUpdate> {
    match ev {
        StreamEvent::Updates { node_id, state, .. } if node_id == "title" => state
            .summary
            .as_ref()
            .map(|title| StreamUpdate::SessionInfoUpdate {
                title: title.clone(),
            }),
        _ => None,
    }
}

fn resolve_tool_call_id(call_id: &Option<String>) -> String {
    call_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("tool-{}", Uuid::new_v4()))
}

/// Uniform mapping for any `StreamEvent<S>` (uses only S-independent fields).
fn stream_event_to_updates_inner<S>(ev: &StreamEvent<S>) -> Vec<StreamUpdate>
where
    S: std::fmt::Debug + Clone + Send + Sync + 'static,
{
    match ev {
        StreamEvent::TaskStart { node_id: _, .. } => vec![],
        StreamEvent::Messages { chunk, .. } => {
            if chunk.kind == MessageChunkKind::Thinking {
                vec![StreamUpdate::AgentThoughtChunk {
                    text: chunk.content.clone(),
                    message_id: None,
                }]
            } else {
                vec![StreamUpdate::AgentMessageChunk {
                    text: chunk.content.clone(),
                    message_id: None,
                }]
            }
        }
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            let id = resolve_tool_call_id(call_id);
            vec![StreamUpdate::ToolCallStarted {
                tool_call_id: id,
                name: name.clone(),
                input: Some(arguments.clone()),
                kind: None,
            }]
        }
        StreamEvent::ToolStart { call_id, name: _ } => {
            let id = resolve_tool_call_id(call_id);
            vec![StreamUpdate::ToolCallUpdated {
                tool_call_id: id,
                status: "running".to_string(),
                output: None,
                raw_output: None,
            }]
        }
        StreamEvent::ToolOutput {
            call_id, content, ..
        } => {
            let id = resolve_tool_call_id(call_id);
            
            // Try to deserialize content as ToolCallContent to check for Diff
    if let Ok(tool_core::ToolCallContent::Diff { path, old_text, new_text }) =
                serde_json::from_str::<tool_core::ToolCallContent>(content)
            {
                return vec![StreamUpdate::Diff {
                    tool_call_id: id,
                    path,
                    old_text,
                    new_text,
                }];
            }
            
            // Handle regular tool output
            vec![StreamUpdate::ToolCallUpdated {
                tool_call_id: id,
                status: "running".to_string(),
                output: Some(content.clone()),
                raw_output: None,
            }]
        }
        StreamEvent::ToolEnd {
            call_id,
            name,
            result,
            is_error,
            raw_result,
        } => {
            let id = resolve_tool_call_id(call_id);

            let is_diff_output = raw_result
                .as_deref()
                .or(Some(result.as_str()))
                .and_then(|s| serde_json::from_str::<tool_core::ToolCallContent>(s).ok())
                .map(|c| matches!(c, tool_core::ToolCallContent::Diff { .. }))
                .unwrap_or(false);

            let mut updates = if is_diff_output {
                let diff_content = raw_result
                    .as_deref()
                    .or(Some(result.as_str()))
                    .and_then(|s| serde_json::from_str::<tool_core::ToolCallContent>(s).ok());

                let mut updates = Vec::new();

                if let Some(tool_core::ToolCallContent::Diff { path, old_text, new_text }) = diff_content {
                    updates.push(StreamUpdate::Diff {
                        tool_call_id: id.clone(),
                        path,
                        old_text,
                        new_text,
                    });
                }

                updates.push(StreamUpdate::ToolCallUpdated {
                    tool_call_id: id,
                    status: if *is_error {
                        "failure".to_string()
                    } else {
                        "success".to_string()
                    },
                    output: None,
                    raw_output: None,
                });
                updates
            } else {
                vec![StreamUpdate::ToolCallUpdated {
                    tool_call_id: id,
                    status: if *is_error {
                        "failure".to_string()
                    } else {
                        "success".to_string()
                    },
                    output: Some(result.clone()),
                    raw_output: raw_result.clone(),
                }]
            };

            if name == "todo_write" && !is_error {
                if let Some(entries) = parse_todo_result_to_plan_entries(result) {
                    updates.push(StreamUpdate::Plan { entries });
                }
            }

            updates
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
        StreamUpdate::UserMessageChunk { text, message_id } => {
            let mut chunk = ContentChunk::new(text.clone().into());
            chunk = chunk.message_id(message_id.clone().map(MessageId::new));
            SessionUpdate::UserMessageChunk(chunk)
        }
        StreamUpdate::AgentMessageChunk { text, message_id } => {
            let mut chunk = ContentChunk::new(text.clone().into());
            chunk = chunk.message_id(message_id.clone().map(MessageId::new));
            SessionUpdate::AgentMessageChunk(chunk)
        }
        StreamUpdate::AgentThoughtChunk { text, message_id } => {
            let mut chunk = ContentChunk::new(text.clone().into());
            chunk = chunk.message_id(message_id.clone().map(MessageId::new));
            SessionUpdate::AgentThoughtChunk(chunk)
        }
        StreamUpdate::ToolCallStarted {
            tool_call_id,
            name,
            input,
            kind,
        } => {
            let tc = create_tool_call(tool_call_id, name, input.as_ref(), kind.as_deref());
            tracing::trace!(
                tool_call_id = %tool_call_id,
                name = %name,
                input = ?input,
                kind = ?kind,
                tc = ?tc,
                "tool_call session update"
            );
            SessionUpdate::ToolCall(tc)
        }
        StreamUpdate::ToolCallUpdated {
            tool_call_id,
            status,
            output,
            raw_output,
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
                let effective_raw = raw_output.as_deref().unwrap_or(s);
                fields = fields
                    .content(vec![s.clone().into()])
                    .raw_output(parse_text_output_to_raw_value(effective_raw));
            }
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(tool_call_id.as_str()),
                fields,
            ))
        }
        StreamUpdate::Diff { tool_call_id, path, old_text, new_text } => {
            let mut fields = ToolCallUpdateFields::new()
                .content(vec![
                    ToolCallContent::Diff(
                        Diff::new(path.clone(), new_text.clone())
                            .old_text(old_text.clone()),
                    )
                ]);
            
            let status = ToolCallStatus::Completed;
            fields = fields.status(status);
            
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(tool_call_id.as_str()),
                fields,
            ))
        }
        StreamUpdate::SessionInfoUpdate { title } => {
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title.clone()))
        }
        StreamUpdate::Plan { entries } => {
            SessionUpdate::Plan(Plan::new(entries.clone()))
        }
        StreamUpdate::UsageUpdate { used, size } => {
            SessionUpdate::UsageUpdate(UsageUpdate::new(*used, *size))
        }
    };
    Some(SessionNotification::new(session_id.clone(), update))
}

fn extract_text_from_result(result: &str) -> Option<String> {
    if let Ok(content) = serde_json::from_str::<tool_core::ToolCallContent>(result) {
        return Some(content.into_text());
    }
    if let Ok(s) = serde_json::from_str::<String>(result) {
        return Some(s);
    }
    Some(result.to_string())
}

fn parse_todo_result_to_plan_entries(result: &str) -> Option<Vec<PlanEntry>> {
    let text = extract_text_from_result(result)?;
    let json_start = text.find('[')?;
    let json_str = &text[json_start..];
    let items: Vec<serde_json::Value> = serde_json::from_str(json_str).ok()?;
    let entries: Vec<PlanEntry> = items
        .iter()
        .filter_map(|t| {
            let content = t.get("content")?.as_str()?.to_string();
            let priority = match t.get("priority")?.as_str()? {
                "high" => PlanEntryPriority::High,
                "medium" => PlanEntryPriority::Medium,
                _ => PlanEntryPriority::Low,
            };
            let status = match t.get("status")?.as_str()? {
                "in_progress" => PlanEntryStatus::InProgress,
                "completed" | "cancelled" => PlanEntryStatus::Completed,
                _ => PlanEntryStatus::Pending,
            };
            Some(PlanEntry::new(content, priority, status))
        })
        .collect();
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn parse_text_output_to_raw_value(output: &str) -> serde_json::Value {
    serde_json::from_str(output).unwrap_or_else(|_| serde_json::json!(output))
}

pub fn name_to_tool_kind(name: &str) -> ToolKind {
    let n = name.to_lowercase();
    if n.contains("read") {
        ToolKind::Read
    } else if n.contains("write") || n.contains("edit") {
        ToolKind::Edit
    } else if n.contains("delete") || n.contains("remove") {
        ToolKind::Delete
    } else if n.contains("move") || n.contains("rename") {
        ToolKind::Move
    } else if n.contains("search") || n.contains("grep") || n.contains("glob") {
        ToolKind::Search
    } else if n.contains("run")
        || n.contains("bash")
        || n.contains("command")
        || n.contains("exec")
        || n.contains("shell")
    {
        ToolKind::Execute
    } else if n.contains("think") || n.contains("reason") {
        ToolKind::Think
    } else if n.contains("fetch") {
        ToolKind::Fetch
    } else if n.contains("switch_mode")
        || n.contains("switchmode")
        || n.contains("set_mode")
        || n.contains("setmode")
    {
        ToolKind::SwitchMode
    } else {
        ToolKind::Other
    }
}

pub struct SessionNotifier {
    tx: mpsc::Sender<SessionNotification>,
    session_id: SessionId,
    current_message_id: Mutex<Option<String>>,
    /// Context window size in tokens; when set, usage_update notifications are emitted.
    context_window_size: Option<u64>,
}

impl SessionNotifier {
    pub fn new(tx: mpsc::Sender<SessionNotification>, session_id: SessionId) -> Self {
        Self {
            tx,
            session_id,
            current_message_id: Mutex::new(None),
            context_window_size: None,
        }
    }

    /// Set the context window size for `usage_update` notifications.
    /// When set, each `StreamEvent::Usage` emits an `UsageUpdate` to the client.
    pub fn with_context_window_size(mut self, size: u64) -> Self {
        self.context_window_size = Some(size);
        self
    }

    pub async fn send_event(&self, event: &TypedAnyStreamEvent) {
        let mut updates = loom_event_to_updates(event);
        if let Some(size) = self.context_window_size {
            if let Some(used) = extract_usage_tokens(event) {
                updates.push(StreamUpdate::UsageUpdate { used, size });
            }
        }
        for u in updates {
            let u = self.inject_message_id(u);
            if let Some(notif) = stream_update_to_session_notification(&self.session_id, &u) {
                if let Err(e) = self.tx.send(notif).await {
                    tracing::error!(session_id = %self.session_id, error = %e, "Failed to send stream event notification");
                }
            }
        }
    }

    pub fn try_send_event(&self, event: &TypedAnyStreamEvent) {
        let mut updates = loom_event_to_updates(event);
        if let Some(size) = self.context_window_size {
            if let Some(used) = extract_usage_tokens(event) {
                updates.push(StreamUpdate::UsageUpdate { used, size });
            }
        }
        self.send_updates(updates);
    }

    pub fn try_send_stream_event(&self, event: &loom_stream::TypedAnyStreamEvent) {
        let loom_stream::TypedAnyStreamEvent::React(e) = event;
        let typed = TypedAnyStreamEvent::React(e.clone());
        self.try_send_event(&typed);
    }

    fn send_updates(&self, updates: Vec<StreamUpdate>) {
        for u in updates {
            let u = self.inject_message_id(u);
            if let Some(notif) = stream_update_to_session_notification(&self.session_id, &u) {
                match self.tx.try_send(notif) {
                    Ok(_) => {
                        tracing::trace!(
                            session_id = %self.session_id,
                            update_type = ?u,
                            "Session notification sent successfully"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            session_id = %self.session_id,
                            update_type = ?u,
                            error = %e,
                            "Failed to send session notification (channel full or closed)"
                        );
                    }
                }
            }
        }
    }

    fn inject_message_id(&self, update: StreamUpdate) -> StreamUpdate {
        match update {
            StreamUpdate::AgentMessageChunk { text, .. } => {
                let id = self.current_message_id
                    .lock()
                    .unwrap()
                    .get_or_insert_with(|| Uuid::new_v4().to_string())
                    .clone();
                StreamUpdate::AgentMessageChunk { text, message_id: Some(id) }
            }
            StreamUpdate::AgentThoughtChunk { text, .. } => {
                let id = self.current_message_id
                    .lock()
                    .unwrap()
                    .get_or_insert_with(|| Uuid::new_v4().to_string())
                    .clone();
                StreamUpdate::AgentThoughtChunk { text, message_id: Some(id) }
            }
            StreamUpdate::UserMessageChunk { text, .. } => {
                let id = self.current_message_id
                    .lock()
                    .unwrap()
                    .get_or_insert_with(|| Uuid::new_v4().to_string())
                    .clone();
                StreamUpdate::UserMessageChunk { text, message_id: Some(id) }
            }
            other => {
                *self.current_message_id.lock().unwrap() = None;
                other
            }
        }
    }

    pub fn try_send_plan(&self, entries: Vec<PlanEntry>) {
        let notif = stream_update_to_session_notification(
            &self.session_id,
            &StreamUpdate::Plan { entries },
        );
        if let Some(notif) = notif {
            if let Err(e) = self.tx.try_send(notif) {
                tracing::warn!(
                    session_id = %self.session_id,
                    error = %e,
                    "Failed to send plan notification"
                );
            }
        }
    }

    pub async fn send_history(&self, messages: &[Message]) {
        tracing::debug!(
            session_id = %self.session_id,
            total_messages = messages.len(),
            "send_history started"
        );
        let mut tool_calls_map: HashMap<String, (String, Option<Value>)> = HashMap::new();
        let mut sent_count: usize = 0;
        let mut skipped_system: usize = 0;

        for (idx, message) in messages.iter().enumerate() {
            let msg_type = match message {
                Message::User(_) => "user",
                Message::Assistant(_) => "assistant",
                Message::Tool { .. } => "tool",
                Message::System(_) => "system",
            };
            let notifications = match message {
Message::User(content) => vec![SessionNotification::new(
                    self.session_id.clone(),
                    SessionUpdate::UserMessageChunk(
                        ContentChunk::new(
                            ContentBlock::Text(
                                TextContent::new(content.as_text().to_string()),
                            ),
                        )
                        .message_id(Some(MessageId::new(Uuid::new_v4().to_string()))),
                    ),
                )],
                Message::Assistant(payload) => {
                    let is_empty_assistant = payload.content.trim().is_empty()
                        && payload
                            .reasoning_content
                            .as_ref()
                            .is_none_or(|s| s.trim().is_empty())
                        && payload.tool_calls.is_empty();
                    if is_empty_assistant {
                        continue;
                    }

                    for tc in &payload.tool_calls {
                        tool_calls_map.insert(
                            tc.id.clone(),
                            (tc.name.clone(), serde_json::from_str(&tc.arguments).ok()),
                        );
                    }

                    let msg_id = Uuid::new_v4().to_string();
                    let mut notifs = vec![SessionNotification::new(
                        self.session_id.clone(),
                        SessionUpdate::AgentMessageChunk(
                            ContentChunk::new(
                                payload.content.clone().into(),
                            )
                            .message_id(Some(MessageId::new(msg_id))),
                        ),
                    )];

                    // Send reasoning content (ACP agent_thought_chunk) so thinking models stream reasoning to clients.
                    if let Some(ref reasoning) = payload.reasoning_content {
                        if !reasoning.trim().is_empty() {
                            let reasoning_msg_id = Uuid::new_v4().to_string();
                            notifs.push(SessionNotification::new(
                                self.session_id.clone(),
                                SessionUpdate::AgentThoughtChunk(
                                    ContentChunk::new(
                                        reasoning.clone().into(),
                                    )
                                    .message_id(Some(MessageId::new(reasoning_msg_id))),
                                ),
                            ));
                        }
                    }

                    for tc in &payload.tool_calls {
                        let args = serde_json::from_str::<Value>(&tc.arguments).ok();
                        let tool_call = create_tool_call(&tc.id, &tc.name, args.as_ref(), None);
                        notifs.push(SessionNotification::new(
                            self.session_id.clone(),
                            SessionUpdate::ToolCall(tool_call),
                        ));
                    }

                    notifs
                }
                Message::Tool {
                    tool_call_id,
                    content,
                } => {
                    let id = ToolCallId::new(tool_call_id.clone());
                    let acp_content = match content {
                        tool_core::ToolCallContent::Text(t) => {
                            ToolCallContent::from(
                                ContentBlock::Text(
                                    TextContent::new(t.clone()),
                                ),
                            )
                        }
                        tool_core::ToolCallContent::Diff {
                            path,
                            old_text,
                            new_text,
                        } => ToolCallContent::Diff(
                            Diff::new(path.clone(), new_text.clone())
                                .old_text(old_text.clone()),
                        ),
                        tool_core::ToolCallContent::Terminal { terminal_id } => {
                            ToolCallContent::Terminal(Terminal::new(
                                TerminalId::new(terminal_id.clone()),
                            ))
                        }
                    };
                    let fields = ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Completed)
                        .content(vec![acp_content])
                        .raw_output(tool_call_content_to_raw_output(content));
                    let tool_call_update = ToolCallUpdate::new(id, fields);

                    vec![SessionNotification::new(
                        self.session_id.clone(),
                        SessionUpdate::ToolCallUpdate(tool_call_update),
                    )]
                }
                Message::System(_) => {
                    skipped_system += 1;
                    continue;
                }
            };

            tracing::trace!(
                session_id = %self.session_id,
                index = idx,
                msg_type = msg_type,
                notification_count = notifications.len(),
                "Replaying history message"
            );

            for notif in notifications {
                if let Err(e) = self.tx.send(notif).await {
                    tracing::error!(session_id = %self.session_id, index = idx, msg_type = msg_type, error = %e, "Failed to send session update during history replay");
                } else {
                    sent_count += 1;
                }
            }
        }

        tracing::debug!(
            session_id = %self.session_id,
            total_messages = messages.len(),
            notifications_sent = sent_count,
            system_skipped = skipped_system,
            "send_history completed"
        );
    }

    pub async fn send_current_mode(&self, mode_id: &str) {
        let notif = SessionNotification::new(
            self.session_id.clone(),
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(SessionModeId::new(
                mode_id.to_string(),
            ))),
        );
        if let Err(e) = self.tx.send(notif).await {
            tracing::error!(session_id = %self.session_id, error = %e, "Failed to send current mode update");
        }
    }

    pub fn try_send_current_mode(&self, mode_id: &str) {
        let notif = SessionNotification::new(
            self.session_id.clone(),
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(SessionModeId::new(
                mode_id.to_string(),
            ))),
        );
        let _ = self.tx.try_send(notif);
    }

    pub fn try_send_session_info_update(&self, title: &str) {
        let notif = SessionNotification::new(
            self.session_id.clone(),
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title.to_string())),
        );
        let _ = self.tx.try_send(notif);
    }
}

/// Extract prompt-token count from a Loom stream event if it is a Usage event.
fn extract_usage_tokens(ev: &TypedAnyStreamEvent) -> Option<u64> {
    let usage = match ev {
        TypedAnyStreamEvent::React(e) => extract_usage_inner(e),
        TypedAnyStreamEvent::Dup(e) => extract_usage_inner(e),
        TypedAnyStreamEvent::Tot(e) => extract_usage_inner(e),
        TypedAnyStreamEvent::Got(e) => extract_usage_inner(e),
    };
    usage.map(|p| p as u64)
}

fn extract_usage_inner<S>(ev: &StreamEvent<S>) -> Option<u32>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    match ev {
        StreamEvent::Usage { prompt_tokens, .. } => Some(*prompt_tokens),
        _ => None,
    }
}

fn tool_call_content_to_raw_output(content: &tool_core::ToolCallContent) -> Value {
    match content {
        tool_core::ToolCallContent::Text(text) => serde_json::json!(text),
        tool_core::ToolCallContent::Diff {
            path,
            old_text,
            new_text,
        } => serde_json::json!({
            "type": "diff",
            "path": path,
            "oldText": old_text,
            "newText": new_text,
        }),
        tool_core::ToolCallContent::Terminal { terminal_id } => serde_json::json!({
            "type": "terminal",
            "terminalId": terminal_id,
        }),
    }
}

pub fn create_tool_call(
    tool_call_id: &str,
    name: &str,
    input: Option<&Value>,
    kind_override: Option<&str>,
) -> ToolCall {
    let id = ToolCallId::new(tool_call_id);
    let title = generate_tool_title(name, input);
    let effective_kind = kind_override
        .map(name_to_tool_kind)
        .unwrap_or_else(|| name_to_tool_kind(name));
    let mut tc = ToolCall::new(id.clone(), title)
        .status(ToolCallStatus::Pending)
        .kind(effective_kind);

    if let Some(v) = input {
        tc = tc.raw_input(v.clone());
        let locations: Vec<ToolCallLocation> = extract_locations(name, v)
            .into_iter()
            .map(|loc| ToolCallLocation::new(loc.path).line(loc.line))
            .collect();
        if !locations.is_empty() {
            tc = tc.locations(locations);
        }
    }
    tc
}

pub fn generate_tool_title(name: &str, input: Option<&serde_json::Value>) -> String {
    let kind = name_to_tool_kind(name);
    let target = extract_target_from_input(name, input);

    match kind {
        // Execute and Other: show command directly without "Running" prefix
        ToolKind::Execute | ToolKind::Other => target.unwrap_or_else(|| name.to_string()),
        // Others: use verb prefix
        _ => {
            let verb = match kind {
                ToolKind::Read => "Reading",
                ToolKind::Edit => "Editing",
                ToolKind::Delete => "Deleting",
                ToolKind::Move => "Moving",
                ToolKind::Search => "Searching",
                ToolKind::Think => "Thinking",
                ToolKind::Fetch => "Fetching",
                ToolKind::SwitchMode => "Switching mode",
                // Execute/Other are handled above; Unknown/future variants
                // (ToolKind is #[non_exhaustive]) fall back to the tool name.
                _ => return target.unwrap_or_else(|| name.to_string()),
            };
            match target {
                Some(t) => format!("{} {}", verb, t),
                None => format!("{} {}", verb, name),
            }
        }
    }
}

fn extract_target_from_input(name: &str, input: Option<&serde_json::Value>) -> Option<String> {
    let obj = input?.as_object()?;
    let n = name.to_lowercase();

    let keys: &[&[&str]] = if n.contains("read")
        || n.contains("file")
        || n.contains("write")
        || n.contains("edit")
        || n.contains("delete")
        || n.contains("remove")
    {
        &[&["path", "file_path", "filepath"]]
    } else if n.contains("move") || n.contains("rename") {
        &[
            &["source", "src", "path"],
            &["destination", "dest", "target"],
        ]
    } else if n.contains("search") || n.contains("grep") || n.contains("glob") {
        &[&["pattern", "query", "search"]]
    } else if n.contains("run")
        || n.contains("bash")
        || n.contains("command")
        || n.contains("exec")
        || n.contains("shell")
    {
        &[&["command", "cmd"]]
    } else if n.contains("fetch") {
        &[&["url", "uri"]]
    } else if n.contains("invoke_agent") || n.contains("invoke") && n.contains("agent") {
        // Special handling for invoke_agent: extract agent names from agents array
        if let Some(agents) = obj.get("agents").and_then(|v| v.as_array()) {
            if !agents.is_empty() {
                let agent_names: Vec<String> = agents
                    .iter()
                    .filter_map(|agent| {
                        agent
                            .get("agent")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                if !agent_names.is_empty() {
                    return Some(format!(
                        "{} agent(s): {}",
                        agent_names.len(),
                        agent_names.join(", ")
                    ));
                }
            }
        }
        &[]
    } else {
        &[]
    };

    for key_group in keys {
        for &key in *key_group {
            if let Some(val) = obj.get(key).and_then(|v| v.as_str()) {
                // Commands should not be truncated - they're the primary information
                let display = if key == "command" || key == "cmd" {
                    val.to_string()
                } else {
                    truncate_tail(val, 60)
                };
                return Some(display);
            }
        }
    }
    None
}


