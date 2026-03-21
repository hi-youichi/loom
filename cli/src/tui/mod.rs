//! TUI (Terminal User Interface) for displaying concurrent agents.
//! 
//! Uses ratatui to render a dashboard showing multiple agents running concurrently.

mod app;
mod event;
mod runtime;
mod runner;
mod terminal;
mod ui;

// Models
pub mod models;

pub use app::{AgentInfo, AgentStatus, App, AppState, InputMode};
pub use event::{EventChannel, EventHandler, TuiEvent};
pub use runner::{TuiConfig, TuiRunner};
pub use terminal::TerminalManager;
