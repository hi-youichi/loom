//! Helve: product-semantic configuration and system prompt assembly for file-tool ReAct runs.
//!
//! This module provides **only** types and pure functions: no server, TUI, or I/O. Callers
//! (e.g. a CLI) parse request/args into [`HelveConfig`], then use
//! [`to_react_build_config`] to obtain a full [`ReactBuildConfig`](crate::agent::react::ReactBuildConfig)
//! for [`build_react_runner`](crate::agent::react::build_react_runner). System prompt text is
//! built via [`assemble_system_prompt`] when a working folder is set.
//!
//! ## Purpose
//!
//! - **Separation of concerns**: Product meaning (working folder, thread, user, approval policy)
//!   lives in [`HelveConfig`]; infra (DB, MCP, OpenAI) stays in
//!   [`ReactBuildConfig`](crate::agent::react::ReactBuildConfig). The server merges them with
//!   [`to_react_build_config`].
//! - **Single source of prompt copy**: [`assemble_system_prompt`] centralises workdir and
//!   approval instructions so the ReAct layer does not embed product wording.
//!
//! ## Main types and functions
//!
//! | Item | Role |
//! |------|-----|
//! | [`HelveConfig`] | Product config: `working_folder`, `thread_id`, `user_id`, `approval_policy`, `role_setting`, `system_prompt_override`. Built from request/CLI. |
//! | [`to_react_build_config`] | Merges `HelveConfig` with a base `ReactBuildConfig`; sets `system_prompt` from override, or [`role_setting`](HelveConfig::role_setting) + [`assemble_system_prompt`]. |
//! | [`assemble_system_prompt`] | Builds full system prompt: base ReAct prompt + workdir path + file rules + optional approval text. |
//! | [`ApprovalPolicy`] | `None` / `DestructiveOnly` / `Always`; controls which tools require user confirmation. |
//! | [`tools_requiring_approval`] | Returns tool names that need approval for a given policy; used by [`ActNode`](crate::agent::react::ActNode) to trigger interrupts. |
//! | [`APPROVAL_REQUIRED_EVENT_TYPE`] | Stream/interrupt event type string; clients use it to show approval UI and resume with `approved` payload. |
//!
//! ## Interaction with other modules
//!
//! - **agent::react**: [`to_react_build_config`] produces [`ReactBuildConfig`](crate::agent::react::ReactBuildConfig); that config is passed to [`build_react_runner`](crate::agent::react::build_react_runner).
//! - **agent::react**: [`ActNode`](crate::agent::react::ActNode) uses [`ApprovalPolicy`] and [`tools_requiring_approval`] to decide when to interrupt before running a tool; [`REACT_SYSTEM_PROMPT`](crate::agent::react::REACT_SYSTEM_PROMPT) is the base for [`assemble_system_prompt`].
//! - **openai_sse**: [`ParsedChatRequest`](crate::openai_sse::ParsedChatRequest) may carry optional `helve_config` parsed from request body (`working_folder`, `approval_policy`, etc.).
//!
//! ## Internal structure
//!
//! - **config**: [`HelveConfig`], [`to_react_build_config`].
//! - **prompt**: [`assemble_system_prompt`], [`ApprovalPolicy`], [`tools_requiring_approval`], [`APPROVAL_REQUIRED_EVENT_TYPE`].

mod config;
mod prompt;

pub use config::{to_react_build_config, HelveConfig};
pub use prompt::{
    assemble_system_prompt, assemble_system_prompt_with_prompts, tools_requiring_approval,
    ApprovalPolicy, APPROVAL_REQUIRED_EVENT_TYPE,
};
