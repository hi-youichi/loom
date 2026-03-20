//! TUI Runner - Main event loop and orchestration for the TUI application.
//!
//! This module provides the main entry point for running the TUI dashboard.

use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use super::{App, EventHandler, TuiEvent, TerminalManager};

/// Configuration for the TUI runner
pub struct TuiConfig {
    /// Tick rate in milliseconds (default: 250ms)
    pub tick_rate: Duration,
    /// Whether to show demo mode with simulated agents
    pub demo_mode: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            tick_rate: Duration::from_millis(250),
            demo_mode: false,
        }
    }
}

/// Main runner for the TUI application
pub struct TuiRunner {
    app: App,
    config: TuiConfig,
}

impl TuiRunner {
    /// Create a new TUI runner
    pub fn new(config: TuiConfig) -> Self {
        Self {
            app: App::new(),
            config,
        }
    }

    /// Run the TUI application
    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Setup terminal
        let mut terminal_manager = TerminalManager::new()?;
        terminal_manager.enable_raw_mode();
        terminal_manager.enter_alternate_screen();
        
        let terminal = terminal_manager.terminal();

        // Create event channel for external events
        let (event_sender, mut event_receiver) = mpsc::unbounded_channel();

        // Start event handler to capture keyboard events
        // Start event handler to capture keyboard events
        let _event_handler = EventHandler::with_sender(self.config.tick_rate, event_sender.clone());

        // If demo mode, spawn a task to generate demo events
        let demo_task = if self.config.demo_mode {
            Some(tokio::spawn(run_demo_mode(event_sender)))
        } else {
            None
        };

        // Main event loop
        let res = self.main_loop(terminal, &mut event_receiver);

        // Wait for demo task to finish (if running)
        if let Some(task) = demo_task {
            task.abort();
        }

        // Restore terminal
        drop(terminal_manager);

        res
    }

    /// Main event loop
    fn main_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        event_receiver: &mut mpsc::UnboundedReceiver<TuiEvent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        loop {
            // Draw the UI
            terminal.draw(|f| {
                super::ui::render(&self.app, f);
            })?;

            // Handle events from channel (includes keyboard and tick events from EventHandler)
            while let Ok(event) = event_receiver.try_recv() {
                self.app.handle_event(event);
            }

            // Check if we should quit
            if self.app.should_quit {
                break;
            }
        }

        Ok(())
    }
}

/// Run demo mode with simulated agents
async fn run_demo_mode(sender: mpsc::UnboundedSender<TuiEvent>) {
    use std::time::Duration;
    use tokio::time::sleep;

    // Simulate a few agents starting
    let agents = vec![
        ("dev-agent-1", "dev", "Implement user authentication feature"),
        ("code-review", "review", "Review PR #123 for security issues"),
        ("test-runner", "test", "Run integration test suite"),
    ];

    let mut agent_ids = Vec::new();

    // Start agents with delays
    for (i, (name, agent_type, task)) in agents.into_iter().enumerate() {
        sleep(Duration::from_millis(500 * (i as u64 + 1))).await;
        
        let id = format!("{}-{}", name, chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        agent_ids.push(id.clone());

        let _ = sender.send(TuiEvent::AgentStarted {
            id: id.clone(),
            name: name.to_string(),
            task: task.to_string(),
        });

        // Simulate progress updates
        let sender_clone = sender.clone();
        let id_clone = id.clone();
        tokio::spawn(async move {
            sleep(Duration::from_secs(2)).await;
            let _ = sender_clone.send(TuiEvent::AgentProgress {
                id: id_clone,
                node: "processing".to_string(),
                message: "Working on task...".to_string(),
            });
        });
    }

    // Wait a bit, then complete some agents
    sleep(Duration::from_secs(5)).await;
    
    if let Some(id) = agent_ids.first() {
        let _ = sender.send(TuiEvent::AgentCompleted {
            id: id.clone(),
            result: "Task completed successfully!".to_string(),
        });
    }
}
