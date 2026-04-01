use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::config::{InteractionMode, StreamingConfig};
use crate::constants::streaming::{LARGE_MESSAGE_THRESHOLD, SMALL_MESSAGE_THRESHOLD};
use crate::formatting::FormattedMessage;
use crate::traits::{AgentRunContext, MessageSender};
use crate::utils::truncate_text;

#[derive(Debug, Clone)]
pub enum StreamCommand {
    StartAct {
        count: u32,
    },
    ActContent {
        content: String,
    },
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

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Idle,
    Acting,
}

impl Phase {
    pub fn is_idle(&self) -> bool {
        matches!(self, Phase::Idle)
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Idle => write!(f, "idle"),
            Phase::Acting => write!(f, "act"),
        }
    }
}

pub struct MessageState {
    pub msg_id: Option<i32>,
    _ack_message_id: Option<i32>,
    _user_message_id: Option<i32>,
    phase: Phase,
    tools: Vec<String>,
    act_text: String,
    tool_start_times: HashMap<String, Instant>,
    act_count: Option<u32>,
    last_sent_length: usize,
    last_update: Instant,
    settings: StreamingConfig,
    final_text: String,
    summary_count: u32,
    // Tracks if we've received a canonical StartAct (not from fallback mode)
    has_received_canonical_start_act: bool,
}

impl MessageState {
    pub fn new(settings: StreamingConfig, context: AgentRunContext) -> Self {
        Self {
            msg_id: None,
            _ack_message_id: context.ack_message_id,
            _user_message_id: context.user_message_id,
            phase: Phase::Idle,
            tools: Vec::new(),
            act_text: String::new(),
            tool_start_times: HashMap::new(),
            act_count: None,
            last_sent_length: 0,
            last_update: Instant::now(),
            settings,
            final_text: String::new(),
            summary_count: 0,
            has_received_canonical_start_act: false,
        }
    }

    pub fn should_update(&self, min_interval_ms: u64) -> bool {
        self.last_update.elapsed() >= Duration::from_millis(min_interval_ms)
    }

    pub fn adaptive_throttle_ms(&self) -> u64 {
        let base = self.settings.throttle_ms;
        if self.last_sent_length > LARGE_MESSAGE_THRESHOLD {
            base * 2
        } else if self.last_sent_length < SMALL_MESSAGE_THRESHOLD {
            base / 2
        } else {
            base
        }
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
    line.strip_prefix(&prefix).map(|s| s.to_string())
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
        let _ = sender
            .edit_formatted(
                chat_id,
                mid,
                &FormattedMessage::markdown_v2(final_text.clone(), final_text),
            )
            .await;
    }
}

fn finalize_text(state: &mut MessageState) {
    match state.phase {
        Phase::Acting if !state.tools.is_empty() || !state.act_text.trim().is_empty() => {
            let body = act_body_for_edit(state);
            state.final_text = truncate_text(&body, state.settings.max_act_chars);
        }
        _ => {}
    }
}

