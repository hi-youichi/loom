//! Agent execution with streaming support
//!
//! Provides functions for running Loom agent with real-time streaming.

use crate::config::Settings;
use crate::error::{BotError, Result};
use crate::streaming::event_mapper::StreamEventMapper;
use crate::streaming::message_handler::StreamCommand;
use crate::traits::{AgentRunContext, MessageSender};
use loom::{run_agent_with_options, RunOptions, RunCmd, RunCompletion};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn run_loom_agent_streaming(
    message: &str,
    chat_id: i64,
    sender: Arc<dyn MessageSender>,
    context: AgentRunContext,
    settings: &Settings,
) -> Result<String> {
    tracing::info!("Running Loom agent (streaming) for chat {}", chat_id);

    let thread_id = format!("telegram_{}", chat_id);

    let (tx, rx) = mpsc::channel::<StreamCommand>(100);

    let model_for_run = context.model_override.clone();
    let handler_sender = sender.clone();
    let handler_settings = settings.streaming.clone();
    let handler_task = tokio::spawn(async move {
        crate::streaming::message_handler::stream_message_handler_with_context(
            rx,
            handler_sender,
            chat_id,
            context,
            handler_settings,
        )
        .await
    });

    let opts = RunOptions {
        message: message.to_string(),
        thread_id: Some(thread_id),
        working_folder: Some(PathBuf::from(".")),
        session_id: None,
        agent: None,
        verbose: false,
        got_adaptive: false,
        display_max_len: 2000,
        output_json: false,
        model: model_for_run,
        provider: None,
        base_url: None,
        api_key: None,
        provider_type: None,
        mcp_config_path: None,
        cancellation: None,
        output_timestamp: false,
        dry_run: false,
    };


    let mapper = StreamEventMapper::new(
        tx.clone(),
        settings.streaming.show_think_phase,
        settings.streaming.show_act_phase,
    );
    let on_event = mapper.boxed_callback();

    let result = run_agent_with_options(&opts, &RunCmd::React, Some(on_event)).await;

    if let Err(send_error) = tx
        .send(crate::streaming::message_handler::StreamCommand::Flush)
        .await
    {
        tracing::error!("Failed to send Flush to stream handler: {}", send_error);
    }
    let final_text = handler_task.await.unwrap_or_default();

    match result {
        Ok(RunCompletion::Finished(_)) => Ok(final_text),
        Ok(RunCompletion::Cancelled) => Err(BotError::Agent("Agent run was cancelled".to_string())),
        Err(e) => Err(BotError::Agent(format!("Agent error: {}", e))),
    }
}
