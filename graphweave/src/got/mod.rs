//! GoT (Graph of Thoughts) graph and runner.
//!
//! User task → PlanGraph (LLM produces DAG) → ExecuteGraph (run nodes in order).
//! Each task node runs as a ReAct sub-task. Optional AGoT: adaptive expansion.

mod adaptive;
mod dag;
mod execute_engine;
mod plan_node;
mod prompt;
mod runner;
mod state;

pub use dag::{append_subgraph, AppendSubgraphError};
pub use prompt::{AGOT_EXPAND_SYSTEM, GOT_PLAN_SYSTEM};
pub use runner::{build_got_initial_state, GotRunError, GotRunner};
pub use state::{GotState, TaskGraph, TaskNode, TaskNodeState, TaskStatus};
