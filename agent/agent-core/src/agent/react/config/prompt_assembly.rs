//! System prompt assembly: working folder path, permission rules.
//!
//! All prompt materials are loaded elsewhere and assembled through the single
//! main entry point [`assemble_system_prompt`].

use std::path::Path;

use crate::EnvContext;

/// Default ReAct base system prompt when no `react.yaml` / `REACT_SYSTEM_PROMPT` override.
///
/// The previous RULES/PHASES block is **disabled**: it conflicts with `tool_choice: required`
/// and with tasks that need real workspace listing without hallucination.
pub const REACT_SYSTEM_PROMPT: &str = "";

/// Borrowed inputs for one system-prompt assembly call.
///
/// This type is a transient borrow: callers construct it inline from `ReactBuildConfig`
/// fields, pass it to [`assemble_system_prompt`], and discard it.
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

/// Assembles the final system prompt from borrowed inputs.
///
/// **Order of assembly**:
/// 1. `env_context` (prepended)
/// 2. `role_setting` (prepended)
/// 3. `agents_md` (appended after role_setting)
/// 4. `skills_prompt` (appended after agents_md)
/// 5. `memory_prompt` (appended after skills_prompt)
/// 6. Base prompt content (REACT_SYSTEM_PROMPT or base_prompt_override)
/// 7. Workdir section (when working_folder is present)
pub fn assemble_system_prompt(inputs: &SystemPromptInputs<'_>) -> String {
    // Full override bypasses all assembly
    if let Some(full) = inputs.full_override {
        return full.to_string();
    }

    let mut parts = Vec::new();

    // 1. Env context (prepended)
    if let Some(ctx) = inputs.env_context {
        parts.push(ctx.to_prompt_section());
    }

    // 2. Role setting
    if let Some(role) = inputs.role_setting {
        parts.push(role.to_string());
    }

    // 3. Agents.md
    if let Some(agents) = inputs.agents_md {
        parts.push(agents.to_string());
    }

    // 4. Skills prompt
    if let Some(skills) = inputs.skills_prompt {
        parts.push(skills.to_string());
    }

    // 5. Memory prompt
    if let Some(memory) = inputs.memory_prompt {
        parts.push(memory.to_string());
    }

    // 6. Base prompt content
    let base_content = inputs
        .base_prompt_override
        .unwrap_or(REACT_SYSTEM_PROMPT);

    // 7. Workdir section (when working_folder is present)
    let final_prompt = if let Some(workdir) = inputs.working_folder {
        format!(
            "{}\n\n--- WORKDIR RULES ---\nCurrent working directory: {}\nFile tools are scoped to this path and have read/write/execute permissions.\nDo not modify system files or cause file system damage.\n",
            base_content,
            workdir.display()
        )
    } else {
        base_content.to_string()
    };

    // Join all parts with newlines
    let mut full_prompt = parts.join("\n\n");
    if !full_prompt.is_empty() {
        full_prompt.push_str("\n\n");
        full_prompt.push_str(&final_prompt);
    } else {
        full_prompt = final_prompt;
    }

    full_prompt
}
