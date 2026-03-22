// Clean implementation of run_loom_agent_streaming using message queue
// Replace the existing function in handler.rs with this

use crate::config::{Settings, StreamingConfig};
use loom::{run_agent_with_options, RunOptions, RunCmd, RunCompletion, AnyStreamEvent};
use std::sync::Arc;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use teloxide::prelude::*;
use teloxide::types::MessageId;

// ============================================================================
// Stream Commands
// ============================================================================

/// Commands sent from event callback to message handler
#[derive(Debug, Clone)]
pub enum StreamCommand {
    /// Start a new Think phase
    StartThink { count: u32 },
    /// Start a new Act phase
    StartAct { count: u32 },
    /// Add content to Think phase
    ThinkContent { content: String },
    /// Tool execution started
    ToolStart { name: String },
    /// Tool execution finished
    ToolEnd { name: String, result: String, is_error: bool },
    /// Flush any remaining content
    Flush,
}

// ============================================================================
// Message State
// ============================================================================

/// State for tracking message content
pub struct MessageState {
    /// Message ID for current phase
    msg_id: Option<i32>,
    /// Current phase: "think" or "act"
    phase: String,
    /// Text content for Think phase
    think_text: String,
    /// Tool results for Act phase
    tools: Vec<String>,
    /// Length of text that has been sent
    last_sent_length: usize,
    /// Last update time for throttling
    last_update: Instant,
    /// Streaming display settings
    settings: StreamingConfig,
    /// Final text result
    final_text: String,
}

impl MessageState {
    fn new(settings: StreamingConfig) -> Self {
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
    
    /// Check if we should update the message (throttle)
    fn should_update(&self, min_interval_ms: u64) -> bool {
        self.last_update.elapsed() >= Duration::from_millis(min_interval_ms)
    }
}

/// Truncate text to max characters
fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

// ============================================================================
// Message Handler (Single Consumer)
// ============================================================================

/// Message handler that processes commands in order
pub async fn stream_message_handler(
    mut rx: mpsc::Receiver<StreamCommand>,
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    settings: StreamingConfig,
) -> String {
    let mut state = MessageState::new(settings);
    
    while let Some(cmd) = rx.recv().await {
        match cmd {
            StreamCommand::StartThink { count } => {
                // Flush previous content if any
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &text).await;
                    }
                }
                
                // Start new Think phase
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
                // Flush Think phase content
                if state.phase == "think" && state.think_text.len() > state.last_sent_length {
                    let text = truncate_text(&state.think_text, state.settings.max_think_chars);
                    if let Some(msg_id) = state.msg_id {
                        let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &text).await;
                    }
                    state.final_text = state.think_text.clone();
                }
                
                // Start new Act phase
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
                
                // Throttle: update at most every 500ms
                if !state.should_update(500) {
                    continue;
                }
                
                // Update message
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
                
                // Throttle: update at most every 300ms
                if !state.should_update(300) {
                    continue;
                }
                
                // Update message
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
                
                // Format result
                let status = if is_error { "❌" } else { "✅" };
                let truncated_result = if result.len() > 200 {
                    format!("{}...", &result[..200])
                } else {
                    result.clone()
                };
                let single_line_result = truncated_result.replace('\n', "\\n").replace('\r', "");
                let completed = format!("{} {}: {}", status, name, single_line_result);
                
                // Update or add tool result
                if let Some(pos) = state.tools.iter().position(|t| t.starts_with(&format!("🔧 {}...", name))) {
                    state.tools[pos] = completed;
                } else {
                    state.tools.push(completed);
                }
                
                // Throttle: update at most every 300ms
                if !state.should_update(300) {
                    continue;
                }
                
