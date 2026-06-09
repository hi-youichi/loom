//! GoT (Graph of Thoughts) graph and runner.
//!
//! User task → PlanGraph (LLM produces DAG) → ExecuteGraph (run nodes in order).
//! Each task node runs as a ReAct sub-task. Optional AGoT: adaptive expansion.
//!
//! Identical submodules are re-exported from loom_agent_patterns.
//! Custom files (adaptive, execute_engine, plan_node, runner, state) remain local.

pub use loom_agent_patterns::agent::got::dag;
pub use loom_agent_patterns::agent::got::prompt;

mod adaptive;
mod execute_engine;
mod plan_node;
mod runner;
mod state;

pub use dag::{append_subgraph, AppendSubgraphError};
pub use prompt::{AGOT_EXPAND_SYSTEM, GOT_PLAN_SYSTEM};
pub use runner::{build_got_initial_state, GotRunError, GotRunner};
pub use state::{GotState, TaskGraph, TaskNode, TaskNodeState, TaskStatus};