//! Agent prompts loaded from YAML files by directory (optional override for in-code defaults).
//!
//! See [`AgentPrompts`] and [`load`].
//! Interacts with [`ReactBuildConfig`](loom_react_config::ReactBuildConfig), [`assemble_system_prompt`](crate::assemble::assemble_system_prompt),
//! and runners that use system/prompt strings (ReAct, ToT, GoT, DUP).

pub mod constants;
mod load;
mod resolve;

pub use constants::*;

use serde::Deserialize;

pub use load::{default_from_embedded, load, load_or_default, LoadError};
pub use resolve::AgentPrompts;

/// Per-file YAML shape for `prompts/react.yaml`. All keys optional.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct ReactPromptsFile {
    pub system_prompt: Option<String>,
    pub tool_error_template: Option<String>,
    pub execution_error_template: Option<String>,
}

/// Per-file YAML shape for `prompts/tot.yaml`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct TotPromptsFile {
    pub expand_system_addon: Option<String>,
    pub research_quality_addon: Option<String>,
}

/// Per-file YAML shape for `prompts/got.yaml`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct GotPromptsFile {
    pub plan_system: Option<String>,
    pub agot_expand_system: Option<String>,
}

/// Per-file YAML shape for `prompts/dup.yaml`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct DupPromptsFile {
    pub understand_prompt: Option<String>,
}

/// Per-file YAML shape for `prompts/prompt.yaml`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct PromptOverridesFile {
    pub system_addon: Option<String>,
}