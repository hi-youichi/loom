//! Product-level config (HelveConfig) and conversion to ReactBuildConfig.
//!
//! Server (or CLI) can parse request body or CLI args into HelveConfig, then call
//! `to_react_build_config(helve, base)` to get a full ReactBuildConfig for
//! `build_react_runner`.

use std::path::PathBuf;

use crate::agent::react::ReactBuildConfig;
use crate::REACT_SYSTEM_PROMPT;

use super::prompt::{assemble_system_prompt, ApprovalPolicy};

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
    /// Role/persona setting: prepended to the assembled system prompt (workdir + approval).
    /// E.g. "You are a code review expert." Does not apply when `system_prompt_override` is set.
    pub role_setting: Option<String>,
    /// When set, used as the full system prompt instead of assembling from workdir + approval.
    pub system_prompt_override: Option<String>,
}

/// Converts a HelveConfig and a base ReactBuildConfig into a single ReactBuildConfig.
///
/// Product fields (working_folder, thread_id, user_id, approval_policy) are taken from
/// `helve` when set; otherwise from `base`. System prompt is set in this order:
/// 1. `helve.system_prompt_override` if present (used as full system prompt)
/// 2. else: base content = assembled from workdir (if set) or `base.system_prompt`;
///    if `helve.role_setting` is set, system prompt = role_setting + "\n\n" + base content;
///    otherwise system prompt = base content
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
    let system_prompt = helve.system_prompt_override.clone().or_else(|| {
        let base_content = helve
            .working_folder
            .as_ref()
            .map(|p| assemble_system_prompt(p.as_path(), helve.approval_policy))
            .or_else(|| base.system_prompt.clone())
            .unwrap_or_else(|| REACT_SYSTEM_PROMPT.to_string());
        Some(if let Some(ref role) = helve.role_setting {
            format!("{}\n\n{}", role.trim(), base_content)
        } else {
            base_content
        })
    });

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
        assert!(c.system_prompt_override.is_none());
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
}
