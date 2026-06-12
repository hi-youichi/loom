//! Agent extensions for Loom.
//!
//! This crate contains advanced agent patterns and extensions including:
//! - DUP (Debate, Update, Predict) agent pattern
//! - ToT (Tree of Thoughts) agent pattern  
//! - GoT (Graph of Thoughts) agent pattern

pub mod dup;
pub mod got;
pub mod tot;

// Re-export extension types at crate root
pub use dup::{
    DupRunner, DupState, DupRunError,
    build_dup_runner, build_dup_initial_state,
    UnderstandOutput, DUP_UNDERSTAND_PROMPT,
};

pub use got::{
    GotRunner, GotState, GotRunError,
    build_got_runner, build_got_initial_state,
    TaskGraph, TaskNode, TaskNodeState, TaskStatus,
};

pub use tot::{
    TotRunner, TotState, TotRunError,
    build_tot_runner, build_tot_initial_state,
    TotCandidate, TotExtension,
};
