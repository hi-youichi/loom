pub mod error;
pub mod models_dev;
pub mod registry;
pub mod tier;

pub mod resolver;

pub use models_dev::resolver::{
    HttpClient, ModelsDevResolver, ReqwestHttpClient, DEFAULT_MODELS_DEV_URL,
};

pub use models_dev::yaml_provider::{
    load_yaml_plugins, YamlModelDef, YamlPluginFile, YamlProviderMeta,
};


pub use models_dev::{
    Cost, CostTier, CostTierInfo, Experimental, ExperimentalMode,
    ExperimentalProviderConfig, Interleaved, InterleavedField, Modalities,
    ModalityType, ModelLimit, Model, ModelProviderConfig, ModelStatus,
    Provider, ProviderShape, ReasoningEffort, ReasoningOption,
};
pub use models_dev::parser::{
    extract_provider_api_from_models_dev_json, parse_all_providers, parse_model, parse_model_limit,
    parse_provider,
};
pub use registry::{CachedModelList, CombinedModelList, ModelEntry, ProviderConfig};
pub use tier::{pick_best_for_tier, ModelTier};

pub mod model_registry;
pub(crate) mod tier_error;
pub mod tier_plan;
pub mod tier_resolve;

pub use model_registry::ModelRegistry;
pub use tier_plan::{tier_plans, TierPlan};
pub use tier_resolve::{
    resolve_from_plan, resolve_from_spec, resolve_tier, resolve_tier_intelligent,
    DefaultTierResolver, ResolvedTierModel, TierResolver,
};
