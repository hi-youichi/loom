//! Resolves effective prompt strings from loaded config and code defaults.
//!
//! [`AgentPrompts`] holds loaded YAML prompt materials. For non-ReAct agent families,
//! getters resolve from loaded values to code defaults. ReAct prompt assembly now lives
//! in the single main assembler path under [`crate::helve`].

use crate::agent::dup::DUP_UNDERSTAND_PROMPT;
use crate::agent::got::{AGOT_EXPAND_SYSTEM, GOT_PLAN_SYSTEM};
use crate::agent::tot::{TOT_EXPAND_SYSTEM_ADDON, TOT_RESEARCH_QUALITY_ADDON};

use super::{DupPromptsFile, GotPromptsFile, HelvePromptsFile, ReactPromptsFile, TotPromptsFile};

/// Loaded YAML prompt materials for all agent patterns.
///
/// Build via [`load`](crate::prompts::load) or [`load_or_default`](crate::prompts::load_or_default).
/// ReAct prompt materials are loaded here but assembled elsewhere.
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
                //                 r#"
                // WORKING FOLDER & FILE RULES:
                // - Working folder path: {workdir}
                // - You may ONLY use the provided file tools (ls, read, write_file, move_file, delete_file, create_dir) to operate inside this directory and its subdirectories.
                // - Do NOT access paths outside the working folder. Any path you use must be under the above folder.
                // - EXPLORE FIRST: When the user asks about the project, codebase, or any contents of the working folder (e.g. "what is this project?", "what files are here?", "describe the code"), you MUST call ls first to explore the structure, then read relevant files (README, config files, etc.) before answering. Never guess or ask the user for more context when the information is available in the working folder.
                // - FILE OUTPUT: When the user explicitly asks you to write/save a document, report, or content to a file (e.g., "write to file", "save to file", "write a report to file"), you MUST call write_file to save the content BEFORE giving FINAL_ANSWER. Do NOT give FINAL_ANSWER with only text—call write_file first, then report the file path in your final answer."#
                r#"
WORKING FOLDER & FILE RULES:
- Working folder path: {workdir}
"#
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

    #[test]
    fn default_getters_match_code_defaults() {
        let p = AgentPrompts::default();
        assert_eq!(p.tot_expand_system_addon(), TOT_EXPAND_SYSTEM_ADDON.trim());
        assert_eq!(
            p.tot_research_quality_addon(),
            TOT_RESEARCH_QUALITY_ADDON.trim()
        );
        assert_eq!(p.got_plan_system(), GOT_PLAN_SYSTEM);
        assert_eq!(p.got_agot_expand_system(), AGOT_EXPAND_SYSTEM);
        assert_eq!(p.dup_understand_prompt(), DUP_UNDERSTAND_PROMPT);
    }

    #[test]
    fn custom_values_override_defaults_for_all_prompt_groups() {
        let mut p = AgentPrompts::default();
        p.tot.expand_system_addon = Some("tot expand".to_string());
        p.tot.research_quality_addon = Some("tot research".to_string());
        p.got.plan_system = Some("got plan".to_string());
        p.got.agot_expand_system = Some("got expand".to_string());
        p.dup.understand_prompt = Some("dup understand".to_string());
        p.helve.workdir_section_template = Some("WORKDIR={workdir}".to_string());
        p.helve.approval_destructive = Some("ask before delete".to_string());
        p.helve.approval_always = Some("ask always".to_string());

        assert_eq!(p.tot_expand_system_addon(), "tot expand");
        assert_eq!(p.tot_research_quality_addon(), "tot research");
        assert_eq!(p.got_plan_system(), "got plan");
        assert_eq!(p.got_agot_expand_system(), "got expand");
        assert_eq!(p.dup_understand_prompt(), "dup understand");
        assert_eq!(p.helve_workdir_section_template(), "WORKDIR={workdir}");
        assert_eq!(p.helve_approval_destructive(), "ask before delete");
        assert_eq!(p.helve_approval_always(), "ask always");
    }

    #[test]
    fn helve_defaults_include_expected_placeholders_and_guidance() {
        let p = AgentPrompts::default();
        let workdir_template = p.helve_workdir_section_template();
        assert!(workdir_template.contains("WORKING FOLDER"));
        assert!(workdir_template.contains("{workdir}"));
        assert!(p.helve_approval_destructive().contains("APPROVAL"));
        assert!(p.helve_approval_always().contains("APPROVAL"));
    }
}