async fn handle_streaming_command(
    cmd: StreamCommand,
    sender: &Arc<dyn MessageSender>,
    chat_id: i64,
    state: &mut MessageState,
) -> bool {
    match cmd {
        StreamCommand::StartAct { count } => {
            tracing::debug!(chat_id, count, previous_phase = %state.phase, "Received StartAct command");

            // If we've already received a canonical StartAct and are in Acting phase,
            // this is a new act cycle - flush pending updates and finalize previous content
            if state.has_received_canonical_start_act && state.phase == Phase::Acting {
                edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
                finalize_text(state);
                state.tools.clear();
                state.tool_start_times.clear();
            }

            state.phase = Phase::Acting;
            state.act_count = Some(count);
            state.act_text.clear();
            state.has_received_canonical_start_act = true;

            let header = format!("{} Act #{}\n\n", state.settings.act_emoji, count);
            match sender
                .send_formatted_returning_id(
                    chat_id,
                    &FormattedMessage::markdown_v2(header.clone(), header),
                )
                .await
            {
                Ok(id) => state.msg_id = Some(id),
                Err(e) => tracing::warn!(chat_id, "Failed to send Act header: {}", e),
            }
            state.last_update = Instant::now();
            state.last_sent_length = 0;
        }

        StreamCommand::ActContent { content } => {
            if !state.settings.show_act_phase {
                return false;
            }

            state.act_text.push_str(&content);

            if !state.should_update(state.adaptive_throttle_ms()) {
                return false;
            }

            edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            state.last_update = Instant::now();
        }

        StreamCommand::ToolStart { name, arguments } => {
            if !state.settings.show_act_phase {
                return false;
            }
            enter_act_phase_without_count_if_needed(sender, chat_id, state).await;

            state.tool_start_times.insert(name.clone(), Instant::now());
            state
                .tools
                .push(format_tool_start_line(&name, arguments.as_deref()));

            if !state.should_update(state.adaptive_throttle_ms()) {
                return false;
            }

            edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            state.last_update = Instant::now();
        }

        StreamCommand::ToolEnd {
            name,
            result,
            is_error,
        } => {
            if !state.settings.show_act_phase {
                return false;
            }

            if let Some(_started) = state.tool_start_times.remove(&name) {
                let result_preview = result.chars().take(200).collect::<String>();
                let result_line = if is_error {
                    format!("  ❌ → {}", result_preview)
                } else {
                    format!("  ✅ → {}", result_preview)
                };

                if let Some(idx) = state
                    .tools
                    .iter()
                    .position(|line| is_inflight_tool_line(line, &name))
                {
                    if let Some(existing_line) = state.tools.get(idx) {
                        if let Some(existing_args) =
                            extract_inflight_tool_arguments(existing_line, &name)
                        {
                            let final_line = if is_error {
                                format!("❌ {} {}   ❌ → {}", name, existing_args, result_preview)
                            } else {
                                format!("✅ {} {}   ✅ → {}", name, existing_args, result_preview)
                            };
                            state.tools[idx] = final_line;
                        } else {
                            let final_line = if is_error {
                                format!("❌ {}   ❌ → {}", name, result_preview)
                            } else {
                                format!("✅ {}   ✅ → {}", name, result_preview)
                            };
                            state.tools[idx] = final_line;
                        }
                    }
                } else {
                    state.tools.push(result_line);
                }
            }

            if !state.should_update(state.adaptive_throttle_ms()) {
                return false;
            }

            edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            state.last_update = Instant::now();
        }

        StreamCommand::Flush => {
            tracing::debug!(chat_id, "Flushing final state");
            if state.phase == Phase::Acting {
                edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            }
            finalize_text(state);
            return true;
        }
    }
    false
}

async fn enter_act_phase_without_count_if_needed(
    sender: &Arc<dyn MessageSender>,
    chat_id: i64,
    state: &mut MessageState,
) {
    if state.phase != Phase::Acting {
        state.phase = Phase::Acting;
        if state.act_count.is_none() {
            state.act_count = Some(1);
            let header = format!("{} Act #1\n\n", state.settings.act_emoji);
            match sender
                .send_formatted_returning_id(
                    chat_id,
                    &FormattedMessage::markdown_v2(header.clone(), header),
                )
                .await
            {
                Ok(id) => state.msg_id = Some(id),
                Err(e) => tracing::warn!(chat_id, "Failed to send Act header: {}", e),
            }
        }
    }
}

fn truncate_chars_with_ellipsis(text: &str, max_chars: usize) -> String {
    let mut truncated: String = text.chars().take(max_chars).collect();
    if truncated.len() < text.len() {
        truncated.push_str("...");
    }
    truncated
}

fn should_emit_periodic_summary(state: &MessageState) -> bool {
    !state.phase.is_idle() || !state.act_text.trim().is_empty() || !state.tools.is_empty()
}

fn phase_label(state: &MessageState) -> &'static str {
    match state.phase {
        Phase::Acting => "执行中",
        _ => "处理中",
    }
}

fn recent_progress_excerpt(state: &MessageState) -> Option<String> {
    let source = state.act_text.trim();
    if source.is_empty() {
        return None;
    }

    let single_line = source.lines().next().unwrap_or("");
    let truncated = truncate_chars_with_ellipsis(single_line, 80);
    Some(truncated.replace('\n', " "))
}

fn build_periodic_summary_text(state: &MessageState) -> Option<String> {
    if !should_emit_periodic_summary(state) {
        return None;
    }

    let tools_summary = if state.tools.is_empty() {
        String::new()
    } else {
        format!("\n\n已执行 {} 个工具", state.tools.len())
    };

    let excerpt = recent_progress_excerpt(state);
    let label = phase_label(state);

    match excerpt {
        Some(progress) => Some(format!("{} {}{}", label, progress, tools_summary)),
        None => Some(format!("{} 进行中{}", label, tools_summary)),
    }
}

