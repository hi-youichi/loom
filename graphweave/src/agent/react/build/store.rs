//! Builds vector store for long-term memory from ReactBuildConfig.

use std::sync::Arc;

use crate::error::AgentError;

use super::super::config::ReactBuildConfig;

pub(crate) fn build_store(
    config: &ReactBuildConfig,
    _db_path: &str,
) -> Result<Option<Arc<dyn crate::memory::Store>>, AgentError> {
    match build_vector_store(config) {
        Ok(store) => Ok(Some(store)),
        Err(_) => Ok(None),
    }
}

fn build_vector_store(
    config: &ReactBuildConfig,
) -> Result<Arc<dyn crate::memory::Store>, AgentError> {
    use crate::memory::{InMemoryVectorStore, OpenAIEmbedder};
    use async_openai::config::OpenAIConfig;

    let api_key = config
        .embedding_api_key
        .as_deref()
        .or(config.openai_api_key.as_deref())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AgentError::ExecutionFailed(
                "embedding requires EMBEDDING_API_KEY or OPENAI_API_KEY".into(),
            )
        })?;
    let model = config
        .embedding_model
        .as_deref()
        .or(config.model.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("text-embedding-3-small");
    let mut openai_config = OpenAIConfig::new().with_api_key(api_key);
    let base = config
        .embedding_base_url
        .as_deref()
        .or(config.openai_base_url.as_deref());
    if let Some(b) = base.filter(|s| !s.is_empty()) {
        let b = b.trim_end_matches('/');
        openai_config = openai_config.with_api_base(b);
    }
    let embedder = OpenAIEmbedder::with_config(openai_config, model);
    let store = InMemoryVectorStore::new(Arc::new(embedder));
    Ok(Arc::new(store) as Arc<dyn crate::memory::Store>)
}
