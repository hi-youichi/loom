use std::collections::HashMap;
use std::sync::Arc;

use stream_event::codex::{
    CodexEvent, agent_message_item, command_execution_item, mcp_tool_call_item,
    McpToolCallItemError, reasoning_item,
};
use stream_event::{MessageChunkKind, StreamEvent};

use crate::approval::ApprovalManager;

/// Tracks sequential item IDs and the current streaming message item.
pub struct ItemTracker {
    counter: usize,
    current_message_id: Option<String>,
    /// Maps tool call_id → item_id so ToolStart/ToolOutput/ToolEnd can reference the same item.
    tool_item_ids: HashMap<String, String>,
    /// Accumulated output per tool item, keyed by item_id.
    tool_output: HashMap<String, String>,
    /// Command string per tool item (needed when emitting ToolEnd).
    tool_command: HashMap<String, String>,
    /// MCP tool metadata: item_id → (server, tool_name, arguments).
    mcp_meta: HashMap<String, (String, String, serde_json::Value)>,
}

impl ItemTracker {
    pub fn new() -> Self {
        Self {
            counter: 0,
            current_message_id: None,
            tool_item_ids: HashMap::new(),
            tool_output: HashMap::new(),
            tool_command: HashMap::new(),
            mcp_meta: HashMap::new(),
        }
    }

    pub fn next_id(&mut self) -> String {
        self.counter += 1;
        format!("item-{}", self.counter)
    }
}

/// Returns true if the tool name represents a local shell/bash tool.
fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "bash" | "shell" | "run_command" | "powershell" | "execute_command"
    )
}

/// Parse an MCP tool name of the form "server__tool" into (server, tool).
/// Returns None if the name does not contain "__".
fn parse_mcp_tool(name: &str) -> Option<(&str, &str)> {
    name.find("__").map(|pos| (&name[..pos], &name[pos + 2..]))
}

