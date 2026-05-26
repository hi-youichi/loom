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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evolution_trigger_config_default() {
        let config = EvolutionTriggerConfig::default();
        assert_eq!(config.dataset_path, std::path::PathBuf::from(".loom/evolution/datasets"));
        assert_eq!(config.min_examples, 5);
        assert_eq!(config.max_iterations, 3);
    }

    #[test]
    fn evolution_trigger_config_serialization() {
        let config = EvolutionTriggerConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: EvolutionTriggerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.min_examples, 5);
        assert_eq!(parsed.max_iterations, 3);
    }

    #[test]
    fn evolution_outcome_serialization() {
        let outcome = EvolutionOutcome {
            skill_name: "debug-spinner".to_string(),
            improved: true,
            baseline_score: 0.5,
            best_score: 0.9,
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let parsed: EvolutionOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skill_name, "debug-spinner");
        assert!(parsed.improved);
        assert!((parsed.baseline_score - 0.5).abs() < f64::EPSILON);
        assert!((parsed.best_score - 0.9).abs() < f64::EPSILON);
    }
}
