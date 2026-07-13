//! Agent run orchestration: config building + runner execution.

pub mod config_builder;
pub mod profile_helper;
pub mod runner;
pub mod types;

pub use config_builder::{build_react_config, load_memory_prompt, resolve_model_config};
pub use runner::{
    build_runner, run_agent_from_config, run_agent_from_config_traced, AnyRunner, RunCmd, RunError,
    RunParams, TypedAnyStreamEvent,
};
pub use types::{
    AgentRunResult, ExtraToolsProvider, ResolvedModelConfig, RunCompletion, RunOptions,
    DEFAULT_WORKING_FOLDER,
};
