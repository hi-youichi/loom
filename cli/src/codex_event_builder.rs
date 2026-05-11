use std::collections::HashMap;

use loom::llm::LlmUsage;
use loom::message::Message;
use loom::tool_source::ToolCallContent;
use loom::state::ReActState;
use stream_event::codex::{
    agent_message_item, command_execution_item, mcp_tool_call_item, reasoning_item,
    CodexEvent, CodexUsage, McpToolCallItemError,
};

struct ItemIdCounter(usize);

impl ItemIdCounter {
    fn new() -> Self {
        Self(0)
    }

    fn next(&mut self) -> String {
        let id = format!("item_{}", self.0);
        self.0 += 1;
        id
    }
}

fn tool_call_display_text(content: &ToolCallContent) -> String {
    match content {
        ToolCallContent::Text(t) => t.clone(),
        ToolCallContent::Diff {
            path,
            old_text: _,
            new_text,
        } => format!("diff: {path}\n{new_text}"),
        ToolCallContent::Terminal { terminal_id } => format!("terminal: {terminal_id}"),
    }
}

pub fn build_codex_events(session_id: &str, checkpoints: &[ReActState]) -> Vec<CodexEvent> {
    let mut events = Vec::new();
    let mut item_id = ItemIdCounter::new();

    events.push(CodexEvent::ThreadStarted {
        thread_id: session_id.to_string(),
    });

    if checkpoints.is_empty() {
        return events;
    }

    let all_messages: Vec<&Message> = checkpoints
        .iter()
        .flat_map(|s| s.messages.iter())
        .collect();

    let tool_result_map = build_tool_result_map_from_refs(&all_messages);

    let mut prev_msg_count: usize = 0;

    for (i, state) in checkpoints.iter().enumerate() {
        let cur_msg_count = count_non_system_messages(&state.messages);
        let delta = cur_msg_count.saturating_sub(prev_msg_count);

        if delta == 0 && i > 0 {
            continue;
        }

        let new_messages = &state.messages[state.messages.len() - delta..];

        let has_assistant = new_messages
            .iter()
            .any(|m| matches!(m, Message::Assistant(_)));

        if !has_assistant {
            prev_msg_count = cur_msg_count;
            continue;
        }

        events.push(CodexEvent::TurnStarted);

        let mut usage_emitted = false;
        for msg in new_messages {
            match msg {
                Message::Assistant(payload) => {
                    if let Some(ref reasoning) = payload.reasoning_content {
                        if !reasoning.is_empty() {
                            let id = item_id.next();
                            let item = reasoning_item(&id, reasoning);
                            events.push(CodexEvent::ItemStarted {
                                item: item.clone(),
                            });
                            events.push(CodexEvent::ItemCompleted { item });
                        }
                    }

                    for tc in &payload.tool_calls {
                        let id = item_id.next();
                        let args: serde_json::Value =
                            serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

                        let is_command = is_shell_tool(&tc.name);

                        if is_command {
                            let cmd = extract_command(&args);
                            let item = command_execution_item(
                                &id,
                                &cmd,
                                "",
                                None,
                                "in_progress",
                            );
                            events.push(CodexEvent::ItemStarted {
                                item: item.clone(),
                            });

                            let (output, exit_code, status) =
                                resolve_tool_result_from_map(&tool_result_map, &tc.id);
                            let completed = command_execution_item(
                                &id, &cmd, &output, exit_code, &status,
                            );
                            events.push(CodexEvent::ItemCompleted {
                                item: completed,
                            });
                        } else {
                            let (server, tool_name) = split_server_tool(&tc.name);
                            let item = mcp_tool_call_item(
                                &id,
                                &server,
                                &tool_name,
                                args,
                                None,
                                None,
                                "in_progress",
                            );
                            events.push(CodexEvent::ItemStarted {
                                item: item.clone(),
                            });

                            let (output_text, is_error) = tool_result_map
                                .get(&tc.id)
                                .cloned()
                                .unwrap_or_default();

                            let (result, error, status) = if is_error {
                                (
                                    None,
                                    Some(McpToolCallItemError {
                                        message: output_text,
                                    }),
                                    "failed".to_string(),
                                )
                            } else if output_text.is_empty() {
                                (None, None, "completed".to_string())
                            } else {
                                let content =
                                    serde_json::json!([{ "type": "text", "text": output_text }]);
                                (
                                    Some(serde_json::json!({
                                        "content": content,
                                        "structured_content": null
                                    })),
                                    None,
                                    "completed".to_string(),
                                )
                            };

                            let completed = mcp_tool_call_item(
                                &id,
                                &server,
                                &tool_name,
                                serde_json::from_str(&tc.arguments)
                                    .unwrap_or(serde_json::json!({})),
                                result,
                                error,
                                &status,
                            );
                            events.push(CodexEvent::ItemCompleted {
                                item: completed,
                            });
                        }
                    }

                    if !payload.content.is_empty() {
                        let id = item_id.next();
                        let item = agent_message_item(&id, &payload.content);
                        events.push(CodexEvent::ItemStarted {
                            item: item.clone(),
                        });
                        events.push(CodexEvent::ItemCompleted { item });
                    }
                }
                Message::Tool { .. } => {}
                _ => {}
            }
        }

        if let Some(ref usage) = state.usage {
            events.push(CodexEvent::TurnCompleted {
                usage: to_codex_usage(usage),
            });
            usage_emitted = true;
        }

        if !usage_emitted {
            events.push(CodexEvent::TurnCompleted {
                usage: CodexUsage {
                    input_tokens: 0,
                    cached_input_tokens: 0,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                },
            });
        }

        prev_msg_count = cur_msg_count;
    }

    events
}