async fn handle_periodic_command(
    cmd: StreamCommand,
    chat_id: i64,
    state: &mut MessageState,
) -> bool {
    match cmd {
        StreamCommand::StartAct { count } => {
            tracing::debug!(chat_id, count, previous_phase = %state.phase, "Received StartAct in periodic mode");
            state.phase = Phase::Acting;
            state.act_count = Some(count);
            state.act_text.clear();
            state.has_received_canonical_start_act = true;
            state.last_update = Instant::now();
        }

        StreamCommand::ActContent { content } => {
            state.act_text.push_str(&content);
            state.last_update = Instant::now();
        }

        StreamCommand::ToolStart { name, arguments } => {
            state.tool_start_times.insert(name.clone(), Instant::now());
            state
                .tools
                .push(format_tool_start_line(&name, arguments.as_deref()));
        }

        StreamCommand::ToolEnd {
            name,
            result,
            is_error,
        } => {
            if state.tool_start_times.remove(&name).is_some() {
                let result_preview = result.chars().take(200).collect::<String>();
                let result_line = if is_error {
                    format!("  ❌ → {}", result_preview)
                } else {
                    format!("  ✅ → {}", result_preview)
                };

                if let Some(idx) = state
                    .tools
                    .iter()
                    .position(|line| is_inflight_tool_line(line, &name))
                {
                    if let Some(existing_line) = state.tools.get(idx) {
                        if let Some(existing_args) =
                            extract_inflight_tool_arguments(existing_line, &name)
                        {
                            let final_line = if existing_args.is_empty() {
                                result_line.clone()
                            } else {
                                format!("{} {}", existing_args, result_line)
                            };
                            state.tools[idx] = final_line;
                        } else {
                            state.tools.push(result_line);
                        }
                    }
                } else {
                    state.tools.push(result_line);
                }
            }
        }

        StreamCommand::Flush => {
            tracing::debug!(chat_id, "Flushing final state in periodic mode");
            finalize_text(state);
            return true;
        }
    }
    false
}

pub async fn stream_message_handler_simple(
    rx: mpsc::Receiver<StreamCommand>,
    sender: Arc<dyn MessageSender>,
    chat_id: i64,
    settings: StreamingConfig,
) -> String {
    stream_message_handler_with_context(rx, sender, chat_id, AgentRunContext::default(), settings)
        .await
}

pub async fn stream_message_handler(
    rx: mpsc::Receiver<StreamCommand>,
    sender: Arc<dyn MessageSender>,
    chat_id: i64,
    context: AgentRunContext,
    settings: crate::config::Settings,
) -> String {
    stream_message_handler_with_context(rx, sender, chat_id, context, settings.streaming).await
}

pub async fn stream_message_handler_with_context(
    mut rx: mpsc::Receiver<StreamCommand>,
    sender: Arc<dyn MessageSender>,
    chat_id: i64,
    context: AgentRunContext,
    settings: StreamingConfig,
) -> String {
    let mut state = MessageState::new(settings.clone(), context);
    let interaction_mode = settings.interaction_mode;

    if interaction_mode == InteractionMode::PeriodicSummary {
        let interval_ms = settings.summary_interval_secs * 1000;
        let mut tick = interval(Duration::from_millis(interval_ms));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        tick.reset(); // Reset to avoid immediate first tick
        let mut rx = rx;

        loop {
            tokio::select! {
                maybe_cmd = rx.recv() => {
                    match maybe_cmd {
                        Some(cmd) => {
                            if handle_periodic_command(cmd, chat_id, &mut state).await {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = tick.tick() => {
                    if let Some(summary_text) = build_periodic_summary_text(&state) {
                        state.summary_count += 1;
                        let formatted = FormattedMessage::markdown_v2(summary_text.clone(), summary_text);
                        match sender.send_formatted_returning_id(chat_id, &formatted).await {
                            Ok(id) => {
                                state.msg_id = Some(id);
                            }
                            Err(e) => {
                                tracing::warn!(chat_id, "Failed to send periodic summary: {}", e);
                            }
                        }
                    }
                }
            }
        }
    } else {
        while let Some(cmd) = rx.recv().await {
            if handle_streaming_command(cmd, &sender, chat_id, &mut state).await {
                break;
            }
        }
    }

    tracing::debug!(
        chat_id,
        final_phase = %state.phase,
        final_text_len = state.final_text.chars().count(),
        summary_count = state.summary_count,
        interaction_mode = ?settings.interaction_mode,
        "Stream message handler completed"
    );
    state.final_text
}
