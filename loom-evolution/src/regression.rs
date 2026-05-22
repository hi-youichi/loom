use crate::dataset::FsDatasetStore;
use crate::judge::{average_fitness, judge_prompt, parse_judge_response};
use crate::optimizer::{EvolutionLlm, retry_llm_call};
use crate::types::{EvalExample, RubricWeights, Split};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionResult {
    pub passed: bool,
    pub baseline_scores: Vec<f64>,
    pub evolved_scores: Vec<f64>,
    pub max_regression: f64,
}

pub struct RegressionGate {
    golden_tasks: Vec<EvalExample>,
    threshold: f64,
    rubric_weights: RubricWeights,
}

impl RegressionGate {
    pub fn new(golden_tasks: Vec<EvalExample>, threshold: f64, rubric_weights: RubricWeights) -> Self {
        Self {
            golden_tasks,
            threshold,
            rubric_weights,
        }
    }

    pub fn from_dataset_dir(
        dataset_dir: &Path,
        threshold: f64,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let store = FsDatasetStore::new(dataset_dir);
        let golden = store.load_split(Split::Holdout)?;
        Ok(Self::new(golden, threshold, RubricWeights::default()))
    }

    pub async fn check(
        &self,
        llm: &dyn EvolutionLlm,
        baseline: &str,
        evolved: &str,
    ) -> Result<RegressionResult, Box<dyn std::error::Error + Send + Sync>> {
        if self.golden_tasks.is_empty() {
            info!("No golden tasks, skipping regression check");
            return Ok(RegressionResult {
                passed: true,
                baseline_scores: vec![],
                evolved_scores: vec![],
                max_regression: 0.0,
            });
        }

        let baseline_scores = self.evaluate(llm, baseline).await?;
        let evolved_scores = self.evaluate(llm, evolved).await?;

        let max_regression = baseline_scores
            .iter()
            .zip(evolved_scores.iter())
            .map(|(b, e)| (b - e).max(0.0))
            .fold(0.0_f64, |acc, r| acc.max(r));

        let passed = max_regression <= self.threshold;

        if !passed {
            warn!(
                "Regression detected: max regression {:.3} > threshold {:.3}",
                max_regression, self.threshold
            );
        } else {
            info!(
                "Regression check passed: max regression {:.3} <= threshold {:.3}",
                max_regression, self.threshold
            );
        }

        Ok(RegressionResult {
            passed,
            baseline_scores,
            evolved_scores,
            max_regression,
        })
    }

    async fn evaluate(
        &self,
        llm: &dyn EvolutionLlm,
        skill_content: &str,
    ) -> Result<Vec<f64>, Box<dyn std::error::Error + Send + Sync>> {
        let mut scores = Vec::new();
        for task in &self.golden_tasks {
            let prompt = judge_prompt(skill_content, task);
            let response = retry_llm_call(llm, &prompt).await?;
            if let Some(score) = parse_judge_response(&response) {
                let fitness = score.fitness(&self.rubric_weights);
                scores.push(fitness);
            } else {
                scores.push(0.0);
            }
        }
        Ok(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_result_serialization() {
        let result = RegressionResult {
            passed: true,
            baseline_scores: vec![0.8, 0.9],
            evolved_scores: vec![0.85, 0.88],
            max_regression: 0.02,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("passed"));
    }
}
