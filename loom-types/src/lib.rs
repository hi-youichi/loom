//! Shared types for the Loom agent framework.
//!
//! This crate contains pure data types and traits shared between
//! "loom", "loom-agent", and other workspace crates.
//! No heavy dependencies (no MCP, no rusqlite, no lancedb).



pub mod cli_run;
pub mod config;
pub mod state;
pub mod command;
pub mod prompts;
pub mod tool_output_normalizer;
pub mod active_operation;
pub mod tools;
