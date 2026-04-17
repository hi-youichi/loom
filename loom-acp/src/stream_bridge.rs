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
use agent_client_protocol::{
    ContentChunk, CurrentModeUpdate, SessionId, SessionModeId, SessionNotification,
    SessionInfoUpdate, SessionUpdate, Terminal, TerminalId, ToolCall, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use loom::message::Message;
use loom::{AnyStreamEvent, MessageChunkKind, StreamEvent};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

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
        /// Result or error message (possibly normalized/truncated).
        output: Option<String>,
        /// Full un-normalized result. When set, used for ACP `raw_output` instead of `output`.
        raw_output: Option<String>,
    },

    /// Incremental tool call argument chunk (during LLM streaming).
    /// Maps to ToolCallUpdate with raw_input_delta if ACP supports it, otherwise ignored.
    ToolCallChunk {
        tool_call_id: String,
        /// Tool name (only present in first chunk).
        name: Option<String>,
        /// Incremental arguments JSON delta.
        arguments_delta: String,
    },

    /// Session metadata update (ACP `session_info_update`).
    /// Used to push title and related metadata changes to the client in real time.
    SessionInfoUpdate { title: String },
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
        AnyStreamEvent::React(e) => {
            let mut updates = stream_event_to_updates_inner(e);
            if let Some(title_update) = extract_title_from_react_event(e) {
                updates.push(title_update);
            }
            updates
        }
        AnyStreamEvent::Dup(e) => stream_event_to_updates_inner(e),
        AnyStreamEvent::Tot(e) => stream_event_to_updates_inner(e),
        AnyStreamEvent::Got(e) => stream_event_to_updates_inner(e),
    }
}

fn extract_title_from_react_event(ev: &StreamEvent<loom::ReActState>) -> Option<StreamUpdate> {
    match ev {
        StreamEvent::Updates { node_id, state, .. } if node_id == "title" => {
            state.summary.as_ref().map(|title| StreamUpdate::SessionInfoUpdate {
                title: title.clone(),
            })
        }
        _ => None,
    }
}

/// Uniform mapping for any `StreamEvent<S>` (uses only S-independent fields).
fn stream_event_to_updates_inner<S>(ev: &StreamEvent<S>) -> Vec<StreamUpdate>
where
    S: std::fmt::Debug + Clone + Send + Sync + 'static,
{
    match ev {
        StreamEvent::TaskStart { node_id: _, .. } => vec![],
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
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            let id = call_id
                .clone()
                .unwrap_or_else(|| format!("tool-{}", Uuid::new_v4()));
            vec![StreamUpdate::ToolCallStarted {
                tool_call_id: id,
                name: name.clone(),
                input: Some(arguments.clone()),
                kind: None,
            }]
        }
        StreamEvent::ToolStart { call_id, name: _ } => {
            let id = call_id.clone().unwrap_or_default();
            if id.is_empty() {
                vec![]
            } else {
                vec![StreamUpdate::ToolCallUpdated {
                    tool_call_id: id,
                    status: "running".to_string(),
                    output: None,
                    raw_output: None,
                }]
            }
        }
        StreamEvent::ToolOutput {
            call_id, content, ..
        } => {
            // Prefer Loom's call_id so the client can attach streamed tool output to the right tool call.
            // If call_id is missing, we keep an empty id; the notification layer will drop it.
            let id = call_id.clone().unwrap_or_default();
            vec![StreamUpdate::ToolCallUpdated {
                tool_call_id: id,
                status: "running".to_string(),
                output: Some(content.clone()),
                raw_output: None,
            }]
        }
        StreamEvent::ToolEnd {
            call_id,
            result,
            is_error,
            raw_result,
            ..
        } => {
            let id = call_id.clone().unwrap_or_default();
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
        }
        StreamEvent::ToolCallChunk {
            call_id,
            name,
            arguments_delta,
        } => {
            // Generate or use existing call_id
            let id = call_id
                .clone()
                .unwrap_or_else(|| format!("tool-chunk-{}", Uuid::new_v4()));
            vec![StreamUpdate::ToolCallChunk {
                tool_call_id: id,
                name: name.clone(),
                arguments_delta: arguments_delta.clone(),
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
        StreamUpdate::ToolCallChunk {
            tool_call_id,
            name,
            arguments_delta,
        } => {
            if let Some(tool_name) = name {
                let tc = create_tool_call(
                    tool_call_id,
                    tool_name,
                    parse_arguments_delta(arguments_delta).as_ref(),
                    None,
                );
                tracing::debug!(
                    tool_call_id = %tool_call_id,
                    name = %tool_name,
                    arguments_delta = %arguments_delta,
                    "tool_call_chunk (first) session update"
                );
                SessionUpdate::ToolCall(tc)
            } else {
                // Subsequent chunks: ACP doesn't support incremental updates yet.
                // The complete ToolCall event will be sent after streaming finishes.
                tracing::trace!(
                    tool_call_id = %tool_call_id,
                    arguments_delta_len = arguments_delta.len(),
                    "ignoring tool_call_chunk (subsequent) - ACP doesn't support incremental updates"
                );
                return None;
            }
        }
        StreamUpdate::SessionInfoUpdate { title } => {
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title.clone()))
        }
    };
    Some(SessionNotification::new(session_id.clone(), update))
}

