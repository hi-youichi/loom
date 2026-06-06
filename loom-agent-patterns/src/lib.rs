//! Agent pattern implementations for Loom.
//!
//! This crate contains ReAct, DUP, ToT, and GoT agent patterns extracted from loom.
//! These are the core agent execution patterns that can be used with the loom framework.
//!
//! # Main Components
//!
//! - [`agent`]: ReAct, DUP, ToT, and GoT agent implementations
//! - [`runner_common`]: Shared streaming execution logic
//! - [`tools`]: Agent invocation tools

pub mod agent;
pub mod runner_common;
pub mod tools;

// Re-export from runner_common
pub use runner_common::{
    load_from_checkpoint_or_build, StreamRunOutcome, StreamRunError, run_stream_with_config,
};

// Re-export from agent::react::build
pub use agent::react::build::{build_react_run_context, BuildRunnerError};
