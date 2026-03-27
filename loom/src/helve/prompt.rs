//! System prompt assembly for Helve: working folder path, permission rules, optional approval.
//!
//! Used by Server (or CLI) to build `ReactBuildConfig.system_prompt` without embedding
//! product copy in React. All prompt materials should be loaded elsewhere and assembled
//! through the single main entry point in this module.

use std::path::{Path, PathBuf};

use crate::agent::react::REACT_SYSTEM_PROMPT;

/// Approval policy for destructive or high-risk file operations.
///
/// When not `None`, the assembled prompt instructs the agent to output a plan
/// and wait for user confirmation before executing certain operations (e.g. delete, bulk write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// No approval; agent may execute all file operations.
    None,
    /// Require approval only for destructive operations (e.g. delete_file).
    DestructiveOnly,
    /// Require approval for destructive and bulk write operations before executing.
    Always,
}

/// Event type for Custom stream events and Interrupt value when approval is required.
/// Server or clients can show an approval UI and resume with `{ "approved": true }` or `{ "approved": false }`.
pub const APPROVAL_REQUIRED_EVENT_TYPE: &str = "approval_required";

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
    /// workdir/approval sections are appended.
    pub base_prompt_override: Option<String>,
    /// Optional role/persona section prepended before the base content.
    pub role_setting: Option<String>,
    /// Optional project rules (for example from `AGENTS.md`) prepended after `role_setting`.
    pub agents_md: Option<String>,
    /// Optional skills section prepended after `agents_md`.
    pub skills_prompt: Option<String>,
    /// Working folder displayed in the workdir section when present.
    pub working_folder: Option<PathBuf>,
    /// Approval policy appended after the workdir section when present.
    pub approval_policy: Option<ApprovalPolicy>,
}

/// Returns the list of tool names that require user approval for the given policy.
///
/// - `DestructiveOnly`: delete_file (and remove_dir if present).
/// - `Always`: delete_file, write_file (and remove_dir if present).
/// - `None`: empty (no tools require approval).
///
/// Used by ActNode to decide whether to interrupt before executing a tool.
pub fn tools_requiring_approval(policy: ApprovalPolicy) -> &'static [&'static str] {
    match policy {
        ApprovalPolicy::None => &[],
        ApprovalPolicy::DestructiveOnly => &["delete_file"],
        ApprovalPolicy::Always => &["delete_file", "write_file"],
    }
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

fn build_approval_section(approval_policy: Option<ApprovalPolicy>) -> String {
    match approval_policy {
        Some(ApprovalPolicy::None) | None => String::new(),
        Some(ApprovalPolicy::DestructiveOnly) => "\n\nAPPROVAL: Before executing delete_file or remove_dir, output your plan and wait for the user to confirm (e.g. \"Proceed?\" or \"Continue?\"). Do not perform the deletion until the user approves.".to_string(),
        Some(ApprovalPolicy::Always) => "\n\nAPPROVAL: Before executing delete_file, remove_dir, or bulk write_file operations, output your plan and wait for the user to confirm. Do not perform these operations until the user approves.".to_string(),
    }
}

fn collect_prefix_sections(inputs: &ReactPromptInputs) -> Vec<&str> {
    [
        inputs.role_setting.as_deref(),
        inputs.agents_md.as_deref(),
        inputs.skills_prompt.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .collect()
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
        "{}{}{}",
        base_prompt,
        build_workdir_section(inputs.working_folder.as_deref()),
        build_approval_section(inputs.approval_policy)
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
/// working folder path, permission rules, and optional approval instructions.
///
/// Callers (e.g. Server) pass the result to `ReactBuildConfig.system_prompt`.
/// Does not perform I/O; `working_folder` is used only as display path in the prompt.
///
/// # Arguments
///
/// * `working_folder` - Path to the working directory (shown in the prompt; need not exist yet).
/// * `approval_policy` - When `Some(p)` with `p != ApprovalPolicy::None`, appends approval instructions.
///
/// # Example
///
/// ```ignore
/// use loom::helve::{assemble_system_prompt, ApprovalPolicy};
/// use std::path::Path;
///
/// let prompt = assemble_system_prompt(Path::new("/tmp/workspace"), Some(ApprovalPolicy::DestructiveOnly));
/// config.system_prompt = Some(prompt);
/// ```
pub fn assemble_system_prompt(
    working_folder: &Path,
    approval_policy: Option<ApprovalPolicy>,
) -> String {
    assemble_react_system_prompt(&ReactPromptInputs {
        working_folder: Some(working_folder.to_path_buf()),
        approval_policy,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_system_prompt_includes_workdir_and_base() {
        let p = assemble_system_prompt(Path::new("/tmp/ws"), None);
        assert!(p.contains(REACT_SYSTEM_PROMPT));
        assert!(p.contains("/tmp/ws"));
        assert!(p.contains("Working folder path"));
    }

    #[test]
    fn assemble_system_prompt_with_approval_destructive_adds_approval_text() {
        let p = assemble_system_prompt(Path::new("/x"), Some(ApprovalPolicy::DestructiveOnly));
        assert!(p.contains("APPROVAL"));
        assert!(p.contains("delete_file"));
        assert!(p.contains("wait for the user"));
    }

    #[test]
    fn assemble_system_prompt_with_approval_none_no_approval_section() {
        let p = assemble_system_prompt(Path::new("/x"), Some(ApprovalPolicy::None));
        assert!(!p.contains("APPROVAL:"));
    }

    #[test]
    fn assemble_react_system_prompt_assembles_prefix_and_sections() {
        let p = assemble_react_system_prompt(&ReactPromptInputs {
            role_setting: Some("You are helpful.".to_string()),
            agents_md: Some("Project rules.".to_string()),
            skills_prompt: Some("Available skills.".to_string()),
            working_folder: Some(PathBuf::from("/tmp/ws")),
            approval_policy: Some(ApprovalPolicy::DestructiveOnly),
            ..Default::default()
        });
        assert!(p.starts_with("You are helpful."));
        assert!(p.contains("Project rules."));
        assert!(p.contains("Available skills."));
        assert!(p.contains(REACT_SYSTEM_PROMPT));
        assert!(p.contains("/tmp/ws"));
        assert!(p.contains("APPROVAL"));
    }
}
