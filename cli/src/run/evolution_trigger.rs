use crate::run::skill_registry::{SkillRegistry, Lifecycle};
use loom::llm::LlmClient;
use loom::message::Message;
use loom::message::UserContent;
use loom_evolution::{EvolutionConfig, EvolutionLlm, GepaOptimizer};
use std::path::Path;
use tracing::{info, warn};

struct LlmClientAdapter<'a> {
    client: &'a dyn LlmClient,
}

#[async_trait::async_trait]
impl<'a> EvolutionLlm for LlmClientAdapter<'a> {
    async fn complete(
        &self,
        prompt: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let messages = vec![Message::user(UserContent::text(prompt.to_string()))];
        let response = self.client.invoke(&messages).await?;
        Ok(response.content)
    }
}

#[derive(Clone)]
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

pub struct EvolutionTrigger<'a> {
    llm: LlmClientAdapter<'a>,
    skills: SkillRegistry,
    trigger_config: EvolutionTriggerConfig,
    evolution_config: EvolutionConfig,
}

impl<'a> EvolutionTrigger<'a> {
    pub fn new(
        llm: &'a dyn LlmClient,
        skills_path: &Path,
        trigger_config: EvolutionTriggerConfig,
    ) -> Self {
        let skills = SkillRegistry::new(skills_path);
        let evolution_config = EvolutionConfig {
            dataset_path: Some(trigger_config.dataset_path.clone()),
            max_iterations: trigger_config.max_iterations,
            ..Default::default()
        };
        Self {
            llm: LlmClientAdapter { client: llm },
            skills,
            trigger_config,
            evolution_config,
        }
    }

    pub fn eligible_skills(&self) -> Vec<String> {
        let Ok(all) = self.skills.list() else {
            return Vec::new();
        };
        all.iter()
            .filter(|m| m.lifecycle == Lifecycle::Active)
            .filter(|m| {
                let skill_dir = self.trigger_config.dataset_path.join(&m.name);
                let train_file = skill_dir.join("train.jsonl");
                if !train_file.exists() {
                    return false;
                }
                let Ok(contents) = std::fs::read_to_string(&train_file) else {
                    return false;
                };
                contents.lines().filter(|l| !l.trim().is_empty()).count() >= self.trigger_config.min_examples
            })
            .map(|m| m.name.clone())
            .collect()
    }

    pub async fn try_evolve(&self, skill_name: &str) -> Result<EvolutionOutcome, String> {
        let skill = self.skills.load(skill_name)
            .map_err(|e| format!("Failed to load skill '{}': {}", skill_name, e))?;

        let optimizer = GepaOptimizer::new(&self.llm, &self.evolution_config);
        let result = optimizer.optimize(skill_name, &skill.body).await
            .map_err(|e| format!("GEPA optimization failed: {}", e))?;

        if result.accepted {
            // Safety: refuse to save if evolved content is empty or would destroy the skill
            if result.evolved_content.trim().is_empty() {
                warn!(
                    "Evolved content for '{}' is empty — refusing to overwrite skill body",
                    skill_name
                );
            } else {
                let mut updated = skill;
                updated.body = result.evolved_content.clone();
                self.skills.save(skill_name, &updated)
                    .map_err(|e| format!("Failed to save evolved skill: {}", e))?;

                info!(
                    "Evolved skill '{}': score {:.3} -> {:.3}",
                    skill_name, result.baseline_score, result.evolved_score
                );
            }
        } else {
            info!(
                "No improvement for skill '{}' (baseline: {:.3}, evolved: {:.3})",
                skill_name, result.baseline_score, result.evolved_score
            );
        }

        Ok(EvolutionOutcome {
            skill_name: skill_name.to_string(),
            improved: result.accepted,
            baseline_score: result.baseline_score,
            best_score: result.evolved_score,
        })
    }
}

pub struct EvolutionOutcome {
    pub skill_name: String,
    pub improved: bool,
    pub baseline_score: f64,
    pub best_score: f64,
}
