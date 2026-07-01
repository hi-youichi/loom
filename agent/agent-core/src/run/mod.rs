//! Agent run orchestration: config building + runner execution.

pub mod config_builder;
pub mod profile_helper;
pub mod runner;
pub mod types;

pub use config_builder::{build_react_config, resolve_model_config, load_memory_prompt};
pub use runner::{
    run_agent_from_config, run_agent_from_config_traced,
    build_runner, AnyRunner, RunCmd, RunError, RunParams,
    TypedAnyStreamEvent, to_loom_any_stream_event,
};
pub use types::{
    AgentRunResult, DEFAULT_WORKING_FOLDER, ResolvedModelConfig,
    RunCompletion, RunOptions,
};
