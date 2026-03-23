//! Agent execution with streaming support
//!
//! Provides functions for running Loom agent with real-time streaming.

use crate::config::Settings;
use crate::error::{BotError, Result};
use crate::streaming::message_handler::StreamCommand;
use crate::traits::MessageSender;
use loom::{run_agent_with_options, RunOptions, RunCmd, RunCompletion, AnyStreamEvent};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug)]
enum CommandPriority {
    Critical,
    BestEffort,
}

fn stream_command_kind(command: &StreamCommand) -> &'static str {
    match command {
        StreamCommand::StartThink { .. } => "StartThink",
        StreamCommand::StartAct { .. } => "StartAct",
        StreamCommand::ThinkContent { .. } => "ThinkContent",
        StreamCommand::ActContent { .. } => "ActContent",
        StreamCommand::ToolStart { .. } => "ToolStart",
        StreamCommand::ToolEnd { .. } => "ToolEnd",
        StreamCommand::Flush => "Flush",
    }
}

fn send_stream_command(
    tx: &mpsc::Sender<StreamCommand>,
    command: StreamCommand,
    priority: CommandPriority,
) {
    let command_kind = stream_command_kind(&command);
    match tx.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::error!(
                "Stream command send failed (handler receiver closed?): {}",
                command_kind
            );
        }
        Err(mpsc::error::TrySendError::Full(command)) => match priority {
            CommandPriority::BestEffort => {
                tracing::debug!("Dropped best-effort stream command due to full queue: {}", command_kind);
            }
            CommandPriority::Critical => {
                let tx_clone = tx.clone();
                tokio::spawn(async move {
                    if let Err(send_error) = tx_clone.send(command).await {
                        tracing::error!(
                            "Stream command send failed after queue drain (handler receiver closed?): {}",
                            send_error
                        );
                    }
                });
            }
        },
    }
}

struct PendingToolArguments {
    by_call_id: HashMap<String, String>,
    by_name: HashMap<String, VecDeque<String>>,
}

