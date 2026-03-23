//! Agent execution with streaming support
//!
//! Provides functions for running Loom agent with real-time streaming.

use crate::config::{Settings, StreamingConfig};
use crate::error::{BotError, Result};
use crate::streaming::message_handler::StreamCommand;
use loom::{run_agent_with_options, RunOptions, RunCmd, RunCompletion, AnyStreamEvent};
use std::sync::Arc;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use teloxide::prelude::*;
use teloxide::types::{MessageId, Message, ChatId};

pub async fn run_loom_agent_streaming(
    message: &str,
    chat_id: i64,
    bot: Bot,
    _reply_to: Option<i32>,
    settings: &Settings,
) -> Result<String> {
    tracing::info!("Running Loom agent (streaming) for chat {}", chat_id);
    
    let thread_id = format!("telegram_{}", chat_id);
    let chat_id = ChatId(chat_id);
    
    let (tx, rx) = mpsc::channel::<StreamCommand>(100);
    
    let handler_bot = bot.clone();
    let handler_chat_id = chat_id;
    let handler_settings = settings.streaming.clone();
    let handler_task = tokio::spawn(async move {
        crate::streaming::message_handler::stream_message_handler(
            rx,
            handler_bot,
            handler_chat_id,
            handler_settings
        ).await
    });
    
    let phase_state = Arc::new(std::sync::RwLock::new((
        String::new(),
        0u32,
        0u32,
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
    
    let tx_clone = tx.clone();
    let phase_state_clone = phase_state.clone();
    let show_think = settings.streaming.show_think_phase;
    let show_act = settings.streaming.show_act_phase;
    
    let on_event = move |ev: AnyStreamEvent| {
        let tx = tx_clone.clone();
        let phase_state = phase_state_clone.clone();
        
        tokio::task::block_in_place(|| {
            match &ev {
                AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                    if node_id == "think" && show_think =>
                {
                    let (_phase, think_count, _act_count) = {
                        let mut ps = phase_state.write().unwrap();
                        ps.1 += 1;
                        ps.0 = "think".to_string();
                        (ps.0.clone(), ps.1, ps.2)
                    };
                    let _ = tx.blocking_send(crate::streaming::message_handler::StreamCommand::StartThink { count: think_count });
                }
                
                AnyStreamEvent::React(loom::StreamEvent::TaskStart { node_id, .. }) 
                    if node_id == "act" && show_act =>
                {
                    let (_phase, _think_count, act_count) = {
                        let mut ps = phase_state.write().unwrap();
                        ps.2 += 1;
                        ps.0 = "act".to_string();
                        (ps.0.clone(), ps.1, ps.2)
                    };
                    let _ = tx.blocking_send(crate::streaming::message_handler::StreamCommand::StartAct { count: act_count });
                }
                
                AnyStreamEvent::React(loom::StreamEvent::Messages { chunk, .. }) => {
                    let phase = phase_state.read().unwrap().0.clone();
                    if phase == "think" && !chunk.content.is_empty() {
                        let _ = tx.blocking_send(crate::streaming::message_handler::StreamCommand::ThinkContent {
                            content: chunk.content.clone()
                        });
                    }
                }
                
                AnyStreamEvent::React(loom::StreamEvent::ToolStart { name, .. }) => {
                    let _ = tx.blocking_send(crate::streaming::message_handler::StreamCommand::ToolStart { name: name.clone() });
                }
                
                AnyStreamEvent::React(loom::StreamEvent::ToolEnd { name, result, is_error, .. }) => {
                    let _ = tx.blocking_send(crate::streaming::message_handler::StreamCommand::ToolEnd {
                        name: name.clone(),
                        result: result.clone(),
                        is_error: *is_error,
                    });
                }
                
                _ => {}
            }
        });
    };
    
    let result = run_agent_with_options(&opts, &RunCmd::React, Some(Box::new(on_event))).await;
    
    let _ = tx.send(crate::streaming::message_handler::StreamCommand::Flush).await;
    let final_text = handler_task.await.unwrap_or_default();
    
    match result {
        Ok(RunCompletion::Finished(_)) => Ok(final_text),
        Ok(RunCompletion::Cancelled) => Err(BotError::Agent("Agent run was cancelled".to_string())),
        Err(e) => Err(BotError::Agent(format!("Agent error: {}", e))),
    }
}
