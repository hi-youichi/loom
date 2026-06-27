//! Tier resolution and model registry for Loom.
//!
//! This crate provides:
//! - **Tier plans**: Static tier-to-model mappings loaded from embedded TOML
//! - **Tier resolution**: Intelligent resolution combining plans, model spec, and provider APIs
//! - **Model registry**: Runtime caching of model lists from providers and models.dev
//! - **Provider loading**: Provider configuration from environment/config
//! - **Model service**: HTTP service for managing available models
//! - **Tier resolver**: `TierResolver` trait and `DefaultTierResolver` implementation
//! - **Tier apply**: `resolve_tier_and_build_config` for applying tier resolution to configs

pub mod plan;
pub mod resolve;
pub mod model_registry;
pub mod provider;
pub mod services;
pub mod resolver;
pub mod apply;

// Re-export key types from model-spec-core
pub use model_spec_core::registry::{ModelEntry, ProviderConfig, CachedModelList, CombinedModelList};

pub use plan::TierPlan;
pub use resolve::{resolve_tier_intelligent, resolve_for_model, resolve_tier_to_model_id};
pub use model_registry::ModelRegistry;
pub use provider::load_provider_configs;
pub use services::ModelService;
pub use resolver::{DefaultTierResolver, ResolvedTierModel, TierResolver, resolve_tier_for_config};
pub use apply::{resolve_tier_and_build_config, resolve_tier_and_build_config_with_resolver};
