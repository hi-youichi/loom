//! GEPA-based skill evolution optimizer for Loom agents.
//!
//! This crate provides the evolution subsystem described in `docs/evolution/`:
//! - Dataset management (synthetic generation, JSONL storage with splits)
//! - Constraint checking (size, structure, safety, semantic preservation)
//! - LLM-as-judge evaluation (rubric scoring)
//! - GEPA optimizer (multi-round candidate generation + evaluation + reflection)
//! - Deployment (version management, backups, rollback)
//!
//! # Quick start
//!
//! ```rust,ignore
//! use loom_evolution::{GepaOptimizer, EvolutionConfig, EvolutionLlm};
//!
//! struct MyLlm;
//! #[async_trait::async_trait]
//! impl EvolutionLlm for MyLlm {
//!     async fn complete(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
//!         // Call your LLM here
//!         todo!()
//!     }
//! }
//!
//! let llm = MyLlm;
//! let config = EvolutionConfig::default();
//! let optimizer = GepaOptimizer::new(&llm, &config);
//! let result = optimizer.optimize("my-skill", &baseline_content).await?;
//! ```

pub mod types;
pub mod dataset;
pub mod constraints;
pub mod judge;
pub mod optimizer;
pub mod deploy;

pub use types::*;
pub use dataset::{FsDatasetStore, DatasetError};
pub use constraints::check_constraints;
pub use judge::{judge_prompt, mutation_prompt, parse_judge_response, average_fitness};
pub use optimizer::{GepaOptimizer, EvolutionLlm};
pub use deploy::{RunStore, DeployError};
