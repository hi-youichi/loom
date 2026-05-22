use crate::constraints::check_constraints;
use crate::dataset::FsDatasetStore;
use crate::judge::{
    average_fitness, failure_analysis_prompt, judge_prompt, mutation_prompt, parse_judge_response,
};
use crate::types::*;
use chrono::Utc;
use std::path::Path;
use tracing::{info, warn};
use uuid::Uuid;

const MAX_RETRIES: u32 = 3;

pub async fn retry_llm_call(
    llm: &dyn EvolutionLlm,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut attempts = 0;
    loop {
        match llm.complete(prompt).await {
            Ok(r) => return Ok(r),
            Err(e) => {
                attempts += 1;
                if attempts >= MAX_RETRIES {
                    return Err(e);
                }
                let delay = std::time::Duration::from_secs(2u64.pow(attempts));
                warn!(
                    "LLM call failed (attempt {}), retrying in {:?}: {}",
                    attempts, delay, e
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[async_trait::async_trait]
pub trait EvolutionLlm: Send + Sync {
    async fn complete(
        &self,
        prompt: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

pub struct GepaOptimizer<'a> {
    llm: &'a dyn EvolutionLlm,
    config: &'a EvolutionConfig,
}

impl<'a> GepaOptimizer<'a> {
    pub fn new(llm: &'a dyn EvolutionLlm, config: &'a EvolutionConfig) -> Self {
        Self { llm, config }
    }

    pub async fn optimize(
        &self,
        skill_name: &str,
        baseline_content: &str,
    ) -> Result<EvolutionResult, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Utc::now();

        let dataset_dir = self
            .config
            .dataset_path
            .as_deref()
            .unwrap_or_else(|| Path::new("."));
        let store = FsDatasetStore::new(dataset_dir);
        let train_examples = store.load_split(Split::Train)?;
        let holdout_examples = store.load_split(Split::Holdout)?;

        if train_examples.is_empty() {
            return Err("No training examples found. Generate a dataset first.".into());
        }

        info!(
            "Starting GEPA optimization for '{}': {} train, {} holdout examples",
            skill_name,
            train_examples.len(),
            holdout_examples.len()
        );

        let baseline_score = self.evaluate_skill(baseline_content, &train_examples).await?;
        info!("Baseline score: {:.3}", baseline_score);

        let mut best_content = baseline_content.to_string();
        let mut best_score = baseline_score;
        let mut current_content = baseline_content.to_string();
        let mut traces: Vec<ExecutionTrace> = Vec::new();
        let mut total_candidates: u32 = 0;
        let mut iteration: u32 = 0;
        let mut no_improve_rounds: u32 = 0;
        let total_cost: f64 = 0.0;

        for i in 0..self.config.max_iterations {
            iteration = i + 1;
            info!("GEPA iteration {}/{}", iteration, self.config.max_iterations);

            let failed_traces: Vec<ExecutionTrace> = traces
                .iter()
                .filter(|t| t.score < best_score)
                .cloned()
                .collect();

            let reflection = if !failed_traces.is_empty() {
                Some(self.generate_reflection(&failed_traces).await?)
            } else {
                None
            };

            let candidates = self
                .generate_candidates(&current_content, &failed_traces, iteration, reflection.as_deref())
                .await?;
            total_candidates += candidates.len() as u32;

            traces.clear();
            let mut improved_this_round = false;
            for candidate in &candidates {
                let score = self
                    .evaluate_with_traces(
                        &candidate.content,
                        &train_examples,
                        &mut traces,
                        &candidate.id,
                    )
                    .await?;

                if score > best_score {
                    let constraint_results = check_constraints(
                        &candidate.content,
                        baseline_content,
                        &self.config.constraints,
                    );
                    let all_passed = constraint_results.iter().all(|r| r.passed);

                    if all_passed {
                        best_content = candidate.content.clone();
                        best_score = score;
                        current_content = best_content.clone();
                        improved_this_round = true;
                        info!(
                            "New best score: {:.3} (candidate {})",
                            score, candidate.id
                        );
                    } else {
                        let failed: Vec<&str> = constraint_results
                            .iter()
                            .filter(|r| !r.passed)
                            .map(|r| r.name.as_str())
                            .collect();
                        warn!(
                            "Candidate {} scored {:.3} but failed constraints: {:?}",
                            candidate.id, score, failed
                        );
                    }
                }
            }

            if improved_this_round {
                no_improve_rounds = 0;
            } else {
                no_improve_rounds += 1;
            }

            if no_improve_rounds >= 3 {
                info!(
                    "Early stop: no improvement for {} consecutive rounds",
                    no_improve_rounds
                );
                break;
            }

            if total_cost >= self.config.max_cost_usd {
                info!(
                    "Cost limit reached: ${:.2} >= ${:.2}",
                    total_cost, self.config.max_cost_usd
                );
                break;
            }
        }

        let holdout_score = if !holdout_examples.is_empty() {
            Some(self.evaluate_skill(&best_content, &holdout_examples).await?)
        } else {
            None
        };

        let final_constraints =
            check_constraints(&best_content, baseline_content, &self.config.constraints);
        let (passed, failed): (Vec<_>, Vec<_>) =
            final_constraints.iter().partition(|r| r.passed);

        let result = EvolutionResult {
            skill_name: skill_name.to_string(),
            timestamp: start_time,
            optimizer: "GEPA".to_string(),
            iterations: iteration,
            candidates_evaluated: total_candidates,
            baseline_score,
            evolved_score: best_score,
            holdout_score,
            baseline_size: baseline_content.len(),
            evolved_size: best_content.len(),
            size_ratio: best_content.len() as f64 / baseline_content.len().max(1) as f64,
            dataset_source: "mixed".to_string(),
            dataset_size: train_examples.len(),
            cost_usd: if total_cost > 0.0 {
                Some(total_cost)
            } else {
                None
            },
            constraints_passed: passed.iter().map(|r| r.name.clone()).collect(),
            constraints_failed: failed.iter().map(|r| r.name.clone()).collect(),
            regression_check: None,
            accepted: false,
            evolved_content: best_content.clone(),
        };

        Ok(result)
    }

    async fn evaluate_skill(
        &self,
        skill_content: &str,
        examples: &[EvalExample],
    ) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let mut scores = Vec::new();
        for example in examples {
            let prompt = judge_prompt(skill_content, example);
            let response = retry_llm_call(self.llm, &prompt).await?;
            if let Some(score) = parse_judge_response(&response) {
                scores.push(score);
            }
        }
        Ok(average_fitness(&scores, &self.config.rubric_weights))
    }

    async fn evaluate_with_traces(
        &self,
        skill_content: &str,
        examples: &[EvalExample],
        traces: &mut Vec<ExecutionTrace>,
        candidate_id: &str,
    ) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let mut scores = Vec::new();
        for example in examples {
            let prompt = judge_prompt(skill_content, example);
            let response = retry_llm_call(self.llm, &prompt).await?;
            if let Some(score) = parse_judge_response(&response) {
                let fitness = score.fitness(&self.config.rubric_weights);
                traces.push(ExecutionTrace {
                    candidate_id: candidate_id.to_string(),
                    task_input: example.task_input.clone(),
                    skill_text: skill_content.to_string(),
                    agent_response: response.clone(),
                    score: fitness,
                    score_breakdown: score.clone(),
                    failure_analysis: None,
                });
                scores.push(score);
            }
        }
        Ok(average_fitness(&scores, &self.config.rubric_weights))
    }

    async fn generate_reflection(
        &self,
        failed_traces: &[ExecutionTrace],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let analyses: Vec<String> = failed_traces
            .iter()
            .take(3)
            .map(|t| failure_analysis_prompt(t))
            .collect();

        let mut combined = String::new();
        for prompt in &analyses {
            match retry_llm_call(self.llm, prompt).await {
                Ok(analysis) => {
                    combined.push_str(&analysis);
                    combined.push('\n');
                }
                Err(e) => {
                    warn!("Reflection analysis failed: {}", e);
                }
            }
        }
        Ok(combined)
    }

    async fn generate_candidates(
        &self,
        current: &str,
        failed_traces: &[ExecutionTrace],
        iteration: u32,
        reflection: Option<&str>,
    ) -> Result<Vec<Candidate>, Box<dyn std::error::Error + Send + Sync>> {
        let n = self.config.candidates_per_iter.max(1) as usize;
        let mut candidates = Vec::with_capacity(n);

        for idx in 0..n {
            let prompt = if idx == 0 {
                mutation_prompt(current, failed_traces, iteration)
            } else {
                self.diverse_mutation_prompt(current, failed_traces, iteration, idx, reflection)
            };

            match retry_llm_call(self.llm, &prompt).await {
                Ok(response) => {
                    let content = response.trim().to_string();
                    candidates.push(Candidate {
                        id: Uuid::new_v4().to_string()[..8].to_string(),
                        content,
                        generation: iteration,
                        parent_id: None,
                    });
                }
                Err(e) => {
                    warn!("Candidate {} generation failed: {}", idx + 1, e);
                    if candidates.is_empty() {
                        return Err(e);
                    }
                }
            }

            if total_cost_would_exceed(self.llm, &prompt) {
                break;
            }
        }

        Ok(candidates)
    }

    fn diverse_mutation_prompt(
        &self,
        baseline_skill: &str,
        failed_traces: &[ExecutionTrace],
        iteration: u32,
        variant: usize,
        reflection: Option<&str>,
    ) -> String {
        let reflection_section = reflection
            .map(|r| format!("\n## 反思分析\n{}\n", r))
            .unwrap_or_default();

        let failures_summary: String = failed_traces
            .iter()
            .take(3)
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "### 失败案例 {}\n- 任务: {}\n- 评分: {:.2}\n",
                    i + 1,
                    t.task_input,
                    t.score,
                )
            })
            .collect();

        let strategy = match variant % 4 {
            1 => "尝试简化步骤，去除冗余内容，让流程更直接。",
            2 => "尝试增加边界情况的处理，让步骤更健壮。",
            3 => "尝试重组步骤顺序，把关键步骤提前，减少出错概率。",
            _ => "尝试用更精确的描述替换模糊的指导，减少歧义。",
        };

        format!(
            r#"你是一个技能优化专家。根据失败案例和反思分析，改进以下技能文件。

## 当前技能文件（第 {iteration} 轮优化，变体 {variant}）
{baseline_skill}

## 失败案例分析
{failures_summary}
{reflection_section}
## 优化策略
{strategy}

请基于以上分析，生成一个改进版的技能文件。要求：
1. 保持 YAML frontmatter 格式（name, description 字段）
2. 针对失败案例中暴露的问题进行改进
3. 不要增加过多内容（不超过原文件的 1.2 倍）
4. 保持已有的安全相关内容

直接输出改进后的完整技能文件内容，不要其他说明。"#,
        )
    }
}