fn build_tool_result_map_from_refs<'a>(
    messages: &[&'a Message],
) -> HashMap<String, (String, bool)> {
    let mut map = HashMap::new();
    for msg in messages {
        if let Message::Tool {
            tool_call_id,
            content,
        } = msg
        {
            let text = tool_call_display_text(content);
            let is_error = match content {
                ToolCallContent::Text(t) => t.starts_with("error:") || t.contains("\"error\""),
                _ => false,
            };
            map.insert(tool_call_id.clone(), (text, is_error));
        }
    }
    map
}

fn count_non_system_messages(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| !matches!(m, Message::System(_)))
        .count()
}

fn is_shell_tool(name: &str) -> bool {
    matches!(name, "bash" | "shell" | "command" | "exec" | "run_command")
}

fn extract_command(args: &serde_json::Value) -> String {
    args.get("command")
        .or_else(|| args.get("cmd"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn resolve_tool_result_from_map(
    map: &HashMap<String, (String, bool)>,
    call_id: &str,
) -> (String, Option<i32>, String) {
    match map.get(call_id) {
        Some((output, is_error)) => {
            if *is_error {
                (output.clone(), Some(1), "failed".to_string())
            } else {
                (output.clone(), Some(0), "completed".to_string())
            }
        }
        None => ("".to_string(), None, "completed".to_string()),
    }
}

fn split_server_tool(name: &str) -> (String, String) {
    match name.split_once('/') {
        Some((server, tool)) => (server.to_string(), tool.to_string()),
        None => ("loom".to_string(), name.to_string()),
    }
}

fn to_codex_usage(usage: &LlmUsage) -> CodexUsage {
    CodexUsage {
        input_tokens: usage.prompt_tokens,
        cached_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens)
            .unwrap_or(0),
        output_tokens: usage.completion_tokens,
        reasoning_output_tokens: usage
            .completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens)
            .unwrap_or(0),
    }
}

pub fn print_cat_text(events: &[CodexEvent]) {
    use std::io::Write;

    for event in events {
        match event {
            CodexEvent::ThreadStarted { thread_id } => {
                println!("Session: {thread_id}");
                println!("{}", "═".repeat(60));
            }
            CodexEvent::TurnStarted => {
                println!("\n{}", "─".repeat(40));
            }
            CodexEvent::TurnCompleted { usage } => {
                if usage.input_tokens > 0 || usage.output_tokens > 0 {
                    println!(
                        "  [usage] in:{} out:{} cached:{} reasoning:{}",
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.cached_input_tokens,
                        usage.reasoning_output_tokens
                    );
                }
            }
            CodexEvent::TurnFailed { error } => {
                println!("  [turn failed] {}", error.message);
            }
            CodexEvent::ItemStarted { item }
            | CodexEvent::ItemUpdated { item }
            | CodexEvent::ItemCompleted { item } => {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                match item_type {
                    "reasoning" => {
                        if matches!(event, CodexEvent::ItemStarted { .. }) {
                            let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            println!("  [think] {text}");
                        }
                    }
                    "agent_message" => {
                        if matches!(event, CodexEvent::ItemCompleted { .. }) {
                            let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            println!("  [reply] {text}");
                        }
                    }
                    "command_execution" => {
                        let cmd = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
                        let status =
                            item.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        if matches!(event, CodexEvent::ItemCompleted { .. }) {
                            let output = item
                                .get("aggregated_output")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let exit = item.get("exit_code").and_then(|v| v.as_i64());
                            println!("  [{id}] $ {cmd}  (exit={:?}, {status})", exit);
                            if !output.is_empty() {
                                for line in output.lines().take(5) {
                                    println!("    {line}");
                                }
                                let remaining = output.lines().count().saturating_sub(5);
                                if remaining > 0 {
                                    println!("    ... ({remaining} more lines)");
                                }
                            }
                        }
                    }
                    "mcp_tool_call" => {
                        let tool = item.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                        let server =
                            item.get("server").and_then(|v| v.as_str()).unwrap_or("");
                        if matches!(event, CodexEvent::ItemCompleted { .. }) {
                            let status =
                                item.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            println!("  [{id}] {server}/{tool} ({status})");
                        }
                    }
                    _ => {}
                }
            }
            CodexEvent::Error { message } => {
                println!("[error] {message}");
            }
        }
    }
    let _ = std::io::stdout().flush();
}
