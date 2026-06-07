use std::collections::HashMap;

use loom_llm::LlmUsage;
use loom_llm::message::{AssistantPayload, Message};
use loom_types::state::ReActState;
use loom_tools::ToolCallContent;
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

fn split_turns(messages: &[Message]) -> Vec<Vec<&Message>> {
    let mut turns: Vec<Vec<&Message>> = Vec::new();
    for msg in messages {
        if matches!(msg, Message::Assistant(_)) {
            turns.push(Vec::new());
        }
        if let Some(last) = turns.last_mut() {
            last.push(msg);
        }
    }
    turns
}

fn emit_assistant_items(
    payload: &AssistantPayload,
    tool_result_map: &HashMap<String, (String, bool)>,
    item_id: &mut ItemIdCounter,
    events: &mut Vec<CodexEvent>,
) {
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

        if is_shell_tool(&tc.name) {
            let cmd = extract_command(&args);
            let item = command_execution_item(&id, &cmd, "", None, "in_progress");
            events.push(CodexEvent::ItemStarted {
                item: item.clone(),
            });

            let (output, exit_code, status) =
                resolve_tool_result_from_map(tool_result_map, &tc.id);
            let completed = command_execution_item(&id, &cmd, &output, exit_code, &status);
            events.push(CodexEvent::ItemCompleted { item: completed });
        } else {
            let (server, tool_name) = split_server_tool(&tc.name);
            let item = mcp_tool_call_item(
                &id, &server, &tool_name, args, None, None, "in_progress",
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
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({})),
                result,
                error,
                &status,
            );
            events.push(CodexEvent::ItemCompleted { item: completed });
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

        let turns = split_turns(new_messages);
        let num_turns = turns.len();

        for (turn_idx, turn_msgs) in turns.iter().enumerate() {
            events.push(CodexEvent::TurnStarted);

            for msg in turn_msgs {
                if let Message::Assistant(payload) = msg {
                    emit_assistant_items(payload, &tool_result_map, &mut item_id, &mut events);
                }
            }

            let is_last_turn = turn_idx == num_turns - 1;
            let usage = if is_last_turn {
                if let Some(ref u) = state.usage {
                    to_codex_usage(u)
                } else if let Some(ref total) = state.total_usage {
                    to_codex_usage(total)
                } else {
                    CodexUsage::zero()
                }
            } else {
                CodexUsage::zero()
            };

            events.push(CodexEvent::TurnCompleted { usage });
        }

        prev_msg_count = cur_msg_count;
    }

    events
}

