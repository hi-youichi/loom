//! Unified slash command system for Loom agents.
//!
//! This crate provides:
//! - [`parse`] to detect slash commands in user input
//! - [`Command`]/[`CommandResult`] types for cross-platform command handling
//! - [`execute`]/[`execute_async`] for built-in command execution
//! - State traits: [`ResetState`], [`CompactState`], [`SummarizeState`]

pub mod builtins;
pub mod command;
pub mod parser;

// Re-exports for convenience
pub use builtins::{execute, execute_async, CompactState, ResetState, SummarizeState};
pub use command::{Command, CommandResult};
pub use parser::parse;