/// Convert a single `StreamEvent<S>` (type-erased via a closure over its fields)
/// into zero or more `CodexEvent`s, updating the tracker in place.
///
/// We accept the concrete fields we care about rather than the generic `StreamEvent<S>`
/// so that we can call this from the four `AnyStreamEvent` arms without monomorphizing
/// the entire function body four times.
fn convert_stream_event_inner(
    ev_kind: StreamEventKind,
    tracker: &mut ItemTracker,
    _approval: &Arc<ApprovalManager>,
) -> Vec<CodexEvent> {
    match ev_kind {
        // ── Text / Thinking message chunks ──────────────────────────────────

        StreamEventKind::Messages { content, kind } => {
            let mut out = Vec::new();
            match kind {
                MessageChunkKind::Thinking => {
                    // Reasoning: always emit as a single completed item.
                    // We do not accumulate thinking chunks across events; each arrives complete.
                    let id = tracker.next_id();
                    out.push(CodexEvent::ItemStarted {
                        item: reasoning_item(&id, &content),
                    });
                    out.push(CodexEvent::ItemCompleted {
                        item: reasoning_item(&id, &content),
                    });
                }
                MessageChunkKind::Message => {
                    if let Some(ref id) = tracker.current_message_id.clone() {
                        // Continue the open message item with a delta.
                        out.push(CodexEvent::ItemUpdated {
                            item: agent_message_item(id, &content),
                        });
                    } else {
                        // Open a new message item.
                        let id = tracker.next_id();
                        tracker.current_message_id = Some(id.clone());
                        out.push(CodexEvent::ItemStarted {
                            item: agent_message_item(&id, &content),
                        });
                    }
                }
            }
            out
        }

        // ── Tool decided by LLM (Think node emits complete arguments) ───────

        StreamEventKind::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            // A ToolCall event means the LLM has finished deciding on a tool invocation.
            // Close any open streaming message first.
            let mut out = close_message(tracker);

            let item_id = tracker.next_id();
            let key = call_id.unwrap_or_else(|| item_id.clone());

            tracker.tool_item_ids.insert(key.clone(), item_id.clone());

            if is_shell_tool(&name) {
                // Render the arguments as a command string.
                let command = extract_command_string(&name, &arguments);
                tracker.tool_command.insert(item_id.clone(), command.clone());
                tracker.tool_output.insert(item_id.clone(), String::new());
                out.push(CodexEvent::ItemStarted {
                    item: command_execution_item(&item_id, &command, "", None, "pending"),
                });
            } else if let Some((server, tool)) = parse_mcp_tool(&name) {
                tracker.mcp_meta.insert(
                    item_id.clone(),
                    (server.to_string(), tool.to_string(), arguments.clone()),
                );
                out.push(CodexEvent::ItemStarted {
                    item: mcp_tool_call_item(
                        &item_id,
                        server,
                        tool,
                        arguments,
                        None,
                        None,
                        "pending",
                    ),
                });
            } else {
                // Unknown tool — treat as a generic command_execution.
                let command = format!("{name}({})", arguments);
                tracker.tool_command.insert(item_id.clone(), command.clone());
                tracker.tool_output.insert(item_id.clone(), String::new());
                out.push(CodexEvent::ItemStarted {
                    item: command_execution_item(&item_id, &command, "", None, "pending"),
                });
            }

            out
        }

        // ── Tool execution started (Act node) ────────────────────────────────

        StreamEventKind::ToolStart { call_id, name } => {
            let key = call_id.as_deref().unwrap_or(&name);
            if let Some(item_id) = tracker.tool_item_ids.get(key).cloned() {
                if let Some((server, tool, args)) = tracker.mcp_meta.get(&item_id).cloned() {
                    return vec![CodexEvent::ItemUpdated {
                        item: mcp_tool_call_item(
                            &item_id, &server, &tool, args, None, None, "running",
                        ),
                    }];
                }
                if let Some(command) = tracker.tool_command.get(&item_id).cloned() {
                    let output = tracker
                        .tool_output
                        .get(&item_id)
                        .cloned()
                        .unwrap_or_default();
                    return vec![CodexEvent::ItemUpdated {
                        item: command_execution_item(
                            &item_id, &command, &output, None, "running",
                        ),
                    }];
                }
            }
            vec![]
        }

        // ── Tool incremental output (Act node) ───────────────────────────────

        StreamEventKind::ToolOutput {
            call_id,
            name,
            content,
        } => {
            let key = call_id.as_deref().unwrap_or(&name);
            if let Some(item_id) = tracker.tool_item_ids.get(key).cloned() {
                if tracker.mcp_meta.contains_key(&item_id) {
                    // MCP tools don't stream output — ignore.
                    return vec![];
                }
                if let Some(command) = tracker.tool_command.get(&item_id).cloned() {
                    let output = tracker.tool_output.entry(item_id.clone()).or_default();
                    output.push_str(&content);
                    let output_snapshot = output.clone();
                    return vec![CodexEvent::ItemUpdated {
                        item: command_execution_item(
                            &item_id,
                            &command,
                            &output_snapshot,
                            None,
                            "running",
                        ),
                    }];
                }
            }
            vec![]
        }

        // ── Tool execution finished (Act node) ───────────────────────────────

        StreamEventKind::ToolEnd {
            call_id,
            name,
            result,
            is_error,
            raw_result,
        } => {
            let key = call_id.as_deref().unwrap_or(&name);
            if let Some(item_id) = tracker.tool_item_ids.get(key).cloned() {
                if let Some((server, tool, args)) = tracker.mcp_meta.get(&item_id).cloned() {
                    let (res_val, err_val) = if is_error {
                        (
                            None,
                            Some(McpToolCallItemError {
                                message: result.clone(),
                            }),
                        )
                    } else {
                        let parsed = raw_result
                            .as_deref()
                            .and_then(|r| serde_json::from_str(r).ok())
                            .unwrap_or_else(|| serde_json::Value::String(result.clone()));
                        (Some(parsed), None)
                    };
                    let status = if is_error { "failed" } else { "completed" };
                    return vec![CodexEvent::ItemCompleted {
                        item: mcp_tool_call_item(
                            &item_id, &server, &tool, args, res_val, err_val, status,
                        ),
                    }];
                }

                if let Some(command) = tracker.tool_command.get(&item_id).cloned() {
                    let output = tracker
                        .tool_output
                        .get(&item_id)
                        .cloned()
                        .unwrap_or_default();
                    // Parse exit code from result if the tool embeds it, otherwise use 0/1.
                    let exit_code: Option<i32> = raw_result
                        .as_deref()
                        .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
                        .and_then(|v| v["exit_code"].as_i64())
                        .map(|c| c as i32)
                        .or(Some(if is_error { 1 } else { 0 }));

                    let status = if is_error { "failed" } else { "completed" };
                    return vec![CodexEvent::ItemCompleted {
                        item: command_execution_item(
                            &item_id,
                            &command,
                            &output,
                            exit_code,
                            status,
                        ),
                    }];
                }
            }
            vec![]
        }

        // ── All other event kinds are not surfaced as CodexEvents ────────────
        StreamEventKind::Other => vec![],
    }
}