impl PendingToolArguments {
    fn new() -> Self {
        Self {
            by_call_id: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    fn remember(&mut self, call_id: &Option<String>, tool_name: &str, arguments: &serde_json::Value) {
        let arguments_text = arguments.to_string();
        if let Some(id) = call_id {
            self.by_call_id.insert(id.clone(), arguments_text);
            return;
        }
        self.by_name
            .entry(tool_name.to_string())
            .or_default()
            .push_back(arguments_text);
    }

    fn take_for_start(&mut self, call_id: &Option<String>, tool_name: &str) -> Option<String> {
        if let Some(id) = call_id {
            if let Some(arguments) = self.by_call_id.remove(id) {
                return Some(arguments);
            }
        }
        self.by_name.get_mut(tool_name).and_then(|queue| queue.pop_front())
    }
}

pub async fn run_loom_agent_streaming(
    message: &str,
    chat_id: i64,
    sender: Arc<dyn MessageSender>,
    _reply_to: Option<i32>,
    settings: &Settings,
) -> Result<String> {
    tracing::info!("Running Loom agent (streaming) for chat {}", chat_id);

    let thread_id = format!("telegram_{}", chat_id);

    let (tx, rx) = mpsc::channel::<StreamCommand>(100);

    let handler_sender = sender.clone();
    let handler_settings = settings.streaming.clone();
    let handler_task = tokio::spawn(async move {
        crate::streaming::message_handler::stream_message_handler(
            rx,
            handler_sender,
            chat_id,
            handler_settings,
        )
        .await
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
    let pending_tool_args = Arc::new(std::sync::RwLock::new(PendingToolArguments::new()));
    let pending_tool_args_clone = pending_tool_args.clone();
    let show_think = settings.streaming.show_think_phase;
    let show_act = settings.streaming.show_act_phase;

    let on_event = move |ev: AnyStreamEvent| {
        let tx = tx_clone.clone();
        let phase_state = phase_state_clone.clone();
        let pending_tool_args = pending_tool_args_clone.clone();

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
                    send_stream_command(
                        &tx,
                        StreamCommand::StartThink {
                            count: think_count,
                        },
                        CommandPriority::Critical,
                    );
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
                    send_stream_command(
                        &tx,
                        StreamCommand::StartAct {
                            count: act_count,
                        },
                        CommandPriority::Critical,
                    );
                }

                AnyStreamEvent::React(loom::StreamEvent::Messages {
                    chunk,
                    metadata,
                }) => {
                    if chunk.content.is_empty() {
                        return;
                    }
                    match metadata.loom_node.as_str() {
                        "think" if show_think => {
                            send_stream_command(
                                &tx,
                                StreamCommand::ThinkContent {
                                    content: chunk.content.clone(),
                                },
                                CommandPriority::BestEffort,
                            );
                        }
                        "act" if show_act => {
                            send_stream_command(
                                &tx,
                                StreamCommand::ActContent {
                                    content: chunk.content.clone(),
                                },
                                CommandPriority::BestEffort,
                            );
                        }
                        _ => {
                            // Fallback for providers/runtimes where node metadata may not be set
                            // consistently: keep the previous phase-based routing behavior.
                            let phase = phase_state.read().unwrap().0.clone();
                            match phase.as_str() {
                                "think" if show_think => {
                                    send_stream_command(
                                        &tx,
                                        StreamCommand::ThinkContent {
                                            content: chunk.content.clone(),
                                        },
                                        CommandPriority::BestEffort,
                                    );
                                }
                                "act" if show_act => {
                                    send_stream_command(
                                        &tx,
                                        StreamCommand::ActContent {
                                            content: chunk.content.clone(),
                                        },
                                        CommandPriority::BestEffort,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolStart { call_id, name }) => {
                    if show_act {
                        let tool_arguments = pending_tool_args
                            .write()
                            .unwrap()
                            .take_for_start(call_id, name);
                        send_stream_command(
                            &tx,
                            StreamCommand::ToolStart {
                                name: name.clone(),
                                arguments: tool_arguments,
                            },
                            CommandPriority::Critical,
                        );
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolCall {
                    call_id,
                    name,
                    arguments,
                }) => {
                    if show_act {
                        pending_tool_args
                            .write()
                            .unwrap()
                            .remember(call_id, name, arguments);
                    }
                }

                AnyStreamEvent::React(loom::StreamEvent::ToolEnd {
                    name,
                    result,
                    is_error,
                    ..
                }) => {
                    if show_act {
                        send_stream_command(
                            &tx,
                            StreamCommand::ToolEnd {
                                name: name.clone(),
                                result: result.clone(),
                                is_error: *is_error,
                            },
                            CommandPriority::Critical,
                        );
                    }
                }

                _ => {}
            }
        });
    };

    let result = run_agent_with_options(&opts, &RunCmd::React, Some(Box::new(on_event))).await;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn best_effort_command_is_dropped_when_channel_is_full() {
        let (tx, mut rx) = mpsc::channel::<StreamCommand>(1);
        tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();

        send_stream_command(
            &tx,
            StreamCommand::ActContent {
                content: "chunk".to_string(),
            },
            CommandPriority::BestEffort,
        );

        let first = rx.recv().await.unwrap();
        assert!(matches!(first, StreamCommand::StartAct { count: 1 }));
        assert!(
            timeout(Duration::from_millis(20), rx.recv()).await.is_err(),
            "best-effort command should be dropped when queue is full"
        );
    }

    #[tokio::test]
    async fn critical_command_waits_for_queue_drain_when_channel_is_full() {
        let (tx, mut rx) = mpsc::channel::<StreamCommand>(1);
        tx.send(StreamCommand::StartAct { count: 1 }).await.unwrap();

        send_stream_command(
            &tx,
            StreamCommand::ToolEnd {
                name: "ls".to_string(),
                result: "ok".to_string(),
                is_error: false,
            },
            CommandPriority::Critical,
        );

        let first = rx.recv().await.unwrap();
        assert!(matches!(first, StreamCommand::StartAct { count: 1 }));

        let second = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("critical command should be sent after queue drains")
            .expect("channel should still be open");
        assert!(matches!(second, StreamCommand::ToolEnd { .. }));
    }
}
