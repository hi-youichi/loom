//! [`SystemPromptInputs`] — borrowed parameter bundle for [`assemble_system_prompt`](super::assemble::assemble_system_prompt).

use std::path::Path;

use crate::env_context::EnvContext;

/// Borrowed inputs for one system-prompt assembly call.
///
/// Unlike the old `HelveConfig` (which was owned and carried storage responsibility),
/// this type is a transient borrow: callers construct it inline from `ReactBuildConfig`
/// fields, pass it to [`assemble_system_prompt`](super::assemble_system_prompt), and discard it.
#[derive(Debug, Clone, Default)]
pub struct SystemPromptInputs<'a> {
    /// When set, overrides the entire final prompt and bypasses all assembly.
    pub full_override: Option<&'a str>,
    /// Optional base prompt content that replaces the default `REACT_SYSTEM_PROMPT`.
    pub base_prompt_override: Option<&'a str>,
    /// Optional role/persona section prepended before the base content.
    pub role_setting: Option<&'a str>,
    /// Optional project rules (e.g. from `AGENTS.md`) prepended after `role_setting`.
    pub agents_md: Option<&'a str>,
    /// Optional skills section prepended after `agents_md`.
    pub skills_prompt: Option<&'a str>,
    /// Optional memory context prepended after `skills_prompt`.
    pub memory_prompt: Option<&'a str>,
    /// Optional runtime environment context prepended before all other sections.
    pub env_context: Option<&'a EnvContext>,
    /// Working folder displayed in the workdir section when present.
    pub working_folder: Option<&'a Path>,
}
