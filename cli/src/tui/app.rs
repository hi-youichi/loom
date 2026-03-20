use std::collections::HashMap;

use crate::tui::event::TuiEvent;

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
    pub agent_order: Vec<String>, // Maintain display order
    pub messages: Vec<String>,     // User input messages
}

pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub input_mode: InputMode,
    pub input: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState {
                agents: HashMap::new(),
                agent_order: Vec::new(),
                messages: Vec::new(),
            },
            should_quit: false,
            input_mode: InputMode::Normal,
            input: String::new(),
        }
    }

    pub fn handle_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::AgentStarted { id, name, task } => {
                let agent = AgentInfo {
                    id: id.clone(),
                    name,
                    task,
                    status: AgentStatus::Running,
                    current_node: None,
                    progress_message: None,
                    result: None,
                    error: None,
                };
                self.state.agents.insert(id.clone(), agent);
                self.state.agent_order.push(id);
            }
            TuiEvent::AgentProgress { id, node, message } => {
                if let Some(agent) = self.state.agents.get_mut(&id) {
                    agent.current_node = Some(node);
                    agent.progress_message = Some(message);
                }
            }
            TuiEvent::AgentCompleted { id, result } => {
                if let Some(agent) = self.state.agents.get_mut(&id) {
                    agent.status = AgentStatus::Completed;
                    agent.result = Some(result);
                }
            }
            TuiEvent::AgentError { id, error } => {
                if let Some(agent) = self.state.agents.get_mut(&id) {
                    agent.status = AgentStatus::Error;
                    agent.error = Some(error);
                }
            }
            TuiEvent::Quit => {
                self.should_quit = true;
            }
            TuiEvent::Tick => {
                // Tick event for UI refresh - no state change needed
            }
            TuiEvent::Key(key) => {
                use crossterm::event::{KeyCode, KeyModifiers};
                
                match self.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.should_quit = true;
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            self.should_quit = true;
                        }
                        KeyCode::Char('i') => {
                            self.input_mode = InputMode::Editing;
                        }
                        _ => {}
                    },
                    InputMode::Editing => match key.code {
                        KeyCode::Enter => {
                            if !self.input.is_empty() {
                                self.state.messages.push(self.input.clone());
                                self.input.clear();
                            }
                            self.input_mode = InputMode::Normal;
                        }
                        KeyCode::Esc => {
                            self.input_mode = InputMode::Normal;
                            self.input.clear();
                        }
                        KeyCode::Backspace => {
                            self.input.pop();
                        }
                        KeyCode::Char(c) => {
                            self.input.push(c);
                        }
                        _ => {}
                    },
                }
            }
            TuiEvent::InputSubmitted(_text) => {
                // Handle input submitted event if needed
            }
        }
    }

    pub fn update(&mut self) {
        // Placeholder for any periodic updates
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