                // Update message
                let text = state.tools.join("\n");
                let final_text = truncate_text(&text, state.settings.max_act_chars);
                if let Some(msg_id) = state.msg_id {
                    let _ = bot.edit_message_text(chat_id, MessageId(msg_id), &final_text).await;
                }
                state.last_update = Instant::now();
            }
            
            StreamCommand::Flush => {
                // Flush remaining content
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

// ============================================================================
// Main Streaming Function
// ============================================================================

/// Run Loom agent with streaming support using message queue
///
/// Uses mpsc channel to ensure message updates are processed in order.
pub async fn run_loom_agent_streaming(
    message: &str,
    chat_id: i64,
    bot: Bot,
    _reply_to: Option<i32>,
    settings: &Settings,
) -> Result<String, String> {
    tracing::info!("Running Loom agent (streaming) for chat {}", chat_id);
    
    let thread_id = format!("telegram_{}", chat_id);
    let chat_id = teloxide::types::ChatId(chat_id);
    
    // Create message channel
    let (tx, rx) = mpsc::channel::<StreamCommand>(100);
    
    // Spawn message handler task
    let handler_bot = bot.clone();
    let handler_chat_id = chat_id;
    let handler_settings = settings.streaming.clone();
    let handler_task = tokio::spawn(async move {
        stream_message_handler(rx, handler_bot, handler_chat_id, handler_settings).await
    });
    
    // Phase state for tracking
    let phase_state = Arc::new(std::sync::RwLock::new((
        String::new(), // current_phase
        0u32,          // think_count
        0u32,          // act_count
    )));
    
    let opts = RunOptions {
        message: message.to_string(),
        thread_id: Some(thread_id),
        working_folder: Some(PathBuf::from(".")),
        session_id: None,
        role_file: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
        model: None,
        mcp_config_path: None,
        cancellation: None,
        output_timestamp: false,
        dry_run: false,
    };
    
    // Create event callback that sends commands to channel
    let tx_clone = tx.clone();
    let phase_state_clone = phase_state.clone();
    let show_think = settings.streaming.show_think_phase;
    let show_act = settings.streaming.show_act_phase;
    
    let on_event = move |ev: AnyStreamEvent| {
        let tx = tx_clone.clone();
        let phase_state = phase_state_clone.clone();
        
        match &ev {
            // Think phase start
            AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                if node_id == "think" && show_think =>
            {
                let (phase, think_count, act_count) = {
                    let mut ps = phase_state.write().unwrap();
                    ps.1 += 1;
                    ps.0 = "think".to_string();
                    (ps.0.clone(), ps.1, ps.2)
                };
                let _ = tx.blocking_send(StreamCommand::StartThink { count: think_count });
            }
            
            // Act phase start
            AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                if node_id == "act" && show_act =>
            {
                let (phase, think_count, act_count) = {
                    let mut ps = phase_state.write().unwrap();
                    ps.2 += 1;
                    ps.0 = "act".to_string();
                    (ps.0.clone(), ps.1, ps.2)
                };
                let _ = tx.blocking_send(StreamCommand::StartAct { count: act_count });
            }
            
            // Think content
            AnyStreamEvent::React(loom::StreamEvent::Messages { chunk, .. }) => {
                let phase = phase_state.read().unwrap().0.clone();
                if phase == "think" && !chunk.content.is_empty() {
                    let _ = tx.blocking_send(StreamCommand::ThinkContent {
                        content: chunk.content.clone()
                    });
                }
            }
            
            // Tool start
            AnyStreamEvent::React(loom::StreamEvent::ToolStart { name, .. }) => {
                let _ = tx.blocking_send(StreamCommand::ToolStart { name: name.clone() });
            }
            
            // Tool end
            AnyStreamEvent::React(loom::StreamEvent::ToolEnd { name, result, is_error, .. }) => {
                let _ = tx.blocking_send(StreamCommand::ToolEnd {
                    name: name.clone(),
                    result: result.clone(),
                    is_error: *is_error,
                });
            }
            
            _ => {}
        }
    };
    
    // Run the agent
    let result = run_agent_with_options(&opts, &RunCmd::React, Some(Box::new(on_event))).await;
    
    // Send flush command and wait for handler to complete
    let _ = tx.send(StreamCommand::Flush).await;
    let final_text = handler_task.await.unwrap_or_default();
    
    match result {
        Ok(RunCompletion::Finished(_)) => Ok(final_text),
        Ok(RunCompletion::Cancelled) => Err("Agent run was cancelled".to_string()),
        Err(e) => Err(format!("Agent error: {}", e)),
    }
}
