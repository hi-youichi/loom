//! models.dev data abstraction: schema types, JSON parser, and HTTP resolver.
//!
//! This module is the single entry point for all models.dev-related code:
//!
//! - **Schema types**: [`Provider`], [`Model`], [`Cost`], [`ModelLimit`], [`Modalities`]
//! - **JSON parser**: [`parse_provider`], [`parse_model`], [`parse_all_providers`]
//! - **HTTP resolver**: [`ModelsDevResolver`], [`HttpClient`], [`ReqwestHttpClient`]

pub mod cost;
pub mod limit;
pub mod model;
pub mod parser;
pub mod provider;
#[cfg(feature = "resolver")]
pub mod resolver;

pub use cost::Cost;
pub use limit::{Modalities, ModalityType, ModelLimit};
pub use model::Model;
pub use parser::{
    extract_provider_api_from_models_dev_json, parse_all_providers, parse_model, parse_model_limit,
    parse_provider,
};
pub use provider::Provider;

#[cfg(feature = "resolver")]
pub use resolver::{
    HttpClient, ModelsDevResolver, ReqwestHttpClient, DEFAULT_MODELS_DEV_URL,
};
