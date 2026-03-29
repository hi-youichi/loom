use std::collections::HashMap;
use std::fmt;
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
    StartThink { count: u32 },
    StartAct { count: u32 },
    ThinkContent { content: String },
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

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Idle,
    Thinking,
    Acting,
}

impl Phase {
    pub fn is_idle(&self) -> bool {
        matches!(self, Phase::Idle)
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Phase::Idle => write!(f, "idle"),
            Phase::Thinking => write!(f, "think"),
            Phase::Acting => write!(f, "act"),
        }
    }
}

pub struct MessageState {
    pub msg_id: Option<i32>,
    _ack_message_id: Option<i32>,
    _user_message_id: Option<i32>,
    phase: Phase,
    think_text: String,
    tools: Vec<String>,
    act_text: String,
    tool_start_times: HashMap<String, Instant>,
    act_count: Option<u32>,
    last_sent_length: usize,
    last_update: Instant,
    settings: StreamingConfig,
    final_text: String,
    summary_count: u32,
}

impl MessageState {
    pub fn new(settings: StreamingConfig, context: AgentRunContext) -> Self {
        Self {
            msg_id: None,
            _ack_message_id: context.ack_message_id,
            _user_message_id: context.user_message_id,
            phase: Phase::Idle,
            think_text: String::new(),
            tools: Vec::new(),
            act_text: String::new(),
            tool_start_times: HashMap::new(),
            act_count: None,
            last_sent_length: 0,
            last_update: Instant::now(),
            settings,
            final_text: String::new(),
            summary_count: 0,
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
    line.strip_prefix(&prefix).map(|rest| rest.to_string())
}

fn truncate_chars_with_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{}...", truncated)
}

fn should_emit_periodic_summary(state: &MessageState) -> bool {
    !state.phase.is_idle()
        || !state.think_text.trim().is_empty()
        || !state.act_text.trim().is_empty()
        || !state.tools.is_empty()
}

fn phase_label(state: &MessageState) -> &'static str {
    match state.phase {
        Phase::Thinking => "思考中",
        Phase::Acting => "执行中",
        _ => "处理中",
    }
}

fn recent_progress_excerpt(state: &MessageState) -> Option<String> {
    let source = if !state.act_text.trim().is_empty() {
        state.act_text.trim()
    } else if !state.think_text.trim().is_empty() {
        state.think_text.trim()
    } else {
        return None;
    };

    let single_line = source.lines().next().unwrap_or("");
    let truncated = truncate_chars_with_ellipsis(single_line, 80);
    Some(truncated.replace('\n', " "))
}

fn build_periodic_summary_text(state: &MessageState) -> Option<String> {
    if !should_emit_periodic_summary(state) {
        return None;
    }

    let label = phase_label(state);
    let mut parts = vec![format!("⏳ {}...", label)];

    if let Some(excerpt) = recent_progress_excerpt(state) {
        parts.push(excerpt);
    }

    if !state.tools.is_empty() {
        parts.push(format!("🔧 {} tools", state.tools.len()));
    }

    Some(parts.join("\n"))
}

