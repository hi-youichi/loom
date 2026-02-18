//! Resolves effective prompt strings from loaded config and code defaults.
//!
//! [`AgentPrompts`] holds per-pattern optional overrides; getters return the string to use
//! (override or in-code default). Used by runners and [`assemble_system_prompt`](crate::helve::assemble_system_prompt).

use crate::agent::dup::DUP_UNDERSTAND_PROMPT;
use crate::agent::got::{AGOT_EXPAND_SYSTEM, GOT_PLAN_SYSTEM};
use crate::agent::react::{
    DEFAULT_EXECUTION_ERROR_TEMPLATE, DEFAULT_TOOL_ERROR_TEMPLATE, REACT_SYSTEM_PROMPT,
};
use crate::agent::tot::{TOT_EXPAND_SYSTEM_ADDON, TOT_RESEARCH_QUALITY_ADDON};

use super::{
    DupPromptsFile, GotPromptsFile, HelvePromptsFile, ReactPromptsFile, TotPromptsFile,
};

/// Loaded and env-overridden prompts for all agent patterns. Getters resolve to code defaults when unset.
///
/// Build via [`load`](crate::prompts::load) or [`load_or_default`](crate::prompts::load_or_default).
/// Pass to [`build_react_runner`](crate::agent::react::build_react_runner) or use in
/// [`assemble_system_prompt_with_prompts`](crate::helve::assemble_system_prompt_with_prompts) to override in-code prompts.
#[derive(Clone, Debug)]
pub struct AgentPrompts {
    pub react: ReactPromptsFile,
    pub tot: TotPromptsFile,
    pub got: GotPromptsFile,
    pub dup: DupPromptsFile,
    pub helve: HelvePromptsFile,
}

impl Default for AgentPrompts {
    fn default() -> Self {
        Self {
            react: ReactPromptsFile::default(),
            tot: TotPromptsFile::default(),
            got: GotPromptsFile::default(),
            dup: DupPromptsFile::default(),
            helve: HelvePromptsFile::default(),
        }
    }
}

impl AgentPrompts {
    /// ReAct system prompt (base for ReAct/ToT/DUP and Helve assembly). Env `REACT_SYSTEM_PROMPT` overrides file.
    pub fn react_system_prompt(&self) -> String {
        std::env::var("REACT_SYSTEM_PROMPT")
            .ok()
            .or_else(|| self.react.system_prompt.clone())
            .unwrap_or_else(|| REACT_SYSTEM_PROMPT.to_string())
    }

    /// ReAct tool error template for ActNode. Placeholder: `{error}`.
    pub fn react_tool_error_template(&self) -> String {
        self.react
            .tool_error_template
            .clone()
            .unwrap_or_else(|| DEFAULT_TOOL_ERROR_TEMPLATE.to_string())
    }

    /// ReAct execution error template. Placeholders: `{tool_name}`, `{tool_kwargs}`, `{error}`.
    pub fn react_execution_error_template(&self) -> String {
        self.react
            .execution_error_template
            .clone()
            .unwrap_or_else(|| DEFAULT_EXECUTION_ERROR_TEMPLATE.to_string())
    }

    /// ToT expand node system addon.
    pub fn tot_expand_system_addon(&self) -> String {
        self.tot
            .expand_system_addon
            .clone()
            .unwrap_or_else(|| TOT_EXPAND_SYSTEM_ADDON.trim().to_string())
    }

    /// ToT research quality addon (append when research_quality_addon enabled).
    pub fn tot_research_quality_addon(&self) -> String {
        self.tot
            .research_quality_addon
            .clone()
            .unwrap_or_else(|| TOT_RESEARCH_QUALITY_ADDON.trim().to_string())
    }

    /// GoT plan node system prompt (output DAG JSON).
    pub fn got_plan_system(&self) -> String {
        self.got
            .plan_system
            .clone()
            .unwrap_or_else(|| GOT_PLAN_SYSTEM.to_string())
    }

    /// AGoT expand node system prompt.
    pub fn got_agot_expand_system(&self) -> String {
        self.got
            .agot_expand_system
            .clone()
            .unwrap_or_else(|| AGOT_EXPAND_SYSTEM.to_string())
    }

    /// DUP understand node system prompt.
    pub fn dup_understand_prompt(&self) -> String {
        self.dup
            .understand_prompt
            .clone()
            .unwrap_or_else(|| DUP_UNDERSTAND_PROMPT.to_string())
    }

    /// Helve workdir section template. Placeholder: `{workdir}`. Caller replaces with actual path.
    pub fn helve_workdir_section_template(&self) -> String {
        self.helve
            .workdir_section_template
            .clone()
            .unwrap_or_else(|| {
                // In-code default (same as helve/prompt.rs inline)
                r#"
WORKING FOLDER & FILE RULES:
- Working folder path: {workdir}
- You may ONLY use the provided file tools (ls, read_file, write_file, move_file, delete_file, create_dir) to operate inside this directory and its subdirectories.
- Do NOT access paths outside the working folder. Any path you use must be under the above folder.
- EXPLORE FIRST: When the user asks about the project, codebase, or any contents of the working folder (e.g. "what is this project?", "what files are here?", "describe the code"), you MUST call ls first to explore the structure, then read_file relevant files (README, config files, etc.) before answering. Never guess or ask the user for more context when the information is available in the working folder.
- FILE OUTPUT: When the user explicitly asks you to write/save a document, report, or content to a file (e.g., "write to file", "save to file", "write a report to file"), you MUST call write_file to save the content BEFORE giving FINAL_ANSWER. Do NOT give FINAL_ANSWER with only text—call write_file first, then report the file path in your final answer."#
                    .to_string()
            })
    }

    /// Helve approval text when policy is DestructiveOnly.
    pub fn helve_approval_destructive(&self) -> String {
        self.helve
            .approval_destructive
            .clone()
            .unwrap_or_else(|| {
                "\n\nAPPROVAL: Before executing delete_file or remove_dir, output your plan and wait for the user to confirm (e.g. \"Proceed?\" or \"Continue?\"). Do not perform the deletion until the user approves.".to_string()
            })
    }

    /// Helve approval text when policy is Always.
    pub fn helve_approval_always(&self) -> String {
        self.helve
            .approval_always
            .clone()
            .unwrap_or_else(|| {
                "\n\nAPPROVAL: Before executing delete_file, remove_dir, or bulk write_file operations, output your plan and wait for the user to confirm. Do not perform these operations until the user approves.".to_string()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default AgentPrompts returns same ReAct system prompt as in-code constant.
    #[test]
    fn default_react_system_prompt_equals_const() {
        let p = AgentPrompts::default();
        assert_eq!(p.react_system_prompt(), REACT_SYSTEM_PROMPT);
    }

    /// Default AgentPrompts returns same tool error template as in-code constant.
    #[test]
    fn default_react_tool_error_template_equals_const() {
        let p = AgentPrompts::default();
        assert_eq!(p.react_tool_error_template(), DEFAULT_TOOL_ERROR_TEMPLATE);
    }

    /// When react.system_prompt is set, react_system_prompt returns it (env unset in test).
    #[test]
    fn loaded_react_system_prompt_overrides_default() {
        let mut p = AgentPrompts::default();
        p.react.system_prompt = Some("Custom system prompt.".to_string());
        assert_eq!(p.react_system_prompt(), "Custom system prompt.");
    }
}