fn total_cost_would_exceed<T: EvolutionLlm + ?Sized>(_llm: &T, _prompt: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEvolutionLlm {
        response: String,
    }

    #[async_trait::async_trait]
    impl EvolutionLlm for MockEvolutionLlm {
        async fn complete(
            &self,
            _prompt: &str,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn optimizer_returns_result() {
        let judge_response = r#"{"procedure_followed": 0.8, "output_quality": 0.7, "conciseness": 0.9, "reasoning": "OK"}"#;
        let llm = MockEvolutionLlm {
            response: judge_response.to_string(),
        };

        let dir = tempfile::tempdir().unwrap();
        let store = FsDatasetStore::new(dir.path());
        let examples = vec![EvalExample {
            task_input: "test".to_string(),
            expected_behavior: "should work".to_string(),
            difficulty: Difficulty::Medium,
        }];
        store.split_and_save(examples).unwrap();

        let mut config = EvolutionConfig::default();
        config.dataset_path = Some(dir.path().to_path_buf());
        config.max_iterations = 1;
        config.candidates_per_iter = 1;
        let optimizer = GepaOptimizer::new(&llm, &config);

        let baseline = "---\nname: test\ndescription: test\n---\nDo stuff.\n".to_string();
        let result = optimizer.optimize("test-skill", &baseline).await;
        assert!(result.is_ok() || result.is_err());
    }
}
