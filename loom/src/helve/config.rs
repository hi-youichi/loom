//! Product-level config (HelveConfig) and conversion to ReactBuildConfig.
//!
//! Server (or CLI) can parse request body or CLI args into HelveConfig, then call
//! `to_react_build_config(helve, base)` to get a full ReactBuildConfig for
//! `build_react_runner`.

use std::path::PathBuf;

use crate::agent::react::ReactBuildConfig;
use super::prompt::{assemble_react_system_prompt, ApprovalPolicy, ReactPromptInputs};

/// Product-semantic configuration for a Helve-style run.
///
/// Holds only the fields that carry product meaning (working folder, thread, user,
/// approval policy, role setting, optional system prompt override). Convert to [`ReactBuildConfig`]
/// via [`to_react_build_config`] so that runner build (e.g. `build_react_runner`) can use it.
///
/// **Interaction**: Built by Server from request body or by CLI from args; passed to
/// `to_react_build_config` together with a base `ReactBuildConfig` (e.g. from env).
#[derive(Clone, Debug, Default)]
pub struct HelveConfig {
    /// Working folder for file tools. When set, file tools are scoped to this path
    /// and the assembled system prompt includes workdir rules.
    pub working_folder: Option<PathBuf>,
    /// Thread ID for checkpointer (conversation / run identity).
    pub thread_id: Option<String>,
    /// User ID for long-term store (namespace).
    pub user_id: Option<String>,
    /// When set, tools that require approval (e.g. delete_file) will interrupt before execution.
    pub approval_policy: Option<ApprovalPolicy>,
    /// Role/persona setting (e.g. from instructions.md): prepended to the assembled system prompt.
    /// E.g. "You are a code review expert." Does not apply when `system_prompt_override` is set.
    pub role_setting: Option<String>,
    /// Project-level agent rules (e.g. from AGENTS.md): appended after role_setting, before base.
    /// Order in prompt: role_setting + agents_md + base_content.
    pub agents_md: Option<String>,
    /// When set, used as the full system prompt instead of assembling from workdir + approval.
    pub system_prompt_override: Option<String>,
    /// Skills prompt: available_skills summary (and optionally preloaded content). Injected between agents_md and base_content.
    pub skills_prompt: Option<String>,
}

