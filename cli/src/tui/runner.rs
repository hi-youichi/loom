//! TUI Runner - Main event loop and orchestration for the TUI application.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{LocalBackend, RunBackend, RunCmd, RunOptions};

use super::runtime::spawn_agent_run;
use super::{App, EventChannel, EventHandler, TuiEvent, TerminalManager};

/// Configuration for the TUI runner
pub struct TuiConfig {
    /// Tick rate in milliseconds (default: 250ms)
    pub tick_rate: Duration,
    /// Whether to show demo mode with simulated agents
    pub demo_mode: bool,
    /// Working folder for file tools
    pub working_folder: Option<PathBuf>,
    /// Optional role/instructions file
    pub role_file: Option<PathBuf>,
    /// Optional named agent profile
    pub agent: Option<String>,
    /// Enable verbose backend logging
    pub verbose: bool,
    /// Optional model override
    pub model: Option<String>,
    /// Optional MCP config path
    pub mcp_config_path: Option<PathBuf>,
    /// Run tools in dry-run mode
    pub dry_run: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(250),
            demo_mode: false,
            working_folder: None,
            role_file: None,
            agent: None,
            verbose: false,
            model: None,
            mcp_config_path: None,
            dry_run: false,
        }
    }
}

/// Main runner for the TUI application
pub struct TuiRunner {
    app: App,
    config: TuiConfig,
    backend: Arc<dyn RunBackend>,
    thread_id: Option<String>,
}

impl TuiRunner {
    /// Create a new TUI runner
    pub fn new(config: TuiConfig) -> Self {
        Self {
            app: App::new(),
            config,
            backend: Arc::new(LocalBackend),
            thread_id: None,
        }
    }

    /// Run the TUI application
    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Setup terminal
        let mut terminal_manager = TerminalManager::new()?;

        // Run demo mode if enabled
        if self.config.demo_mode {
            self.run_demo_mode(&mut terminal_manager)?;
        } else {
            let (event_channel, mut event_rx) = EventChannel::new();
            let event_tx = event_channel.sender().clone();
            let _event_handler = EventHandler::with_sender(self.config.tick_rate, event_tx.clone());
            self.run_main_loop(&mut terminal_manager, &mut event_rx, event_tx)?;
        }

        Ok(())
    }

    /// Main event loop
    fn run_main_loop(
        &mut self,
        terminal_manager: &mut TerminalManager,
        event_rx: &mut mpsc::UnboundedReceiver<TuiEvent>,
        event_tx: mpsc::UnboundedSender<TuiEvent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            // Draw UI
            terminal_manager.terminal().draw(|f| {
                crate::tui::ui::render(f, &self.app);
            })?;

            // Handle events
            match event_rx.try_recv() {
                Ok(event) => {
                    match event {
                        TuiEvent::Key(key) => {
                            match self.app.input_mode {
                                crate::tui::app::InputMode::Normal => {
                                    match key.code {
                                        KeyCode::Char('q') => {
                                            self.app.should_quit = true;
                                        }
                                        KeyCode::Char('i') => {
                                            self.app.input_mode = crate::tui::app::InputMode::Editing;
                                        }
                                        _ => {}
                                    }
                                }
                                crate::tui::app::InputMode::Editing => {
                                    match key.code {
                                        KeyCode::Char(c) => {
                                            if key.modifiers.is_empty()
                                                || key.modifiers == KeyModifiers::SHIFT
                                            {
                                                self.app.input.push(c);
                                                self.app.cursor_position += 1;
                                            }
                                        }
                                        KeyCode::Backspace => {
                                            if self.app.cursor_position > 0 {
                                                self.app.cursor_position -= 1;
                                                self.app.input.remove(self.app.cursor_position);
                                            }
                                        }
                                        KeyCode::Enter => {
                                            if !self.app.input.is_empty() {
                                                self.submit_current_input(event_tx.clone());
                                                self.app.input.clear();
                                                self.app.cursor_position = 0;
                                            }
                                        }
                                        KeyCode::Esc => {
                                            self.app.input_mode = crate::tui::app::InputMode::Normal;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        TuiEvent::Quit => {
                            self.app.should_quit = true;
                        }
                        _ => {
                            self.app.handle_event(&event);
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(16));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    break;
                }
            }

            if self.app.should_quit {
                break;
            }
        }

        Ok(())
    }

    /// Run demo mode with simulated agents
    fn run_demo_mode(
        &mut self,
        terminal_manager: &mut TerminalManager,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create demo events
        let demo_events = vec![
            TuiEvent::AgentStarted {
                id: "agent-1".to_string(),
                name: "Research Agent".to_string(),
                task: "Analyze codebase structure".to_string(),
            },
            TuiEvent::AgentStarted {
                id: "agent-2".to_string(),
                name: "Code Agent".to_string(),
                task: "Implement new feature".to_string(),
            },
            TuiEvent::AgentProgress {
                id: "agent-1".to_string(),
                node: "analyzing".to_string(),
                message: "Scanning source files...".to_string(),
            },
            TuiEvent::AgentProgress {
                id: "agent-2".to_string(),
                node: "implementing".to_string(),
                message: "Writing code...".to_string(),
            },
        ];

        // Send demo events
        for event in demo_events {
            self.app.handle_event(&event);
        }

        loop {
            // Draw UI
            terminal_manager.terminal().draw(|f| {
                crate::tui::ui::render(f, &self.app);
            })?;

            // Check for quit event
            if event::poll(self.config.tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('q') {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn submit_current_input(&mut self, event_tx: mpsc::UnboundedSender<TuiEvent>) {
        let content = self.app.input.trim().to_string();
        if content.is_empty() {
            return;
        }

        let _ = event_tx.send(TuiEvent::UserMessageAdded {
            content: content.clone(),
        });

        let thread_id = self
            .thread_id
            .get_or_insert_with(|| format!("session-{}", Uuid::new_v4()))
            .clone();
        let agent_id = format!("agent-{}", Uuid::new_v4());
        let opts = self.build_run_options(content.clone(), thread_id);

        spawn_agent_run(
            event_tx,
            Arc::clone(&self.backend),
            opts,
            RunCmd::React,
            agent_id,
            content,
        );
    }

    fn build_run_options(&self, message: String, thread_id: String) -> RunOptions {
        RunOptions {
            message,
            working_folder: self.config.working_folder.clone(),
            session_id: None,
            cancellation: None,
            thread_id: Some(thread_id),
            role_file: self.config.role_file.clone(),
            agent: self.config.agent.clone(),
            verbose: self.config.verbose,
            got_adaptive: false,
            display_max_len: 200,
            output_json: true,
            model: self.config.model.clone(),
            mcp_config_path: self.config.mcp_config_path.clone(),
            output_timestamp: false,
            dry_run: self.config.dry_run,
            provider: None,
            base_url: None,
            api_key: None,
            provider_type: None,
        }
    }
}
