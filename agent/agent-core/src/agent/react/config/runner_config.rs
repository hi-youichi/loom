//! ToT/GoT runner configuration types.

/// ToT-specific runner config.
#[derive(Clone, Debug)]
pub struct TotRunnerConfig {
    pub max_depth: u32,
    pub candidates_per_step: u32,
    pub research_quality_addon: bool,
}

impl Default for TotRunnerConfig {
    fn default() -> Self {
        Self {
            max_depth: 5,
            candidates_per_step: 3,
            research_quality_addon: false,
        }
    }
}

/// GoT-specific runner config.
#[derive(Clone, Debug, Default)]
pub struct GotRunnerConfig {
    pub adaptive: bool,
    pub agot_llm_complexity: bool,
}