fn build_tool_result_map_from_refs(
    messages: &[&Message],
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
#[cfg(test)]
mod tests {
    use loom_llm::LlmUsage;
    use loom_llm::message::{AssistantPayload, AssistantToolCall, Message};
    use loom_types::state::ReActState;
    use loom_tools::ToolCallContent;
    use stream_event::codex::{CodexEvent, CodexUsage};

    use super::build_codex_events;

    fn make_usage(prompt: u32, completion: u32) -> LlmUsage {
        LlmUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        }
    }

    fn assistant_reply(content: &str) -> Message {
        Message::Assistant(AssistantPayload {
            content: content.to_string(),
            tool_calls: vec![],
            reasoning_content: None,
        })
    }

    fn assistant_with_tools(
        reasoning: Option<&str>,
        content: &str,
        tools: Vec<(&str, &str, &str)>,
    ) -> Message {
        Message::Assistant(AssistantPayload {
            content: content.to_string(),
            tool_calls: tools
                .into_iter()
                .map(|(id, name, args)| AssistantToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: args.to_string(),
                })
                .collect(),
            reasoning_content: reasoning.map(|s| s.to_string()),
        })
    }

    fn tool_result(call_id: &str, text: &str) -> Message {
        Message::Tool {
            tool_call_id: call_id.to_string(),
            content: ToolCallContent::Text(text.to_string()),
        }
    }

    fn make_checkpoint(
        messages: Vec<Message>,
        usage: Option<LlmUsage>,
        total_usage: Option<LlmUsage>,
    ) -> ReActState {
        ReActState {
            messages,
            usage,
            total_usage,
            ..Default::default()
        }
    }

    fn count_events(events: &[CodexEvent], f: fn(&CodexEvent) -> bool) -> usize {
        events.iter().filter(|e| f(e)).count()
    }

    fn is_turn_started(e: &CodexEvent) -> bool {
        matches!(e, CodexEvent::TurnStarted)
    }

    fn is_turn_completed(e: &CodexEvent) -> bool {
        matches!(e, CodexEvent::TurnCompleted { .. })
    }

    fn get_turn_completed_usages(events: &[CodexEvent]) -> Vec<&CodexUsage> {
        events
            .iter()
            .filter_map(|e| match e {
                CodexEvent::TurnCompleted { usage } => Some(usage),
                _ => None,
            })
            .collect()
    }

    fn get_item_ids(events: &[CodexEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                CodexEvent::ItemStarted { item } | CodexEvent::ItemCompleted { item } => {
                    item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                }
                _ => None,
            })
            .collect()
    }

    fn get_item_types(events: &[CodexEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                CodexEvent::ItemCompleted { item } => {
                    item.get("type").and_then(|v| v.as_str()).map(|s| s.to_string())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn single_assistant_no_tools() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("hello"),
                assistant_reply("hi there"),
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp]);

        assert_eq!(count_events(&events, is_turn_started), 1);
        assert_eq!(count_events(&events, is_turn_completed), 1);

        let types = get_item_types(&events);
        assert_eq!(types, vec!["agent_message"]);

        let usages = get_turn_completed_usages(&events);
        assert_eq!(usages.len(), 1);
        assert_eq!(*usages[0], CodexUsage::zero());
    }

    #[test]
    fn single_assistant_with_tools() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("list files"),
                assistant_with_tools(
                    None,
                    "",
                    vec![("tc_1", "bash", r#"{"command":"ls"}"#)],
                ),
                tool_result("tc_1", "file1.rs\nfile2.rs"),
                assistant_reply("Here are the files."),
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp]);

        assert_eq!(count_events(&events, is_turn_started), 2);
        assert_eq!(count_events(&events, is_turn_completed), 2);

        let types = get_item_types(&events);
        assert_eq!(types[0], "command_execution");
        assert_eq!(types[1], "agent_message");
    }

    #[test]
    fn multi_assistant_in_one_checkpoint() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("do stuff"),
                assistant_with_tools(
                    Some("thinking step 1"),
                    "",
                    vec![
                        ("tc_1", "bash", r#"{"command":"echo a"}"#),
                        ("tc_2", "mcp/read_file", r#"{"path":"x.rs"}"#),
                    ],
                ),
                tool_result("tc_1", "a"),
                tool_result("tc_2", "file content"),
                assistant_with_tools(
                    None,
                    "",
                    vec![("tc_3", "mcp/write_file", r#"{"path":"y.rs"}"#)],
                ),
                tool_result("tc_3", "ok"),
                assistant_reply("All done."),
            ],
            Some(make_usage(100, 50)),
            None,
        );
        let events = build_codex_events("s1", &[cp]);

        assert_eq!(count_events(&events, is_turn_started), 3);
        assert_eq!(count_events(&events, is_turn_completed), 3);

        let types = get_item_types(&events);
        assert_eq!(types[0], "reasoning");
        assert_eq!(types[1], "command_execution");
        assert_eq!(types[2], "mcp_tool_call");
        assert_eq!(types[3], "mcp_tool_call");
        assert_eq!(types[4], "agent_message");

        let usages = get_turn_completed_usages(&events);
        assert_eq!(usages[0], &CodexUsage::zero());
        assert_eq!(usages[1], &CodexUsage::zero());
        assert_eq!(
            usages[2],
            &CodexUsage {
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 50,
                reasoning_output_tokens: 0,
            }
        );
    }

    #[test]
    fn multi_checkpoint_delta() {
        let cp1 = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("start"),
                assistant_with_tools(
                    None,
                    "",
                    vec![("tc_1", "bash", r#"{"command":"ls"}"#)],
                ),
                tool_result("tc_1", "output"),
            ],
            None,
            None,
        );
        let cp2 = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("start"),
                Message::Assistant(AssistantPayload {
                    content: String::new(),
                    tool_calls: vec![AssistantToolCall {
                        id: "tc_1".into(),
                        name: "bash".into(),
                        arguments: r#"{"command":"ls"}"#.into(),
                    }],
                    reasoning_content: None,
                }),
                Message::Tool {
                    tool_call_id: "tc_1".into(),
                    content: ToolCallContent::Text("output".into()),
                },
                assistant_with_tools(
                    None,
                    "",
                    vec![("tc_2", "bash", r#"{"command":"pwd"}"#)],
                ),
                tool_result("tc_2", "/home"),
                assistant_reply("done"),
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp1, cp2]);

        assert_eq!(count_events(&events, is_turn_started), 3);
        assert_eq!(count_events(&events, is_turn_completed), 3);
    }

    #[test]
    fn checkpoint_no_assistant_skipped() {
        let cp1 = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("hi"),
                assistant_reply("hello"),
            ],
            None,
            None,
        );
        let cp2 = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("hi"),
                Message::Assistant(AssistantPayload {
                    content: "hello".into(),
                    tool_calls: vec![],
                    reasoning_content: None,
                }),
                Message::Tool {
                    tool_call_id: "x".into(),
                    content: ToolCallContent::Text("orphan result".into()),
                },
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp1, cp2]);

        assert_eq!(count_events(&events, is_turn_started), 1);
        assert_eq!(count_events(&events, is_turn_completed), 1);
    }

    #[test]
    fn empty_checkpoints() {
        let events = build_codex_events("s1", &[]);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], CodexEvent::ThreadStarted { .. }));
        assert_eq!(count_events(&events, is_turn_started), 0);
    }

    #[test]
    fn usage_per_turn_with_state_usage() {
        let cp1 = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("a"),
                assistant_reply("reply a"),
            ],
            Some(make_usage(10, 5)),
            None,
        );
        let cp2 = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("a"),
                Message::Assistant(AssistantPayload {
                    content: "reply a".into(),
                    tool_calls: vec![],
                    reasoning_content: None,
                }),
                Message::user("b"),
                assistant_reply("reply b"),
            ],
            Some(make_usage(20, 10)),
            None,
        );
        let events = build_codex_events("s1", &[cp1, cp2]);
        let usages = get_turn_completed_usages(&events);
        assert_eq!(usages.len(), 2);
        assert_eq!(usages[0].input_tokens, 10);
        assert_eq!(usages[0].output_tokens, 5);
        assert_eq!(usages[1].input_tokens, 20);
        assert_eq!(usages[1].output_tokens, 10);
    }

    #[test]
    fn usage_delta_from_total_usage() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("go"),
                assistant_reply("ok"),
            ],
            None,
            Some(make_usage(500, 200)),
        );
        let events = build_codex_events("s1", &[cp]);
        let usages = get_turn_completed_usages(&events);
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].input_tokens, 500);
        assert_eq!(usages[0].output_tokens, 200);
    }

    #[test]
    fn usage_zero_when_no_usage() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("go"),
                assistant_reply("ok"),
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp]);
        let usages = get_turn_completed_usages(&events);
        assert_eq!(usages[0], &CodexUsage::zero());
    }

    #[test]
    fn item_id_sequential() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("go"),
                assistant_with_tools(
                    Some("think"),
                    "",
                    vec![("tc_1", "bash", r#"{"command":"ls"}"#)],
                ),
                tool_result("tc_1", "out"),
                assistant_reply("done"),
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp]);
        let ids = get_item_ids(&events);
        let unique_ids: Vec<&String> = ids.iter().collect::<std::collections::HashSet<_>>().into_iter().collect();
        assert!(unique_ids.len() >= 3);

        let completed_ids: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                CodexEvent::ItemCompleted { item } => {
                    item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(completed_ids, vec!["item_0", "item_1", "item_2"]);
    }

    #[test]
    fn tool_result_filled_in_items() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("run"),
                assistant_with_tools(
                    None,
                    "",
                    vec![
                        ("tc_1", "bash", r#"{"command":"true"}"#),
                        ("tc_2", "mcp/tool", r#"{}"#),
                    ],
                ),
                tool_result("tc_1", "success output"),
                Message::Tool {
                    tool_call_id: "tc_2".into(),
                    content: ToolCallContent::Text("error:something failed".into()),
                },
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp]);

        let items: Vec<&serde_json::Value> = events
            .iter()
            .filter_map(|e| match e {
                CodexEvent::ItemCompleted { item } => Some(item),
                _ => None,
            })
            .collect();

        let cmd_item = &items[0];
        assert_eq!(cmd_item.get("type").unwrap().as_str().unwrap(), "command_execution");
        assert_eq!(
            cmd_item.get("aggregated_output").unwrap().as_str().unwrap(),
            "success output"
        );
        assert_eq!(cmd_item.get("exit_code").unwrap().as_i64().unwrap(), 0);

        let mcp_item = &items[1];
        assert_eq!(mcp_item.get("type").unwrap().as_str().unwrap(), "mcp_tool_call");
        assert_eq!(mcp_item.get("status").unwrap().as_str().unwrap(), "failed");
    }

    #[test]
    fn user_message_not_splitting() {
        let cp = make_checkpoint(
            vec![
                Message::system("sys"),
                Message::user("prompt1"),
                assistant_reply("reply1"),
                Message::user("prompt2"),
                assistant_reply("reply2"),
            ],
            None,
            None,
        );
        let events = build_codex_events("s1", &[cp]);

        assert_eq!(count_events(&events, is_turn_started), 2);
        assert_eq!(count_events(&events, is_turn_completed), 2);

        let types = get_item_types(&events);
        assert_eq!(types, vec!["agent_message", "agent_message"]);
    }
}
