//! Message handler for streaming responses
//!
//! Processes streaming commands from Loom agent and updates Telegram messages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};

use crate::config::{InteractionMode, StreamingConfig};
use crate::formatting::FormattedMessage;
use crate::traits::{AgentRunContext, MessageSender};
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
    _ack_message_id: Option<i32>,
    _user_message_id: Option<i32>,
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
    summary_count: u32,
}

impl MessageState {
    pub fn new(settings: StreamingConfig, context: AgentRunContext) -> Self {
        Self {
            msg_id: None,
            _ack_message_id: context.ack_message_id,
            _user_message_id: context.user_message_id,
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
            summary_count: 0,
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

fn should_emit_periodic_summary(state: &MessageState) -> bool {
    !state.phase.is_empty()
        || !state.think_text.trim().is_empty()
        || !state.act_text.trim().is_empty()
        || !state.tools.is_empty()
}

fn phase_label(state: &MessageState) -> &'static str {
    match state.phase.as_str() {
        "think" => "思考中",
        "act" => "执行中",
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

    let single_line = source.replace('\n', " ");
    Some(truncate_chars_with_ellipsis(&single_line, 160))
}

fn build_periodic_summary_text(state: &MessageState) -> Option<String> {
    if !should_emit_periodic_summary(state) {
        return None;
    }

    let mut lines = vec![format!("进展更新（第 {} 次）", state.summary_count + 1)];
    lines.push(format!("当前阶段：{}。", phase_label(state)));

    if let Some(count) = state.act_count {
        lines.push(format!("已执行 {} 个动作。", count));
    }

    if !state.tools.is_empty() {
        let recent_tools = state
            .tools
            .iter()
            .rev()
            .take(2)
            .map(|line| truncate_chars_with_ellipsis(&line.replace('\n', " "), 120))
            .collect::<Vec<_>>();
        lines.push(format!(
            "最近工具进展：{}",
            recent_tools.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ));
    }

    if let Some(excerpt) = recent_progress_excerpt(state) {
        lines.push(format!("当前进展：{}", excerpt));
    }

    lines.push("完成后我会单独发送最终结果。".to_string());
    Some(lines.join("\n"))
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
    state.phase = "act".to_string();

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
    if state.phase == "think" {
        if !state.think_text.is_empty() {
            state.final_text = truncate_text(&state.think_text, state.settings.max_think_chars);
        }
    } else if state.phase == "act" && (!state.tools.is_empty() || !state.act_text.trim().is_empty()) {
        let body = act_body_for_edit(state);
        state.final_text = truncate_text(&body, state.settings.max_act_chars);
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
            if state.phase == "act" && (!state.tools.is_empty() || !state.act_text.trim().is_empty()) {
                edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            }
            if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = sender
                .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                .await;
                }
            }

            tracing::debug!(chat_id, count, "Switching stream handler phase to think");
            state.phase = "think".to_string();
            state.think_text.clear();

            state.last_sent_length = 0;
            state.last_update = Instant::now();
            state.act_count = None;

            if state.settings.show_think_phase {
                let header = format!("{} Think #{}\n\n", state.settings.think_emoji, count);
                let header_len = header.len();
                match sender
            .send_formatted_returning_id(chat_id, &FormattedMessage::markdown_v2(header.clone(), header.clone()))
            .await {
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
            tracing::debug!(chat_id, count, previous_phase = %state.phase, "Received StartAct command");
            if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = sender
                        .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                        .await;
                }
                state.final_text = state.think_text.clone();
            }

            tracing::debug!(chat_id, count, "Switching stream handler phase to act");
            state.phase = "act".to_string();
            state.tools.clear();
            state.act_text.clear();
            state.tool_start_times.clear();
            state.last_sent_length = 0;
            state.last_update = Instant::now();

            if state.settings.show_act_phase {
                let header = format!("{} Act #{}\n\n", state.settings.act_emoji, count);
                match sender
                    .send_formatted_returning_id(
                        chat_id,
                        &FormattedMessage::markdown_v2(header.clone(), header.clone()),
                    )
                    .await
                {
                    Ok(id) => {
                        state.msg_id = Some(id);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send Act header: {}", e);
                    }
                }
            }
            state.act_count = Some(count);
        }


        StreamCommand::ThinkContent { content } => {
            if state.phase != "think" || !state.settings.show_think_phase {
                return false;
            }

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

            if !state.should_update(state.settings.throttle_ms) {
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

            if !state.should_update(state.settings.throttle_ms) {
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
                return false;
            }

            edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            state.last_update = Instant::now();
        }

        StreamCommand::Flush => {
            if state.phase == "think" {
                if state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = sender
                .edit_formatted(chat_id, msg_id, &FormattedMessage::markdown_v2(text.clone(), text))
                .await;
                    }
                }
            } else if state.phase == "act" && (!state.tools.is_empty() || !state.act_text.trim().is_empty()) {
                edit_act_message_if_possible(sender, chat_id, state.msg_id, state).await;
            }
            finalize_text(state);
            return true;
        }
    }

    false
}