fn parse_arguments_delta(delta: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(delta).ok()
}

fn parse_text_output_to_raw_value(output: &str) -> serde_json::Value {
    serde_json::json!(output)
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
}

impl SessionNotifier {
    pub fn new(tx: mpsc::Sender<SessionNotification>, session_id: SessionId) -> Self {
        Self { tx, session_id }
    }

    pub async fn send_event(&self, event: &AnyStreamEvent) {
        let updates = loom_event_to_updates(event);
        for u in &updates {
            if let Some(notif) = stream_update_to_session_notification(&self.session_id, u) {
                if let Err(e) = self.tx.send(notif).await {
                    tracing::error!(session_id = %self.session_id, error = %e, "Failed to send stream event notification");
                }
            }
        }
    }

    pub fn try_send_event(&self, event: &AnyStreamEvent) {
        let updates = loom_event_to_updates(event);
        for u in &updates {
            if let Some(notif) = stream_update_to_session_notification(&self.session_id, u) {
                let _ = self.tx.try_send(notif);
            }
        }
    }

    pub async fn send_history(&self, messages: &[Message]) {
        let mut tool_calls_map: HashMap<String, (String, Option<Value>)> = HashMap::new();

        for message in messages {
            let notifications = match message {
                Message::User(content) => vec![SessionNotification::new(
                    self.session_id.clone(),
                    SessionUpdate::UserMessageChunk(ContentChunk::new(
                        agent_client_protocol::ContentBlock::Text(
                            agent_client_protocol::TextContent::new(content.as_text().to_string()),
                        ),
                    )),
                )],
                Message::Assistant(payload) => {
                    for tc in &payload.tool_calls {
                        tool_calls_map.insert(
                            tc.id.clone(),
                            (tc.name.clone(), serde_json::from_str(&tc.arguments).ok()),
                        );
                    }

                    let mut notifs = vec![SessionNotification::new(
                        self.session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                            payload.content.clone().into(),
                        )),
                    )];

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
                        loom::tool_source::ToolCallContent::Text(t) => {
                            agent_client_protocol::ToolCallContent::from(
                                agent_client_protocol::ContentBlock::Text(
                                    agent_client_protocol::TextContent::new(t.clone()),
                                ),
                            )
                        }
                        loom::tool_source::ToolCallContent::Diff {
                            path,
                            old_text,
                            new_text,
                        } => agent_client_protocol::ToolCallContent::Diff(
                            agent_client_protocol::Diff::new(path.clone(), new_text.clone())
                                .old_text(old_text.clone()),
                        ),
                        loom::tool_source::ToolCallContent::Terminal { terminal_id } => {
                            agent_client_protocol::ToolCallContent::Terminal(Terminal::new(
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
                Message::System(_) => continue,
            };

            for notif in notifications {
                if let Err(e) = self.tx.send(notif).await {
                    tracing::error!(session_id = %self.session_id, error = %e, "Failed to send session update during history replay");
                }
            }
        }
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

fn tool_call_content_to_raw_output(content: &loom::tool_source::ToolCallContent) -> Value {
    match content {
        loom::tool_source::ToolCallContent::Text(text) => serde_json::json!(text),
        loom::tool_source::ToolCallContent::Diff {
            path,
            old_text,
            new_text,
        } => serde_json::json!({
            "type": "diff",
            "path": path,
            "oldText": old_text,
            "newText": new_text,
        }),
        loom::tool_source::ToolCallContent::Terminal { terminal_id } => serde_json::json!({
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
                ToolKind::Execute | ToolKind::Other | _ => unreachable!(),
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
                    .filter_map(|agent| agent.get("agent").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    .collect();
                if !agent_names.is_empty() {
                    return Some(format!("{} agent(s): {}", agent_names.len(), agent_names.join(", ")));
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
                    truncate_path(val, 60)
                };
                return Some(display);
            }
        }
    }
    None
}

fn truncate_path(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let start = s.len() - max_len + 3;
        format!("...{}", &s[start..])
    }
}
