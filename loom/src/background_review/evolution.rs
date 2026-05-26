//! Evolution trigger types.
//!
//! The actual EvolutionTrigger implementation lives in `cli` because it depends on
//! `loom-evolution`, which in turn depends on `loom` (cyclic dependency).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolutionTriggerConfig {
    pub dataset_path: std::path::PathBuf,
    pub min_examples: usize,
    pub max_iterations: u32,
}

impl Default for EvolutionTriggerConfig {
    fn default() -> Self {
        Self {
            dataset_path: std::path::PathBuf::from(".loom/evolution/datasets"),
            min_examples: 5,
            max_iterations: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionOutcome {
    pub skill_name: String,
    pub improved: bool,
    pub baseline_score: f64,
    pub best_score: f64,
}