async fn handle_periodic_command(
    cmd: StreamCommand,
    chat_id: i64,
    state: &mut MessageState,
) -> bool {

    match cmd {
        StreamCommand::StartThink { .. } => {
            state.phase = "think".to_string();
            state.think_text.clear();
            state.act_text.clear();
            state.tools.clear();
            state.tool_start_times.clear();
            state.act_count = None;
        }
        StreamCommand::StartAct { count } => {
            if state.phase != "act" || state.act_count != Some(count) {
                state.phase = "act".to_string();
                state.act_text.clear();
                state.tools.clear();
                state.tool_start_times.clear();
            }
            state.act_count = Some(count);
        }
        StreamCommand::ThinkContent { content } => {
            if state.phase != "think" {
                state.phase = "think".to_string();
            }
            state.think_text.push_str(&content);
        }
        StreamCommand::ActContent { content } => {
            if state.phase != "act" {
                state.phase = "act".to_string();
            }
            state.act_text.push_str(&content);
        }
        StreamCommand::ToolStart { name, arguments } => {
            if state.phase != "act" {
                state.phase = "act".to_string();
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
            if state.phase != "act" {
                state.phase = "act".to_string();
            }

            let status = if is_error { "❌" } else { "✅" };
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
            let truncated_result = truncate_chars_with_ellipsis(&result.replace('\r', ""), 240);
            let existing_args = state
                .tools
                .iter()
                .find(|line| is_inflight_tool_line(line, &name))
                .and_then(|line| extract_inflight_tool_arguments(line, &name));
            let display_name = match existing_args {
                Some(ref args) if !args.is_empty() => format!("{} {}", name, args),
                _ => name.clone(),
            };
            let completed = format!("{} {}{}:\n{}", status, display_name, duration_str, truncated_result);

            if let Some(pos) = state
                .tools
                .iter()
                .position(|t| is_inflight_tool_line(t, &name))
            {
                state.tools[pos] = completed;
            } else {
                state.tools.push(completed);
            }
        }
        StreamCommand::Flush => {
            finalize_text(state);
            tracing::debug!(
                chat_id,
                phase = %state.phase,
                final_text_len = state.final_text.chars().count(),
                summary_count = state.summary_count,
                "Received Flush command and finalized stream text"
            );
            return true;
        }
    }

    false
}

/// Processes streaming UI commands using [`MessageSender`] (mockable in tests).
pub async fn stream_message_handler(
    rx: mpsc::Receiver<StreamCommand>,
    sender: Arc<dyn MessageSender>,
    chat_id: i64,
    settings: StreamingConfig,
) -> String {
    stream_message_handler_with_context(
        rx,
        sender,
        chat_id,
        AgentRunContext {
            interaction_mode: settings.interaction_mode,
            ..AgentRunContext::default()
        },
        settings,
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

    if settings.interaction_mode == InteractionMode::PeriodicSummary {
        let mut summary_interval = interval(Duration::from_secs(settings.summary_interval_secs.max(1)));
        summary_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        summary_interval.tick().await;

        loop {
            tokio::select! {
                maybe_cmd = rx.recv() => {
                    let Some(cmd) = maybe_cmd else {
                        finalize_text(&mut state);
                        break;
                    };

                    if handle_periodic_command(cmd, chat_id, &mut state).await {

                        break;
                    }
                }
                _ = summary_interval.tick() => {
                    if let Some(summary) = build_periodic_summary_text(&state) {
                        tracing::debug!(
                            chat_id,
                            phase = %state.phase,
                            summary_count = state.summary_count,
                            summary_len = summary.chars().count(),
                            "Sending periodic summary"
                        );
                        if let Err(e) = sender
                            .send_formatted(chat_id, &FormattedMessage::markdown_v2(summary.clone(), summary))
                            .await {
                            tracing::warn!("Failed to send periodic summary: {}", e);
                        } else {
                            state.summary_count += 1;
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
