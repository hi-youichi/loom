//! Resolves effective prompt strings from loaded config and code defaults.
//!
//! [`AgentPrompts`] holds loaded YAML prompt materials. For non-ReAct agent families,
//! getters resolve from loaded values to code defaults. ReAct prompt assembly now lives
//! in the single main assembler path under [`crate::prompt`].

use crate::prompts::constants::{DUP_UNDERSTAND_PROMPT, AGOT_EXPAND_SYSTEM, GOT_PLAN_SYSTEM, TOT_EXPAND_SYSTEM_ADDON, TOT_RESEARCH_QUALITY_ADDON};

use super::{DupPromptsFile, GotPromptsFile, PromptOverridesFile, ReactPromptsFile, TotPromptsFile};

/// Loaded YAML prompt materials for all agent patterns.
///
/// Build via [`load`](super::load) or [`load_or_default`](super::load_or_default).
/// ReAct prompt materials are loaded here but assembled elsewhere.
#[derive(Clone, Debug, Default)]
pub struct AgentPrompts {
    pub react: ReactPromptsFile,
    pub tot: TotPromptsFile,
    pub got: GotPromptsFile,
    pub dup: DupPromptsFile,
    pub prompt_overrides: PromptOverridesFile,
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
            .unwrap_or_else(|| GOT_PLAN_SYSTEM.trim().to_string())
    }

    /// AGoT expand node system prompt (output child JSON list).
    pub fn got_agot_expand_system(&self) -> String {
        self.got
            .agot_expand_system
            .clone()
            .unwrap_or_else(|| AGOT_EXPAND_SYSTEM.trim().to_string())
    }

    /// DUP understand prompt (how to decompose tasks).
    pub fn dup_understand_prompt(&self) -> String {
        self.dup
            .understand_prompt
            .clone()
            .unwrap_or_else(|| DUP_UNDERSTAND_PROMPT.trim().to_string())
    }

    /// System addon (workdir rules addon).
    pub fn system_addon(&self) -> Option<String> {
        self.prompt_overrides.system_addon.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tot_expand_system_addon_resolves_from_loaded_or_default() {
        let prompts = AgentPrompts {
            tot: TotPromptsFile {
                expand_system_addon: Some("custom addon".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(prompts.tot_expand_system_addon(), "custom addon");

        let prompts_default = AgentPrompts::default();
        assert!(!prompts_default.tot_expand_system_addon().is_empty());
    }

    #[test]
    fn tot_research_quality_addon_resolves_from_loaded_or_default() {
        let prompts = AgentPrompts {
            tot: TotPromptsFile {
                research_quality_addon: Some("custom quality".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(prompts.tot_research_quality_addon(), "custom quality");

        let prompts_default = AgentPrompts::default();
        assert!(!prompts_default.tot_research_quality_addon().is_empty());
    }

    #[test]
    fn got_plan_system_resolves_from_loaded_or_default() {
        let prompts = AgentPrompts {
            got: GotPromptsFile {
                plan_system: Some("custom plan".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(prompts.got_plan_system(), "custom plan");

        let prompts_default = AgentPrompts::default();
        assert!(!prompts_default.got_plan_system().is_empty());
    }

    #[test]
    fn got_agot_expand_system_resolves_from_loaded_or_default() {
        let prompts = AgentPrompts {
            got: GotPromptsFile {
                agot_expand_system: Some("custom expand".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(prompts.got_agot_expand_system(), "custom expand");

        let prompts_default = AgentPrompts::default();
        assert!(!prompts_default.got_agot_expand_system().is_empty());
    }

    #[test]
    fn dup_understand_prompt_resolves_from_loaded_or_default() {
        let prompts = AgentPrompts {
            dup: DupPromptsFile {
                understand_prompt: Some("custom understand".to_string()),
            },
            ..Default::default()
        };
        assert_eq!(prompts.dup_understand_prompt(), "custom understand");

        let prompts_default = AgentPrompts::default();
        assert!(!prompts_default.dup_understand_prompt().is_empty());
    }

    #[test]
    fn system_addon_resolves_only_from_loaded() {
        let prompts = AgentPrompts {
            prompt_overrides: PromptOverridesFile {
                system_addon: Some("system addon".to_string()),
            },
            ..Default::default()
        };
        assert_eq!(prompts.system_addon(), Some("system addon".to_string()));

        let prompts_default = AgentPrompts::default();
        assert!(prompts_default.system_addon().is_none());
    }
}