async fn enter_act_phase_without_count_if_needed(
    sender: &Arc<dyn MessageSender>,
    chat_id: i64,
    state: &mut MessageState,
) {
    if state.phase == Phase::Acting {
        return;
    }
    if state.phase == Phase::Thinking && state.think_text.len() > state.last_sent_length {
        tracing::debug!(
            chat_id,
            previous_phase = %state.phase,
            think_text_len = state.think_text.chars().count(),
            last_sent_length = state.last_sent_length,
            "Flushing pending think text before entering act phase"
        );
        let text = truncate_text(&state.think_text, state.settings.max_think_chars);
        if let Some(msg_id) = state.msg_id {
            let _ = sender
                .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                .await;
        }
        state.final_text = state.think_text.clone();
    }
    tracing::debug!(
        chat_id,
        previous_phase = %state.phase,
        show_act_phase = state.settings.show_act_phase,
        "Switching stream handler phase to act"
    );
    state.phase = Phase::Acting;

    state.tools.clear();
    state.act_text.clear();
    state.tool_start_times.clear();
    state.act_count = None;
    state.last_sent_length = 0;
    state.last_update = Instant::now();

    if state.settings.show_act_phase {
        let header = format!("{} Act\n\n", state.settings.act_emoji);
        match sender
            .send_formatted_returning_id(chat_id, &FormattedMessage::markdown_v2(header.clone(), header.clone()))
            .await {
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
        let _ = sender
            .edit_formatted(chat_id, mid, &FormattedMessage::markdown_v2(final_text.clone(), final_text))
            .await;
    }
}

fn finalize_text(state: &mut MessageState) {
    match state.phase {
        Phase::Thinking => {
            if !state.think_text.is_empty() {
                state.final_text = truncate_text(&state.think_text, state.settings.max_think_chars);
            }
        }
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
        StreamCommand::StartThink { count } => {
            tracing::debug!(chat_id, count, previous_phase = %state.phase, "Received StartThink command");
            if state.phase == Phase::Acting && (!state.tools.is_empty() || !state.act_text.trim().is_empty()) {
                edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            }
            if state.phase == Phase::Thinking && state.think_text.len() > state.last_sent_length {
                tracing::debug!(
                    chat_id,
                    think_text_len = state.think_text.chars().count(),
                    last_sent_length = state.last_sent_length,
                    "Flushing pending think text on new Think round"
                );
                let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = sender
                        .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                        .await;
                }
            }

            state.phase = Phase::Thinking;
            state.think_text.clear();

            let header = format!("💭 Think #{}\n\n", count);
            match sender
                .send_formatted_returning_id(chat_id, &FormattedMessage::markdown_v2(header.clone(), header))
                .await
            {
                Ok(id) => state.msg_id = Some(id),
                Err(e) => tracing::warn!(chat_id, "Failed to send Think header: {}", e),
            }
            state.last_update = Instant::now();
            state.last_sent_length = 0;
        }

        StreamCommand::ThinkContent { content } => {
            state.think_text.push_str(&content);

            if !state.should_update(state.settings.throttle_ms) {
                return false;
            }

            let text = truncate_text(&state.think_text, state.settings.max_think_chars);
            if let Some(msg_id) = state.msg_id {
                let _ = sender
                    .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                    .await;
            }
            state.last_update = Instant::now();
            state.last_sent_length = state.think_text.len();
        }

        StreamCommand::ActContent { content } => {
            if !state.settings.show_act_phase {
                return false;
            }
            enter_act_phase_without_count_if_needed(sender, chat_id, state).await;

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
            enter_act_phase_without_count_if_needed(sender, chat_id, state).await;

            let elapsed = state
                .tool_start_times
                .remove(&name)
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);

            let tool_key = name.clone();
            if let Some(pos) = state.tools.iter().position(|l| is_inflight_tool_line(l, &tool_key)) {
                let display_result = truncate_chars_with_ellipsis(
                    &result,
                    state.settings.max_tool_result_chars,
                );

                let mut new_line = if is_error {
                    format!("❌ {} ({}ms)", name, elapsed.as_millis())
                } else {
                    format!("✅ {} ({}ms)", name, elapsed.as_millis())
                };

                let args = extract_inflight_tool_arguments(&state.tools[pos], &tool_key);
                if let Some(args) = args {
                    new_line = format!("{} {}", new_line, args);
                }

                if !display_result.is_empty() {
                    new_line = format!("{}\n```{}```", new_line, display_result);
                }

                state.tools[pos] = new_line;
            }

            if !state.should_update(state.adaptive_throttle_ms()) {
                return false;
            }

            edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            state.last_update = Instant::now();
        }

        StreamCommand::StartAct { count } => {
            if state.phase != Phase::Acting || state.act_count != Some(count) {
                enter_act_phase_without_count_if_needed(sender, chat_id, state).await;
                state.act_count = Some(count);

                let body = act_body_for_edit(state);
                if !body.trim().is_empty() {
                    let text = truncate_text(&body, state.settings.max_act_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = sender
                            .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                            .await;
                    }
                }
            }
        }

        StreamCommand::Flush => {
            tracing::debug!(chat_id, phase = %state.phase, "Received Flush command");
            match state.phase {
                Phase::Thinking if state.think_text.len() > state.last_sent_length => {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = sender
                            .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                            .await;
                    }
                }
                Phase::Acting if !state.tools.is_empty() || !state.act_text.trim().is_empty() => {
                    edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
                }
                _ => {}
            }
            finalize_text(state);
            return true;
        }
    }

    false
}

async fn handle_periodic_command(
    cmd: StreamCommand,
    _chat_id: i64,
    state: &mut MessageState,
) -> bool {
    match cmd {
        StreamCommand::StartThink { .. } => {
            state.phase = Phase::Thinking;
            state.think_text.clear();
            state.act_text.clear();
            state.tools.clear();
            state.tool_start_times.clear();
            state.act_count = None;
        }
        StreamCommand::StartAct { count } => {
            if state.phase != Phase::Acting || state.act_count != Some(count) {
                state.phase = Phase::Acting;
                state.act_count = Some(count);
            }
        }
        StreamCommand::ThinkContent { content } => {
            state.think_text.push_str(&content);
        }
        StreamCommand::ActContent { content } => {
            if !state.settings.show_act_phase {
                return false;
            }
            state.act_text.push_str(&content);
        }
        StreamCommand::ToolStart { name, arguments } => {
            if !state.settings.show_act_phase {
                return false;
            }
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
            if !state.settings.show_act_phase {
                return false;
            }
            let elapsed = state
                .tool_start_times
                .remove(&name)
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);

            let tool_key = name.clone();
            if let Some(pos) = state.tools.iter().position(|l| is_inflight_tool_line(l, &tool_key)) {
                let display_result = truncate_chars_with_ellipsis(
                    &result,
                    state.settings.max_tool_result_chars,
                );

                let mut new_line = if is_error {
                    format!("❌ {} ({}ms)", name, elapsed.as_millis())
                } else {
                    format!("✅ {} ({}ms)", name, elapsed.as_millis())
                };

                let args = extract_inflight_tool_arguments(&state.tools[pos], &tool_key);
                if let Some(args) = args {
                    new_line = format!("{} {}", new_line, args);
                }

                if !display_result.is_empty() {
                    new_line = format!("{}\n```{}```", new_line, display_result);
                }

                state.tools[pos] = new_line;
            }
        }
        StreamCommand::Flush => {
            finalize_text(state);
            return true;
        }
    }
    false
}

pub async fn stream_message_handler(
    rx: mpsc::Receiver<StreamCommand>,
    sender: Arc<dyn MessageSender>,
    chat_id: i64,
    context: AgentRunContext,
    settings: crate::config::Settings,
) -> String {
    stream_message_handler_with_context(
        rx,
        sender,
        chat_id,
        context,
        settings.streaming,
    )
    .await
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
        let mut tick = interval(Duration::from_millis(settings.periodic_summary_ms));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
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