/// Converts a HelveConfig and a base ReactBuildConfig into a single ReactBuildConfig.
///
/// Product fields (working_folder, thread_id, user_id, approval_policy) are taken from
/// `helve` when set; otherwise from `base`. The final system prompt is assembled
/// through [`assemble_react_system_prompt`].
///
/// Other fields (db_path, mcp_*, openai_*, etc.) are always taken from `base`.
///
/// # Example
///
/// ```ignore
/// let base = ReactBuildConfig::from_env();
/// let helve = HelveConfig {
///     working_folder: Some(PathBuf::from("/tmp/workspace")),
///     thread_id: Some("conv-1".into()),
///     approval_policy: Some(ApprovalPolicy::DestructiveOnly),
///     ..Default::default()
/// };
/// let config = to_react_build_config(&helve, base);
/// let runner = build_react_runner(&config, None, false).await?;
/// ```
pub fn to_react_build_config(helve: &HelveConfig, base: ReactBuildConfig) -> ReactBuildConfig {
    let prompt_inputs = ReactPromptInputs {
        full_override: helve.system_prompt_override.clone(),
        base_prompt_override: base.system_prompt.clone(),
        role_setting: helve.role_setting.clone(),
        agents_md: helve.agents_md.clone(),
        skills_prompt: helve.skills_prompt.clone(),
        working_folder: helve.working_folder.clone(),
        approval_policy: helve.approval_policy,
    };
    let system_prompt = Some(assemble_react_system_prompt(&prompt_inputs));

    ReactBuildConfig {
        system_prompt,
        working_folder: helve.working_folder.clone().or(base.working_folder),
        thread_id: helve.thread_id.clone().or(base.thread_id),
        user_id: helve.user_id.clone().or(base.user_id),
        approval_policy: helve.approval_policy.or(base.approval_policy),
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: to_react_build_config with working_folder and approval_policy sets system_prompt from assembly.
    #[test]
    fn to_react_build_config_assembles_prompt_when_working_folder_set() {
        let mut base = ReactBuildConfig::from_env();
        base.thread_id = Some("t1".into());
        let helve = HelveConfig {
            working_folder: Some(PathBuf::from("/tmp/ws")),
            approval_policy: Some(ApprovalPolicy::DestructiveOnly),
            ..Default::default()
        };
        let out = to_react_build_config(&helve, base);
        assert_eq!(
            out.working_folder.as_deref(),
            Some(std::path::Path::new("/tmp/ws"))
        );
        assert_eq!(out.approval_policy, Some(ApprovalPolicy::DestructiveOnly));
        assert!(out.system_prompt.as_ref().unwrap().contains("/tmp/ws"));
        assert!(out.system_prompt.as_ref().unwrap().contains("APPROVAL"));
        assert_eq!(out.thread_id.as_deref(), Some("t1"));
    }

    /// **Scenario**: system_prompt_override takes precedence over assembled prompt.
    #[test]
    fn to_react_build_config_override_precedence() {
        let base = ReactBuildConfig::from_env();
        let helve = HelveConfig {
            working_folder: Some(PathBuf::from("/x")),
            system_prompt_override: Some("Custom prompt.".to_string()),
            ..Default::default()
        };
        let out = to_react_build_config(&helve, base);
        assert_eq!(out.system_prompt.as_deref(), Some("Custom prompt."));
    }

    /// **Scenario**: HelveConfig defaults are all None.
    #[test]
    fn helve_config_default() {
        let c = HelveConfig::default();
        assert!(c.working_folder.is_none());
        assert!(c.thread_id.is_none());
        assert!(c.user_id.is_none());
        assert!(c.approval_policy.is_none());
        assert!(c.role_setting.is_none());
        assert!(c.agents_md.is_none());
        assert!(c.system_prompt_override.is_none());
        assert!(c.skills_prompt.is_none());
    }

    /// **Scenario**: role_setting is prepended to assembled prompt when no system_prompt_override.
    #[test]
    fn to_react_build_config_role_setting_prepended() {
        let mut base = ReactBuildConfig::from_env();
        base.system_prompt = None;
        let helve = HelveConfig {
            working_folder: Some(PathBuf::from("/tmp/ws")),
            role_setting: Some("You are a code review expert.".to_string()),
            ..Default::default()
        };
        let out = to_react_build_config(&helve, base);
        let prompt = out.system_prompt.as_deref().unwrap();
        assert!(prompt.starts_with("You are a code review expert."));
        assert!(prompt.contains("/tmp/ws"));
    }

    /// **Scenario**: only agents_md is prepended when role_setting is None.
    #[test]
    fn to_react_build_config_agents_md_only() {
        let mut base = ReactBuildConfig::from_env();
        base.system_prompt = None;
        let helve = HelveConfig {
            working_folder: Some(PathBuf::from("/tmp/ws")),
            agents_md: Some("Project rules from AGENTS.md.".to_string()),
            ..Default::default()
        };
        let out = to_react_build_config(&helve, base);
        let prompt = out.system_prompt.as_deref().unwrap();
        assert!(prompt.starts_with("Project rules from AGENTS.md."));
        assert!(prompt.contains("/tmp/ws"));
    }

    /// **Scenario**: role_setting then agents_md then base_content order.
    #[test]
    fn to_react_build_config_role_setting_then_agents_md() {
        let mut base = ReactBuildConfig::from_env();
        base.system_prompt = None;
        let helve = HelveConfig {
            working_folder: Some(PathBuf::from("/tmp/ws")),
            role_setting: Some("SOUL content.".to_string()),
            agents_md: Some("AGENTS content.".to_string()),
            ..Default::default()
        };
        let out = to_react_build_config(&helve, base);
        let prompt = out.system_prompt.as_deref().unwrap();
        assert!(prompt.starts_with("SOUL content."));
        assert!(prompt.contains("AGENTS content."));
        assert!(prompt.contains("/tmp/ws"));
        let soul_pos = prompt.find("SOUL content.").unwrap();
        let agents_pos = prompt.find("AGENTS content.").unwrap();
        let workdir_pos = prompt.find("/tmp/ws").unwrap();
        assert!(soul_pos < agents_pos && agents_pos < workdir_pos);
    }

    #[test]
    fn to_react_build_config_role_agents_skills_then_base() {
        let mut base = ReactBuildConfig::from_env();
        base.system_prompt = Some("BASE".to_string());
        let helve = HelveConfig {
            role_setting: Some("ROLE".to_string()),
            agents_md: Some("AGENTS".to_string()),
            skills_prompt: Some("SKILLS".to_string()),
            ..Default::default()
        };
        let out = to_react_build_config(&helve, base);
        let prompt = out.system_prompt.as_deref().unwrap();
        let role_pos = prompt.find("ROLE").unwrap();
        let agents_pos = prompt.find("AGENTS").unwrap();
        let skills_pos = prompt.find("SKILLS").unwrap();
        let base_pos = prompt.find("BASE").unwrap();
        assert!(role_pos < agents_pos && agents_pos < skills_pos && skills_pos < base_pos);
    }
}
