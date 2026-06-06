//! Shared types for the Loom agent framework.
//!
//! This crate contains pure data types and traits shared between
//! "loom", "loom-agent", and other workspace crates.
//! No heavy dependencies (no MCP, no rusqlite, no lancedb).

pub mod approval;
pub mod cli_run;
pub mod config;
pub mod state;
pub mod command;
pub mod prompts;
