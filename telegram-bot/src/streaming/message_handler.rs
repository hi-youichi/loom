//! Message handler for streaming responses
//!
//! Processes streaming commands from Loom agent and updates Telegram messages.

use crate::config::StreamingConfig;
use crate::utils::truncate_text;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ChatId};

/// Commands sent from event callback to message handler
#[derive(Debug, Clone)]
pub enum StreamCommand {
    StartThink { count: u32 },
    StartAct { count: u32 },
    ThinkContent { content: String },
    ToolStart { name: String },
    ToolEnd { name: String, result: String, is_error: bool },
    Flush,
}

/// State for tracking message content
pub struct MessageState {
    pub msg_id: Option<i32>,
    phase: String,
    think_text: String,
    tools: Vec<String>,
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

/// Message handler that processes commands in order
pub async fn stream_message_handler(
    mut rx: mpsc::Receiver<StreamCommand>,
    bot: Bot,
    chat_id: ChatId,
    settings: StreamingConfig,
) -> String {
    let mut state = MessageState::new(settings);
    
    while let Some(cmd) = rx.recv().await {
        match cmd {
            StreamCommand::StartThink { count } => {
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &text).await;
                    }
                }
                
                if state.settings.show_think_phase {
                    let header = format!("{} Think #{}\n\n", state.settings.think_emoji, count);
                    let header_len = header.len();
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        state.msg_id = Some(msg.id.0);
                        state.phase = "think".to_string();
                        state.think_text = header;
                        state.last_sent_length = header_len;
                        state.last_update = Instant::now();
                    }
                }
            }
            
            StreamCommand::StartAct { count } => {
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &text).await;
                    }
                    state.final_text = state.think_text.clone();
                }
                
                if state.settings.show_act_phase {
                    let header = format!("{} Act #{}\n\n", state.settings.act_emoji, count);
                    if let Ok(msg) = bot.send_message(chat_id, &header).await {
                        state.msg_id = Some(msg.id.0);
                        state.phase = "act".to_string();
                        state.tools.clear();
                        state.last_sent_length = 0;
                        state.last_update = Instant::now();
                    }
                }
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
                    let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &text).await;
                }
                state.last_update = Instant::now();
                state.last_sent_length = state.think_text.len();
            }
            
            StreamCommand::ToolStart { name } => {
                if state.phase != "act" || !state.settings.show_act_phase {
                    continue;
                }
                
                state.tools.push(format!("🔧 {}...", name));
                
                if !state.should_update(state.settings.throttle_ms) {
                    continue;
                }
                
                let text = state.tools.join("\n");
                let final_text = truncate_text(&text, state.settings.max_act_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &final_text).await;
                }
                state.last_update = Instant::now();
            }
            
            StreamCommand::ToolEnd { name, result, is_error } => {
                if state.phase != "act" || !state.settings.show_act_phase {
                    continue;
                }
                
                let status = if is_error { "❌" } else { "✅" };
                let max_result_len = state.settings.max_act_chars.saturating_sub(50).min(200);
                let truncated_result = if result.len() > max_result_len {
                    format!("{}...", &result[..max_result_len])
                } else {
                    result.clone()
                };
                let single_line_result = truncated_result.replace('\n', "\\n").replace('\r', "");
                let completed = format!("{} {}: {}", status, name, single_line_result);
                
                if let Some(pos) = state.tools.iter().position(|t| t.starts_with(&format!("🔧 {}...", name))) {
                    state.tools[pos] = completed;
                } else {
                    state.tools.push(completed);
                }
                
                if !state.should_update(state.settings.throttle_ms) {
                    continue;
                }
                
                let text = state.tools.join("\n");
                let final_text = truncate_text(&text, state.settings.max_act_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &final_text).await;
                }
                state.last_update = Instant::now();
            }
            
            StreamCommand::Flush => {
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &text).await;
                    }
                    state.final_text = state.think_text.clone();
                } else if state.phase == "act" && !state.tools.is_empty() {
                    let text = state.tools.join("\n");
                    let final_text = truncate_text(&text, state.settings.max_act_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &final_text).await;
                    }
                }
                break;
            }
        }
    }
    
    state.final_text
}
