//! System prompt assembly: working folder path, permission rules.
//!
//! All prompt materials are loaded elsewhere and assembled through the single
//! main entry point [`assemble_system_prompt`].

use crate::inputs::SystemPromptInputs;
use crate::prompts::constants::REACT_SYSTEM_PROMPT;

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
