//! LLM config block for run config summary.
//!
//! Implements [`ConfigSection`](super::ConfigSection). Used by CLI or other callers
//! to build the "LLM config" line in a run config summary.

use super::ConfigSection;

/// LLM configuration summary: model, api_base, temperature, tool_choice.
///
/// Built from `RunConfig` (or equivalent) in the CLI. Implements [`ConfigSection`].
pub struct LlmConfigSummary {
    /// Model name, e.g. `gpt-4o-mini`.
    pub model: String,
    /// API base URL, e.g. `https://api.openai.com/v1`.
    pub api_base: String,
    /// Sampling temperature; `None` means use API default (displayed as "(default)").
    pub temperature: Option<f32>,
    /// Tool choice mode, e.g. `"auto"`, `"none"`, `"required"`.
    pub tool_choice: String,
}

impl ConfigSection for LlmConfigSummary {
    fn section_name(&self) -> &str {
        "LLM config"
    }

    fn entries(&self) -> Vec<(&'static str, String)> {
        let temperature = self
            .temperature
            .map(|t| t.to_string())
            .unwrap_or_else(|| "(default)".to_string());
        vec![
            ("model", self.model.clone()),
            ("api_base", self.api_base.clone()),
            ("temperature", temperature),
            ("tool_choice", self.tool_choice.clone()),
        ]
    }
}
