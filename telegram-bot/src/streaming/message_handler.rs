//! Message handler for streaming responses
//!
//! Processes streaming commands from Loom agent and updates Telegram messages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::config::StreamingConfig;
use crate::traits::MessageSender;
use crate::utils::truncate_text;

/// Commands sent from event callback to message handler
#[derive(Debug, Clone)]
pub enum StreamCommand {
    StartThink { count: u32 },
    StartAct { count: u32 },
    ThinkContent { content: String },
    /// Model text during the Act phase (streamed chunks).
    ActContent { content: String },
    ToolStart {
        name: String,
        arguments: Option<String>,
    },
    ToolEnd {
        name: String,
        result: String,
        is_error: bool,
    },
    Flush,
}

/// State for tracking message content
pub struct MessageState {
    pub msg_id: Option<i32>,
    phase: String,
    think_text: String,
    tools: Vec<String>,
    act_text: String,
    tool_start_times: HashMap<String, Instant>,
    act_count: Option<u32>,
    last_sent_length: usize,
    last_update: Instant,
    settings: StreamingConfig,
    final_text: String,
}

impl MessageState {
    pub fn new(settings: StreamingConfig) -> Self {
        Self {
            msg_id: None,
            phase: String::new(),
            think_text: String::new(),
            tools: Vec::new(),
            act_text: String::new(),
            tool_start_times: HashMap::new(),
            act_count: None,
            last_sent_length: 0,
            last_update: Instant::now(),
            settings,
            final_text: String::new(),
        }
    }

    pub fn should_update(&self, min_interval_ms: u64) -> bool {
        self.last_update.elapsed() >= Duration::from_millis(min_interval_ms)
    }
}

fn act_body_for_edit(state: &MessageState) -> String {
    let tools_block = state.tools.join("\n");
    let act_trimmed = state.act_text.trim_end();
    if tools_block.is_empty() {
        act_trimmed.to_string()
    } else if act_trimmed.is_empty() {
        tools_block
    } else {
        format!("{}\n\n{}", tools_block, act_trimmed)
    }
}

fn format_tool_start_line(name: &str, arguments: Option<&str>) -> String {
    match arguments {
        Some(args) if !args.is_empty() => format!("🔧 {} {}", name, args),
        _ => format!("🔧 {}", name),
    }
}

fn is_inflight_tool_line(line: &str, name: &str) -> bool {
    line == format!("🔧 {}", name) || line.starts_with(&format!("🔧 {} ", name))
}

fn extract_inflight_tool_arguments(line: &str, name: &str) -> Option<String> {
    let prefix = format!("🔧 {} ", name);
    line.strip_prefix(&prefix).map(|rest| rest.to_string())
}

fn truncate_chars_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

async fn enter_act_phase_without_count_if_needed(
    sender: &Arc<dyn MessageSender>,
    chat_id: i64,
    state: &mut MessageState,
) {
    if state.phase == "act" {
        return;
    }
    if state.phase == "think" && state.think_text.len() > state.last_sent_length {
        let text = truncate_text(&state.think_text, state.settings.max_think_chars);
        if let Some(msg_id) = state.msg_id {
            let _ = sender.edit_message(chat_id, msg_id, &text).await;
        }
        state.final_text = state.think_text.clone();
    }

    state.phase = "act".to_string();
    state.tools.clear();
    state.act_text.clear();
    state.tool_start_times.clear();
    state.act_count = None;
    state.last_sent_length = 0;
    state.last_update = Instant::now();

    if state.settings.show_act_phase {
        let header = format!("{} Act\n\n", state.settings.act_emoji);
        match sender.send_text_returning_id(chat_id, &header).await {
            Ok(id) => {
                state.msg_id = Some(id);
            }
            Err(e) => {
                tracing::warn!("Failed to send fallback Act header: {}", e);
            }
        }
    }
}

async fn edit_act_message_if_possible(
    sender: &Arc<dyn MessageSender>,
    chat_id: i64,
    msg_id: Option<i32>,
    state: &MessageState,
) {
    let body = act_body_for_edit(state);
    let final_text = truncate_text(&body, state.settings.max_act_chars);
    if let Some(mid) = msg_id {
        let _ = sender.edit_message(chat_id, mid, &final_text).await;
    }
}

