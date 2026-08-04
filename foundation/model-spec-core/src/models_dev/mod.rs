//! models.dev data abstraction: schema types, JSON parser, and HTTP resolver.
//!
//! This module is the single entry point for all models.dev-related code:
//!
//! - **Schema types**: [`Provider`], [`Model`], [`Cost`], [`CostTier`],
//!   [`ModelLimit`], [`Modalities`], [`ReasoningOption`], [`Interleaved`],
//!   [`ModelStatus`], [`Experimental`], [`ModelProviderConfig`]
//! - **JSON parser**: [`parse_provider`], [`parse_model`], [`parse_all_providers`]
//! - **HTTP resolver**: [`ModelsDevResolver`], [`HttpClient`], [`ReqwestHttpClient`]

pub mod cost;
pub mod limit;
pub mod model;
pub mod parser;
pub mod provider;
pub mod bundled_providers;
pub mod resolver;
pub mod yaml_provider;

pub use cost::{Cost, CostTier, CostTierInfo};
pub use limit::{Modalities, ModalityType, ModelLimit};
pub use model::{
    Experimental, ExperimentalMode, ExperimentalProviderConfig, Interleaved,
    InterleavedField, Model, ModelProviderConfig, ModelStatus, ProviderShape,
    ReasoningEffort, ReasoningOption,
};
pub use parser::{
    extract_provider_api_from_models_dev_json, parse_all_providers, parse_model, parse_model_limit,
    parse_provider,
};
pub use provider::Provider;

pub use resolver::{
    HttpClient, ModelsDevResolver, ReqwestHttpClient, DEFAULT_MODELS_DEV_URL,
};
