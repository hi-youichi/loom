//! Model registry types: re-exported from `model-spec-core`.
//!
//! `ProviderConfig`, `ModelEntry`, `CachedModelList`, and `CombinedModelList`
//! live in `model-spec-core` (L0 leaf) so that crates can depend on them
//! without pulling in the full `loom-llm`.

pub use model_spec_core::registry::{
    CachedModelList, CombinedModelList, ModelEntry, ProviderConfig, DEFAULT_CACHE_TTL,
    DEFAULT_PROVIDER_CACHE_TTL,
};