/// Processes streaming UI commands using [`MessageSender`] (mockable in tests).
pub async fn stream_message_handler(
    mut rx: mpsc::Receiver<StreamCommand>,
    sender: Arc<dyn MessageSender>,
    chat_id: i64,
    settings: StreamingConfig,
) -> String {
    let mut state = MessageState::new(settings);

    while let Some(cmd) = rx.recv().await {
        match cmd {
            StreamCommand::StartThink { count } => {
                if state.phase == "act" && (!state.tools.is_empty() || !state.act_text.trim().is_empty()) {
                    edit_act_message_if_possible(&sender, chat_id, state.msg_id, &state).await;
                }
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = sender.edit_message(chat_id, msg_id, &text).await;
                    }
                }

                state.phase = "think".to_string();
                state.think_text.clear();
                state.last_sent_length = 0;
                state.last_update = Instant::now();
                state.act_count = None;

                if state.settings.show_think_phase {
                    let header = format!("{} Think #{}\n\n", state.settings.think_emoji, count);
                    let header_len = header.len();
                    match sender.send_text_returning_id(chat_id, &header).await {
                        Ok(id) => {
                            state.msg_id = Some(id);
                            state.think_text = header;
                            state.last_sent_length = header_len;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to send Think header: {}", e);
                        }
                    }
                }
            }

            StreamCommand::StartAct { count } => {
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = sender.edit_message(chat_id, msg_id, &text).await;
                    }
                    state.final_text = state.think_text.clone();
                }
                let is_fallback_act = state.phase == "act" && state.act_count.is_none();
                let is_same_act_count = state.act_count == Some(count);
                let should_reset_for_new_act = state.phase != "act" || (!is_fallback_act && !is_same_act_count);

                if should_reset_for_new_act {
                    state.phase = "act".to_string();
                    state.tools.clear();
                    state.act_text.clear();
                    state.tool_start_times.clear();
                    state.last_sent_length = 0;
                    state.last_update = Instant::now();

                    if state.settings.show_act_phase {
                        let header = format!("{} Act #{}\n\n", state.settings.act_emoji, count);
                        match sender.send_text_returning_id(chat_id, &header).await {
                            Ok(id) => {
                                state.msg_id = Some(id);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to send Act header: {}", e);
                            }
                        }
                    }
                }
                state.act_count = Some(count);
            }

            StreamCommand::ThinkContent { content } => {
                if state.phase != "think" || !state.settings.show_think_phase {
                    continue;
                }

                state.think_text.push_str(&content);

                if !state.should_update(state.settings.throttle_ms) {
                    continue;
                }

                let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = sender.edit_message(chat_id, msg_id, &text).await;
                }
                state.last_update = Instant::now();
                state.last_sent_length = state.think_text.len();
            }

            StreamCommand::ActContent { content } => {
                if !state.settings.show_act_phase {
                    continue;
                }
                enter_act_phase_without_count_if_needed(&sender, chat_id, &mut state).await;

                state.act_text.push_str(&content);

                if !state.should_update(state.settings.throttle_ms) {
                    continue;
                }

                edit_act_message_if_possible(&sender, chat_id, state.msg_id, &state).await;
                state.last_update = Instant::now();
            }

            StreamCommand::ToolStart { name, arguments } => {
                if !state.settings.show_act_phase {
                    continue;
                }
                enter_act_phase_without_count_if_needed(&sender, chat_id, &mut state).await;

                state.tool_start_times.insert(name.clone(), Instant::now());
                state
                    .tools
                    .push(format_tool_start_line(&name, arguments.as_deref()));

                if !state.should_update(state.settings.throttle_ms) {
                    continue;
                }

                edit_act_message_if_possible(&sender, chat_id, state.msg_id, &state).await;
                state.last_update = Instant::now();
            }

            StreamCommand::ToolEnd {
                name,
                result,
                is_error,
            } => {
                if !state.settings.show_act_phase {
                    continue;
                }
                enter_act_phase_without_count_if_needed(&sender, chat_id, &mut state).await;

                let status = if is_error { "❌" } else { "✅" };
                let max_result_len = if state.settings.max_act_chars == 0 {
                    1000
                } else {
                    state
                        .settings
                        .max_act_chars
                        .saturating_sub(80)
                        .clamp(120, 2000)
                };
                let truncated_result = truncate_chars_with_ellipsis(&result, max_result_len);
                let display_result = truncated_result.replace('\r', "");

                let duration_str = state
                    .tool_start_times
                    .remove(&name)
                    .map(|start| {
                        let millis = start.elapsed().as_millis();
                        if millis < 1000 {
                            format!(" ({}ms)", millis)
                        } else {
                            format!(" ({:.1}s)", millis as f64 / 1000.0)
                        }
                    })
                    .unwrap_or_default();

                let existing_args = state
                    .tools
                    .iter()
                    .find(|line| is_inflight_tool_line(line, &name))
                    .and_then(|line| extract_inflight_tool_arguments(line, &name));
                let display_name = match existing_args {
                    Some(ref args) if !args.is_empty() => format!("{} {}", name, args),
                    _ => name.clone(),
                };
                let completed = format!("{} {}{}:\n{}", status, display_name, duration_str, display_result);

                if let Some(pos) = state
                    .tools
                    .iter()
                    .position(|t| is_inflight_tool_line(t, &name))
                {
                    state.tools[pos] = completed;
                } else {
                    state.tools.push(completed);
                }

                if !state.should_update(state.settings.throttle_ms) {
                    continue;
                }

                edit_act_message_if_possible(&sender, chat_id, state.msg_id, &state).await;
                state.last_update = Instant::now();
            }

            StreamCommand::Flush => {
                if state.phase == "think" {
                    if state.think_text.len() > state.last_sent_length {
                        let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                        if let Some(msg_id) = state.msg_id {
                            let _ = sender.edit_message(chat_id, msg_id, &text).await;
                        }
                    }
                    if !state.think_text.is_empty() {
                        state.final_text =
                            truncate_text(&state.think_text, state.settings.max_think_chars);
                    }
                } else if state.phase == "act"
                    && (!state.tools.is_empty() || !state.act_text.trim().is_empty())
                {
                    edit_act_message_if_possible(&sender, chat_id, state.msg_id, &state).await;
                    let body = act_body_for_edit(&state);
                    state.final_text = truncate_text(&body, state.settings.max_act_chars);
                }
                break;
            }
        }
    }

    state.final_text
}
