//! GoT (Graph of Thoughts) graph and runner.
//!
//! User task → PlanGraph (LLM produces DAG) → ExecuteGraph (run nodes in order).
//! Each task node runs as a ReAct sub-task. Optional AGoT: adaptive expansion.
//!
//! Custom files (adaptive, execute_engine, plan_node, runner, state, dag, prompt) remain local.

mod adaptive;
mod execute_engine;
mod plan_node;
mod runner;
mod state;

mod dag;
pub mod build;
mod prompt;

pub use dag::{append_subgraph, AppendSubgraphError};
pub use prompt::{AGOT_EXPAND_SYSTEM, GOT_PLAN_SYSTEM};
pub use runner::{build_got_initial_state, GotRunError, GotRunner};
pub use state::{GotState, TaskGraph, TaskNode, TaskNodeState, TaskStatus};
pub use build::build_got_runner;