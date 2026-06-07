//! Utility functions and types for Loom TUI

pub mod error;
pub mod terminal;
pub mod events;
pub mod colors;

pub use error::{Result, LoomTuiError};
pub use terminal::{setup_terminal, TerminalCleanup};
pub use events::{EventProcessor, KeyEventHandler};
pub use colors::{ColorScheme, ColorPalette};