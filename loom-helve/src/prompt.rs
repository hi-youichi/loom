//! System prompt assembly for Helve: working folder path, permission rules.
//!
//! Used by Server (or CLI) to build `ReactBuildConfig.system_prompt` without embedding
//! product copy in React. All prompt materials should be loaded elsewhere and assembled
//! through the single main entry point in this module.

use std::path::{Path, PathBuf};

use loom_types::prompts::REACT_SYSTEM_PROMPT;
use crate::env_context::EnvContext;



/// Raw materials used to assemble the final ReAct system prompt.
///
/// This type intentionally stores *inputs*, not the final string:
/// loading happens in callers, while prompt assembly happens in
/// [`assemble_react_system_prompt`].
#[derive(Debug, Clone, Default)]
pub struct ReactPromptInputs {
    /// When set, overrides the entire final prompt and bypasses all assembly.
    pub full_override: Option<String>,
    /// Optional base prompt content that replaces [`REACT_SYSTEM_PROMPT`] before
    /// workdir section is appended.
    pub base_prompt_override: Option<String>,
    /// Optional role/persona section prepended before the base content.
    pub role_setting: Option<String>,
    /// Optional project rules (for example from `AGENTS.md`) prepended after `role_setting`.
    pub agents_md: Option<String>,
    /// Optional skills section prepended after `agents_md`.
    pub skills_prompt: Option<String>,
    /// Optional memory context (user preferences, project facts) prepended after `skills_prompt`.
    pub memory_prompt: Option<String>,
    /// Optional runtime environment context (OS, locale, agent intro) prepended before all other sections.
    pub env_context: Option<EnvContext>,
    /// Working folder displayed in the workdir section when present.
    pub working_folder: Option<PathBuf>,
}


fn canonical_display(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn build_workdir_section(working_folder: Option<&Path>) -> String {
    let Some(path) = working_folder else {
        return String::new();
    };
    format!(
        r#"
WORKING FOLDER & FILE RULES:
- Working folder path: {}
"#,
        canonical_display(path)
    )
}



fn push_trimmed(sections: &mut Vec<String>, opt: Option<&String>) {
    if let Some(s) = opt {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            sections.push(trimmed.to_string());
        }
    }
}

fn collect_prefix_sections(inputs: &ReactPromptInputs) -> Vec<String> {
    let mut sections = Vec::new();
    if let Some(ctx) = &inputs.env_context {
        let s = ctx.to_prompt_section();
        if !s.trim().is_empty() {
            sections.push(s);
        }
    }
    push_trimmed(&mut sections, inputs.role_setting.as_ref());
    push_trimmed(&mut sections, inputs.agents_md.as_ref());
    push_trimmed(&mut sections, inputs.skills_prompt.as_ref());
    push_trimmed(&mut sections, inputs.memory_prompt.as_ref());
    sections
}

/// Assembles the final ReAct system prompt from loaded prompt materials.
///
/// This is the single main prompt assembly path for ReAct. Callers should gather
/// inputs first, then pass them here to produce the final prompt string.
pub fn assemble_react_system_prompt(inputs: &ReactPromptInputs) -> String {
    if let Some(full) = &inputs.full_override {
        return full.clone();
    }

    let base_prompt = inputs
        .base_prompt_override
        .clone()
        .unwrap_or_else(|| REACT_SYSTEM_PROMPT.to_string());
    let base_content = format!(
        "{}{}",
        base_prompt,
        build_workdir_section(inputs.working_folder.as_deref())
    );

    let prefix_sections = collect_prefix_sections(inputs);
    if prefix_sections.is_empty() {
        base_content
    } else if base_content.is_empty() {
        prefix_sections.join("\n\n")
    } else {
        format!("{}\n\n{}", prefix_sections.join("\n\n"), base_content)
    }
}

/// Assembles the full system prompt for a Helve-style run: base ReAct prompt plus
/// working folder path and permission rules.
///
/// Callers (e.g. Server) pass the result to `ReactBuildConfig.system_prompt`.
/// Does not perform I/O; `working_folder` is used only as display path in the prompt.
///
/// # Arguments
///
/// * `working_folder` - Path to the working directory (shown in the prompt; need not exist yet).
///
/// # Example
///
/// ```ignore
/// use loom::helve::assemble_system_prompt;
/// use std::path::Path;
///
/// let prompt = assemble_system_prompt(Path::new("/tmp/workspace"));
/// config.system_prompt = Some(prompt);
/// ```
pub fn assemble_system_prompt(
    working_folder: &Path,
) -> String {
    assemble_react_system_prompt(&ReactPromptInputs {
        working_folder: Some(working_folder.to_path_buf()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_system_prompt_includes_workdir_and_base() {
        let p = assemble_system_prompt(Path::new("/tmp/ws"));
        assert!(p.contains(REACT_SYSTEM_PROMPT));
        assert!(p.contains("/tmp/ws"));
        assert!(p.contains("Working folder path"));
    }



    #[test]
    fn assemble_react_system_prompt_assembles_prefix_and_sections() {
        let p = assemble_react_system_prompt(&ReactPromptInputs {
            role_setting: Some("You are helpful.".to_string()),
            agents_md: Some("Project rules.".to_string()),
            skills_prompt: Some("Available skills.".to_string()),
            working_folder: Some(PathBuf::from("/tmp/ws")),
            ..Default::default()
        });
        assert!(p.starts_with("You are helpful."));
        assert!(p.contains("Project rules."));
        assert!(p.contains("Available skills."));
        assert!(p.contains(REACT_SYSTEM_PROMPT));
        assert!(p.contains("/tmp/ws"));
    }

    #[test]
    fn env_context_prepended_before_role_setting() {
        let ctx = EnvContext::default();
        let p = assemble_react_system_prompt(&ReactPromptInputs {
            env_context: Some(ctx),
            role_setting: Some("You are helpful.".to_string()),
            working_folder: Some(PathBuf::from("/tmp/ws")),
            ..Default::default()
        });
        assert!(p.starts_with("ENVIRONMENT:"));
        assert!(p.contains("You are helpful."));
        let env_pos = p.find("ENVIRONMENT:").unwrap();
        let role_pos = p.find("You are helpful.").unwrap();
        let workdir_pos = p.find("/tmp/ws").unwrap();
        assert!(env_pos < role_pos && role_pos < workdir_pos);
    }

    #[test]
    fn env_context_to_prompt_section_contains_os_and_agent() {
        let ctx = super::EnvContext::detect();
        let section = ctx.to_prompt_section();
        assert!(section.contains("OS:"));
        assert!(section.contains("Locale:"));
        assert!(section.contains("Agent: Loom"));
        assert!(section.starts_with("ENVIRONMENT:"));
    }
}