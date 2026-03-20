use std::collections::HashMap;

use chrono::Utc;
use crate::tui::event::TuiEvent;
use crate::tui::models::{Message, MessageContent, MessageRole, ToolCallStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Running,
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub task: String,
    pub status: AgentStatus,
    pub current_node: Option<String>,
    pub progress_message: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
}

pub struct AppState {
    pub agents: HashMap<String, AgentInfo>,
    pub agent_order: Vec<String>,
    pub selected_agent: Option<String>,
    pub messages: Vec<Message>,
}

pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor_position: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState {
                agents: HashMap::new(),
                agent_order: Vec::new(),
                selected_agent: None,
                messages: Vec::new(),
            },
            should_quit: false,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor_position: 0,
        }
    }

    pub fn handle_event(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::AgentStarted { id, name, task } => {
                let agent = AgentInfo {
                    id: id.clone(),
                    name: name.clone(),
                    task: task.clone(),
                    status: AgentStatus::Running,
                    current_node: None,
                    progress_message: None,
                    result: None,
                    error: None,
                };
                self.state.agents.insert(id.clone(), agent);
                if !self.state.agent_order.contains(id) {
                    self.state.agent_order.push(id.clone());
                }
                if self.state.selected_agent.is_none() {
                    self.state.selected_agent = Some(id.clone());
                }
            }
            TuiEvent::AgentProgress { id, node, message } => {
                if let Some(agent) = self.state.agents.get_mut(id) {
                    agent.current_node = Some(node.clone());
                    agent.progress_message = Some(message.clone());
                }
            }
            TuiEvent::AgentCompleted { id, result } => {
                if let Some(agent) = self.state.agents.get_mut(id) {
                    agent.status = AgentStatus::Completed;
                    agent.result = Some(result.clone());
                }
            }
            TuiEvent::AgentError { id, error } => {
                if let Some(agent) = self.state.agents.get_mut(id) {
                    agent.status = AgentStatus::Error;
                    agent.error = Some(error.clone());
                }
            }
            TuiEvent::UserMessageAdded { content } => {
                self.add_user_message(content.clone());
            }
            TuiEvent::AssistantMessageStarted { message_id, .. } => {
                self.ensure_message(
                    message_id,
                    Message {
                        id: message_id.clone(),
                        role: MessageRole::Assistant,
                        content: MessageContent::Text(String::new()),
                        timestamp: Utc::now(),
                        collapsed: false,
                    },
                );
            }
            TuiEvent::AssistantMessageChunk { message_id, chunk, .. } => {
                self.ensure_message(
                    message_id,
                    Message {
                        id: message_id.clone(),
                        role: MessageRole::Assistant,
                        content: MessageContent::Text(String::new()),
                        timestamp: Utc::now(),
                        collapsed: false,
                    },
                );
                if let Some(message) = self.find_message_mut(message_id) {
                    match &mut message.content {
                        MessageContent::Text(text) => text.push_str(chunk),
                        _ => {
                            message.content = MessageContent::Text(chunk.clone());
                        }
                    }
                }
            }
            TuiEvent::AssistantMessageCompleted { .. } => {}
            TuiEvent::ThinkingStarted { message_id, .. } => {
                self.ensure_message(
                    message_id,
                    Message {
                        id: message_id.clone(),
                        role: MessageRole::Assistant,
                        content: MessageContent::Thinking {
                            content: String::new(),
                            complete: false,
                        },
                        timestamp: Utc::now(),
                        collapsed: false,
                    },
                );
            }
            TuiEvent::ThinkingChunk { message_id, chunk, .. } => {
                self.ensure_message(
                    message_id,
                    Message {
                        id: message_id.clone(),
                        role: MessageRole::Assistant,
                        content: MessageContent::Thinking {
                            content: String::new(),
                            complete: false,
                        },
                        timestamp: Utc::now(),
                        collapsed: false,
                    },
                );
                if let Some(message) = self.find_message_mut(message_id) {
                    match &mut message.content {
                        MessageContent::Thinking { content, .. } => content.push_str(chunk),
                        _ => {
                            message.content = MessageContent::Thinking {
                                content: chunk.clone(),
                                complete: false,
                            };
                        }
                    }
                }
            }
            TuiEvent::ThinkingCompleted { message_id, .. } => {
                if let Some(message) = self.find_message_mut(message_id) {
                    if let MessageContent::Thinking { complete, .. } = &mut message.content {
                        *complete = true;
                    }
                }
            }
            TuiEvent::ToolCallStarted {
                call_id,
                name,
                arguments,
                ..
            } => {
                self.ensure_message(
                    call_id,
                    Message {
                        id: call_id.clone(),
                        role: MessageRole::Assistant,
                        content: MessageContent::ToolCall {
                            id: call_id.clone(),
                            name: name.clone(),
                            arguments: arguments.clone(),
                            status: ToolCallStatus::Executing,
                        },
                        timestamp: Utc::now(),
                        collapsed: false,
                    },
                );
            }
            TuiEvent::ToolCallOutput { .. } => {}
            TuiEvent::ToolCallCompleted {
                call_id,
                result,
                is_error,
                ..
            } => {
                if let Some(message) = self.find_message_mut(call_id) {
                    if let MessageContent::ToolCall { status, .. } = &mut message.content {
                        *status = if *is_error {
                            ToolCallStatus::Error
                        } else {
                            ToolCallStatus::Success
                        };
                    }
                }
                self.state
                    .messages
                    .push(Message::tool_result(call_id.clone(), result.clone(), !is_error));
            }
            TuiEvent::Quit => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    pub fn add_user_message(&mut self, content: String) {
        self.state.messages.push(Message::text(MessageRole::User, content));
    }

    pub fn toggle_message_collapse(&mut self, _index: usize) {
        // TODO: Implement message collapse
    }

    fn find_message_mut(&mut self, id: &str) -> Option<&mut Message> {
        self.state.messages.iter_mut().find(|message| message.id == id)
    }

    fn ensure_message(&mut self, id: &str, message: Message) {
        if self.state.messages.iter().all(|existing| existing.id != id) {
            self.state.messages.push(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_appends_streaming_assistant_chunks() {
        let mut app = App::new();
        let message_id = "assistant-1".to_string();

        app.handle_event(&TuiEvent::AssistantMessageStarted {
            agent_id: "agent-1".to_string(),
            message_id: message_id.clone(),
        });
        app.handle_event(&TuiEvent::AssistantMessageChunk {
            agent_id: "agent-1".to_string(),
            message_id: message_id.clone(),
            chunk: "Hello".to_string(),
        });
        app.handle_event(&TuiEvent::AssistantMessageChunk {
            agent_id: "agent-1".to_string(),
            message_id: message_id.clone(),
            chunk: " world".to_string(),
        });

        let message = app
            .state
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("assistant message should exist");
        match &message.content {
            MessageContent::Text(text) => assert_eq!(text, "Hello world"),
            other => panic!("expected text message, got {other:?}"),
        }
    }

    #[test]
    fn app_marks_thinking_complete() {
        let mut app = App::new();
        let message_id = "thinking-1".to_string();

        app.handle_event(&TuiEvent::ThinkingStarted {
            agent_id: "agent-1".to_string(),
            message_id: message_id.clone(),
        });
        app.handle_event(&TuiEvent::ThinkingChunk {
            agent_id: "agent-1".to_string(),
            message_id: message_id.clone(),
            chunk: "step 1".to_string(),
        });
        app.handle_event(&TuiEvent::ThinkingCompleted {
            agent_id: "agent-1".to_string(),
            message_id: message_id.clone(),
        });

        let message = app
            .state
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("thinking message should exist");
        match &message.content {
            MessageContent::Thinking { content, complete } => {
                assert_eq!(content, "step 1");
                assert!(*complete);
            }
            other => panic!("expected thinking message, got {other:?}"),
        }
    }
}
