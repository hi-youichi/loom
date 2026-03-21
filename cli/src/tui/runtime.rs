use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{RunBackend, RunCmd, RunOptions, RunOutput, StreamOut};

use super::TuiEvent;

pub fn spawn_agent_run(
    event_tx: mpsc::UnboundedSender<TuiEvent>,
    backend: Arc<dyn RunBackend>,
    opts: RunOptions,
    cmd: RunCmd,
    agent_id: String,
    task: String,
) {
    let _ = event_tx.send(TuiEvent::AgentStarted {
        id: agent_id.clone(),
        name: "Loom Agent".to_string(),
        task,
    });

    tokio::spawn(async move {
        let bridge_state = Arc::new(Mutex::new(StreamBridgeState::new(agent_id.clone())));
        let stream_state = Arc::clone(&bridge_state);
        let stream_tx = event_tx.clone();

        let stream_out: StreamOut = Some(Arc::new(Mutex::new(move |value: Value| {
            if let Ok(mut bridge) = stream_state.lock() {
                bridge.handle_value(&stream_tx, value);
            }
        })));

        match backend.run(&opts, &cmd, stream_out).await {
            Ok(output) => {
                finalize_stream(&event_tx, &bridge_state, &output);
                let reply = match output {
                    RunOutput::Reply { reply, .. } | RunOutput::Json { reply, .. } => reply,
                };
                let _ = event_tx.send(TuiEvent::AgentCompleted {
                    id: agent_id,
                    result: reply,
                });
            }
            Err(error) => {
                finalize_stream(&event_tx, &bridge_state, &RunOutput::Reply {
                    reply: String::new(),
                    reasoning_content: None,
                    reply_envelope: None,
                    stop_reason: crate::backend::RunStopReason::EndTurn,
                });
                let _ = event_tx.send(TuiEvent::AgentError {
                    id: agent_id,
                    error: error.to_string(),
                });
            }
        }
    });
}

fn finalize_stream(
    event_tx: &mpsc::UnboundedSender<TuiEvent>,
    state: &Arc<Mutex<StreamBridgeState>>,
    output: &RunOutput,
) {
    let Ok(mut bridge) = state.lock() else {
        return;
    };

    let (reply, reasoning_content) = match output {
        RunOutput::Reply {
            reply,
            reasoning_content,
            ..
        }
        | RunOutput::Json {
            reply,
            reasoning_content,
            ..
        } => (reply.as_str(), reasoning_content.as_deref()),
    };

    if !bridge.seen_thinking {
        if let Some(reasoning) = reasoning_content.filter(|value| !value.is_empty()) {
            let _ = event_tx.send(TuiEvent::ThinkingStarted {
                agent_id: bridge.agent_id.clone(),
                message_id: bridge.thinking_message_id.clone(),
            });
            let _ = event_tx.send(TuiEvent::ThinkingChunk {
                agent_id: bridge.agent_id.clone(),
                message_id: bridge.thinking_message_id.clone(),
                chunk: reasoning.to_string(),
            });
            bridge.seen_thinking = true;
        }
    }

    if bridge.seen_thinking {
        let _ = event_tx.send(TuiEvent::ThinkingCompleted {
            agent_id: bridge.agent_id.clone(),
            message_id: bridge.thinking_message_id.clone(),
        });
    }

    if !bridge.seen_assistant && !reply.is_empty() {
        let _ = event_tx.send(TuiEvent::AssistantMessageStarted {
            agent_id: bridge.agent_id.clone(),
            message_id: bridge.assistant_message_id.clone(),
        });
        let _ = event_tx.send(TuiEvent::AssistantMessageChunk {
            agent_id: bridge.agent_id.clone(),
            message_id: bridge.assistant_message_id.clone(),
            chunk: reply.to_string(),
        });
        bridge.seen_assistant = true;
    }

    if bridge.seen_assistant {
        let _ = event_tx.send(TuiEvent::AssistantMessageCompleted {
            agent_id: bridge.agent_id.clone(),
            message_id: bridge.assistant_message_id.clone(),
        });
    }
}

