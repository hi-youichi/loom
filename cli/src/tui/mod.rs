//! TUI (Terminal User Interface) for displaying concurrent agents.
//! 
//! Uses ratatui to render a dashboard showing multiple agents running concurrently.

mod app;
mod event;
mod runner;
mod terminal;
mod ui;

pub use app::{AgentInfo, AgentStatus, App, AppState};
pub use event::{EventChannel, EventHandler, TuiEvent};
pub use runner::{TuiConfig, TuiRunner};
pub use terminal::TerminalManager;
