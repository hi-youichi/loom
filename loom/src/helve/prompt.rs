//! System prompt assembly for Helve: working folder path, permission rules, optional approval.
//!
//! Used by Server (or CLI) to build `ReactBuildConfig.system_prompt` without embedding
//! product copy in React.
//! When using file-based prompts, use [`assemble_system_prompt_with_prompts`] with [`crate::prompts::AgentPrompts`].

use std::path::Path;

use crate::agent::react::REACT_SYSTEM_PROMPT;
use crate::prompts::AgentPrompts;

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
    let workdir_display = working_folder
        .canonicalize()
        .unwrap_or_else(|_| working_folder.to_path_buf())
        .display()
        .to_string();
    let workdir_section = format!(
        //         r#"
        // WORKING FOLDER & FILE RULES:
        // - Working folder path: {}
        // - You may ONLY use the provided file tools (ls, read, write_file, move_file, delete_file, create_dir) to operate inside this directory and its subdirectories.
        // - Do NOT access paths outside the working folder. Any path you use must be under the above folder.
        // - EXPLORE FIRST: When the user asks about the project, codebase, or any contents of the working folder (e.g. "what is this project?", "what files are here?", "describe the code"), you MUST call ls first to explore the structure, then read relevant files (README, config files, etc.) before answering. Never guess or ask the user for more context when the information is available in the working folder.
        // - FILE OUTPUT: When the user explicitly asks you to write/save a document, report, or content to a file (e.g., "write to file", "save to file", "write a report to file"), you MUST call write_file to save the content BEFORE giving FINAL_ANSWER. Do NOT give FINAL_ANSWER with only text—call write_file first, then report the file path in your final answer."#,
        r#"
WORKING FOLDER & FILE RULES:
- Working folder path: {}
"#,
        workdir_display
    );

    let approval_section = match approval_policy {
        Some(ApprovalPolicy::None) | None => String::new(),
        Some(ApprovalPolicy::DestructiveOnly) => "\n\nAPPROVAL: Before executing delete_file or remove_dir, output your plan and wait for the user to confirm (e.g. \"Proceed?\" or \"Continue?\"). Do not perform the deletion until the user approves.".to_string(),
        Some(ApprovalPolicy::Always) => "\n\nAPPROVAL: Before executing delete_file, remove_dir, or bulk write_file operations, output your plan and wait for the user to confirm. Do not perform these operations until the user approves.".to_string(),
    };

    format!(
        "{}{}{}",
        REACT_SYSTEM_PROMPT, workdir_section, approval_section
    )
}

/// Assembles the full system prompt using [`AgentPrompts`] (base, workdir template, approval).
///
/// Use when prompts are loaded from YAML (e.g. [`crate::prompts::load_or_default`]). Replaces
/// `{workdir}` in the Helve workdir template with the display path of `working_folder`.
pub fn assemble_system_prompt_with_prompts(
    working_folder: &Path,
    approval_policy: Option<ApprovalPolicy>,
    prompts: &AgentPrompts,
) -> String {
    let workdir_display = working_folder
        .canonicalize()
        .unwrap_or_else(|_| working_folder.to_path_buf())
        .display()
        .to_string();
    let workdir_section = prompts
        .helve_workdir_section_template()
        .replace("{workdir}", &workdir_display);
    let approval_section = match approval_policy {
        Some(ApprovalPolicy::None) | None => String::new(),
        Some(ApprovalPolicy::DestructiveOnly) => prompts.helve_approval_destructive(),
        Some(ApprovalPolicy::Always) => prompts.helve_approval_always(),
    };
    format!(
        "{}{}{}",
        prompts.react_system_prompt(),
        workdir_section,
        approval_section
    )
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

    /// assemble_system_prompt_with_prompts uses prompts and replaces {workdir}.
    #[test]
    fn assemble_system_prompt_with_prompts_includes_workdir_and_base() {
        let prompts = crate::prompts::AgentPrompts::default();
        let p = assemble_system_prompt_with_prompts(Path::new("/tmp/ws"), None, &prompts);
        assert!(p.contains(REACT_SYSTEM_PROMPT));
        assert!(p.contains("/tmp/ws"));
        assert!(p.contains("Working folder path"));
    }
}