struct StreamBridgeState {
    agent_id: String,
    thinking_message_id: String,
    assistant_message_id: String,
    seen_thinking: bool,
    seen_assistant: bool,
    tool_seq: usize,
    tool_ids_by_name: HashMap<String, String>,
}

impl StreamBridgeState {
    fn new(agent_id: String) -> Self {
        Self {
            thinking_message_id: format!("{}-thinking-{}", agent_id, Uuid::new_v4()),
            assistant_message_id: format!("{}-assistant-{}", agent_id, Uuid::new_v4()),
            agent_id,
            seen_thinking: false,
            seen_assistant: false,
            tool_seq: 0,
            tool_ids_by_name: HashMap::new(),
        }
    }

    fn handle_value(&mut self, event_tx: &mpsc::UnboundedSender<TuiEvent>, value: Value) {
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return;
        };

        match event_type {
            "node_enter" => {
                let node = value
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("run")
                    .to_string();
                let _ = event_tx.send(TuiEvent::AgentProgress {
                    id: self.agent_id.clone(),
                    node: node.clone(),
                    message: format!("Running {}", node),
                });
            }
            "thought_chunk" => {
                if !self.seen_thinking {
                    let _ = event_tx.send(TuiEvent::ThinkingStarted {
                        agent_id: self.agent_id.clone(),
                        message_id: self.thinking_message_id.clone(),
                    });
                    self.seen_thinking = true;
                }
                if let Some(chunk) = value.get("content").and_then(Value::as_str) {
                    let _ = event_tx.send(TuiEvent::ThinkingChunk {
                        agent_id: self.agent_id.clone(),
                        message_id: self.thinking_message_id.clone(),
                        chunk: chunk.to_string(),
                    });
                }
            }
            "message_chunk" => {
                if !self.seen_assistant {
                    let _ = event_tx.send(TuiEvent::AssistantMessageStarted {
                        agent_id: self.agent_id.clone(),
                        message_id: self.assistant_message_id.clone(),
                    });
                    self.seen_assistant = true;
                }
                if let Some(chunk) = value.get("content").and_then(Value::as_str) {
                    let _ = event_tx.send(TuiEvent::AssistantMessageChunk {
                        agent_id: self.agent_id.clone(),
                        message_id: self.assistant_message_id.clone(),
                        chunk: chunk.to_string(),
                    });
                }
            }
            "tool_call" => {
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let call_id = value
                    .get("call_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| {
                        let generated = format!("{}-tool-{}", self.agent_id, self.tool_seq);
                        self.tool_seq += 1;
                        generated
                    });
                let arguments = value
                    .get("arguments")
                    .map(|args| serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string()))
                    .unwrap_or_else(|| "{}".to_string());
                self.tool_ids_by_name.insert(name.clone(), call_id.clone());
                let _ = event_tx.send(TuiEvent::ToolCallStarted {
                    agent_id: self.agent_id.clone(),
                    call_id,
                    name,
                    arguments,
                });
            }
            "tool_output" => {
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let call_id = self.resolve_tool_call_id(&name, value.get("call_id"));
                let content = value
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let _ = event_tx.send(TuiEvent::ToolCallOutput {
                    agent_id: self.agent_id.clone(),
                    call_id,
                    content,
                });
            }
            "tool_end" => {
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let call_id = self.resolve_tool_call_id(&name, value.get("call_id"));
                let result = value
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let is_error = value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let _ = event_tx.send(TuiEvent::ToolCallCompleted {
                    agent_id: self.agent_id.clone(),
                    call_id,
                    result,
                    is_error,
                });
            }
            _ => {}
        }
    }

    fn resolve_tool_call_id(&mut self, name: &str, call_id: Option<&Value>) -> String {
        if let Some(id) = call_id.and_then(Value::as_str) {
            return id.to_string();
        }

        if let Some(existing) = self.tool_ids_by_name.get(name) {
            return existing.clone();
        }

        let generated = format!("{}-tool-{}", self.agent_id, self.tool_seq);
        self.tool_seq += 1;
        self.tool_ids_by_name
            .insert(name.to_string(), generated.clone());
        generated
    }
}
