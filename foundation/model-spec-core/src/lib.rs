pub mod cost;
pub mod limit;
pub mod model;
pub mod parser;
pub mod provider;
pub mod registry;
pub mod tier;

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
pub use registry::{CachedModelList, CombinedModelList, ModelEntry, ProviderConfig};
pub use tier::{pick_best_for_tier, ModelTier};

#[cfg(feature = "tier")]
pub mod model_registry;
#[cfg(feature = "tier")]
pub(crate) mod tier_error;
#[cfg(feature = "tier")]
pub mod tier_plan;
#[cfg(feature = "tier")]
pub mod tier_resolve;

#[cfg(feature = "tier")]
pub use model_registry::ModelRegistry;
#[cfg(feature = "tier")]
pub use tier_plan::{tier_plans, TierPlan};
#[cfg(feature = "tier")]
pub use tier_resolve::{
    resolve_from_plan, resolve_from_spec, resolve_tier, resolve_tier_intelligent,
    DefaultTierResolver, ResolvedTierModel, TierResolver,
};