/// Close any open streaming message item and return its ItemCompleted event (if any).
fn close_message(tracker: &mut ItemTracker) -> Vec<CodexEvent> {
    if let Some(id) = tracker.current_message_id.take() {
        // We do not have the accumulated text here; the protocol expects the
        // final text to be provided. We emit an empty-text ItemCompleted so the
        // client knows the stream is done. Real text was sent via ItemUpdated.
        vec![CodexEvent::ItemCompleted {
            item: agent_message_item(&id, ""),
        }]
    } else {
        vec![]
    }
}

/// A type-erased summary of the `StreamEvent<S>` fields we care about,
/// extracted before the generic `S` state is dropped.
enum StreamEventKind {
    Messages {
        content: String,
        kind: MessageChunkKind,
    },
    ToolCall {
        call_id: Option<String>,
        name: String,
        arguments: serde_json::Value,
    },
    ToolStart {
        call_id: Option<String>,
        name: String,
    },
    ToolOutput {
        call_id: Option<String>,
        name: String,
        content: String,
    },
    ToolEnd {
        call_id: Option<String>,
        name: String,
        result: String,
        is_error: bool,
        raw_result: Option<String>,
    },
    Other,
}

fn classify_stream_event<S>(ev: &StreamEvent<S>) -> StreamEventKind
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    match ev {
        StreamEvent::Messages { chunk, .. } => StreamEventKind::Messages {
            content: chunk.content.clone(),
            kind: chunk.kind,
        },
        StreamEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => StreamEventKind::ToolCall {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
        },
        StreamEvent::ToolStart { call_id, name } => StreamEventKind::ToolStart {
            call_id: call_id.clone(),
            name: name.clone(),
        },
        StreamEvent::ToolOutput {
            call_id,
            name,
            content,
        } => StreamEventKind::ToolOutput {
            call_id: call_id.clone(),
            name: name.clone(),
            content: content.clone(),
        },
        StreamEvent::ToolEnd {
            call_id,
            name,
            result,
            is_error,
            raw_result,
        } => StreamEventKind::ToolEnd {
            call_id: call_id.clone(),
            name: name.clone(),
            result: result.clone(),
            is_error: *is_error,
            raw_result: raw_result.clone(),
        },
        _ => StreamEventKind::Other,
    }
}

/// Extract a human-readable command string from a shell tool call's arguments.
fn extract_command_string(name: &str, arguments: &serde_json::Value) -> String {
    // Most shell tools pass {"command": "..."} or {"cmd": "..."}
    if let Some(s) = arguments.get("command").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(s) = arguments.get("cmd").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    // Fallback: render the whole arguments value
    format!("{name}({})", arguments)
}

/// Public entry point: convert one `AnyStreamEvent` into zero or more `CodexEvent`s.
pub fn codex_events_from_stream_event(
    ev: &agent::run::TypedAnyStreamEvent,
    tracker: &mut ItemTracker,
    approval: &Arc<ApprovalManager>,
) -> Vec<CodexEvent> {
    let kind = match ev {
        agent::run::TypedAnyStreamEvent::React(inner) => classify_stream_event(inner),
        agent::run::TypedAnyStreamEvent::Dup(inner) => classify_stream_event(inner),
        agent::run::TypedAnyStreamEvent::Tot(inner) => classify_stream_event(inner),
        agent::run::TypedAnyStreamEvent::Got(inner) => classify_stream_event(inner),
    };
    convert_stream_event_inner(kind, tracker, approval)
}
