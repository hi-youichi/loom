//! Core types for the evolution subsystem.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// An evaluation example: a task input paired with expected behavior (rubric).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExample {
    pub task_input: String,
    pub expected_behavior: String,
    #[serde(default)]
    pub difficulty: Difficulty,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Easy,
    #[default]
    Medium,
    Hard,
}

/// Which split an example belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    Train,
    Val,
    Holdout,
}

/// Result of a single constraint check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Configuration for constraint checking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintConfig {
    /// Maximum size ratio (evolved / baseline). Default: 1.2
    #[serde(default = "default_size_ratio")]
    pub max_size_ratio: f64,
    /// Minimum semantic similarity (cosine). Default: 0.7
    #[serde(default = "default_semantic_threshold")]
    pub min_semantic_similarity: f64,
    /// Whether to check semantic preservation (needs embedding). Default: false
    #[serde(default)]
    pub check_semantic: bool,
}

fn default_size_ratio() -> f64 {
    1.2
}
fn default_semantic_threshold() -> f64 {
    0.7
}

impl Default for ConstraintConfig {
    fn default() -> Self {
        Self {
            max_size_ratio: default_size_ratio(),
            min_semantic_similarity: default_semantic_threshold(),
            check_semantic: false,
        }
    }
}

/// Scoring dimensions for a single evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubricScore {
    /// Did the response follow the skill procedure? [0, 1]
    pub procedure_followed: f64,
    /// Was the output correct / useful? [0, 1]
    pub output_quality: f64,
    /// Was the response concise? [0, 1]
    pub conciseness: f64,
}

impl RubricScore {
    /// Weighted fitness score.
    pub fn fitness(&self, weights: &RubricWeights) -> f64 {
        self.procedure_followed * weights.procedure
            + self.output_quality * weights.quality
            + self.conciseness * weights.conciseness
    }
}

/// Configurable weights for rubric scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubricWeights {
    #[serde(default = "default_w_procedure")]
    pub procedure: f64,
    #[serde(default = "default_w_quality")]
    pub quality: f64,
    #[serde(default = "default_w_conciseness")]
    pub conciseness: f64,
}

fn default_w_procedure() -> f64 {
    0.3
}
fn default_w_quality() -> f64 {
    0.5
}
fn default_w_conciseness() -> f64 {
    0.2
}

impl Default for RubricWeights {
    fn default() -> Self {
        Self {
            procedure: default_w_procedure(),
            quality: default_w_quality(),
            conciseness: default_w_conciseness(),
        }
    }
}

/// A single execution trace for a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub candidate_id: String,
    pub task_input: String,
    pub skill_text: String,
    pub agent_response: String,
    pub score: f64,
    pub score_breakdown: RubricScore,
    pub failure_analysis: Option<String>,
}

/// A candidate skill produced during evolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub content: String,
    pub generation: u32,
    pub parent_id: Option<String>,
}

/// The result of a full evolution run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionResult {
    pub skill_name: String,
    pub timestamp: DateTime<Utc>,
    pub optimizer: String,
    pub iterations: u32,
    pub candidates_evaluated: u32,
    pub baseline_score: f64,
    pub evolved_score: f64,
    pub holdout_score: Option<f64>,
    pub baseline_size: usize,
    pub evolved_size: usize,
    pub size_ratio: f64,
    pub dataset_source: String,
    pub dataset_size: usize,
    pub cost_usd: Option<f64>,
    pub constraints_passed: Vec<String>,
    pub constraints_failed: Vec<String>,
    pub regression_check: Option<String>,
    pub accepted: bool,
    /// The evolved skill content.
    pub evolved_content: String,
}

/// Configuration for an evolution run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionConfig {
    /// Maximum number of GEPA iterations.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// Number of candidates per iteration.
    #[serde(default = "default_candidates_per_iter")]
    pub candidates_per_iter: u32,
    /// Maximum cost per run in USD.
    #[serde(default = "default_max_cost")]
    pub max_cost_usd: f64,
    /// Constraint configuration.
    #[serde(default)]
    pub constraints: ConstraintConfig,
    /// Rubric weights.
    #[serde(default)]
    pub rubric_weights: RubricWeights,
    /// Dataset path (JSONL directory).
    pub dataset_path: Option<PathBuf>,
    /// Evolution runs output directory.
    #[serde(default = "default_evolution_dir")]
    pub evolution_dir: PathBuf,
}

fn default_max_iterations() -> u32 {
    10
}
fn default_candidates_per_iter() -> u32 {
    5
}
fn default_max_cost() -> f64 {
    10.0
}
fn default_evolution_dir() -> PathBuf {
    dirs_data_path().join("evolution")
}

fn dirs_data_path() -> PathBuf {
    // Default to ~/.loom/data
    let home = std::env::var("LOOM_HOME")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{}/.loom", h)))
        .unwrap_or_else(|| "~/.loom".to_string());
    PathBuf::from(home).join("data")
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            max_iterations: default_max_iterations(),
            candidates_per_iter: default_candidates_per_iter(),
            max_cost_usd: default_max_cost(),
            constraints: ConstraintConfig::default(),
            rubric_weights: RubricWeights::default(),
            dataset_path: None,
            evolution_dir: default_evolution_dir(),
        }
    }
}
