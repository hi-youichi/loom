//! OpenAI Embeddings implementation of [`Embedder`].
//!
//! Uses OpenAI's Embeddings API to generate vector embeddings from text.
//! Supports models like `text-embedding-3-small`, `text-embedding-3-large`, and `text-embedding-ada-002`.
//!
//! **Interaction**: Implements [`Embedder`]; used by [`LanceStore`](crate::memory::LanceStore) for vector search.
//!
//! Requires `OPENAI_API_KEY` environment variable (or custom config).

use async_openai::{
    config::OpenAIConfig,
    types::embeddings::{CreateEmbeddingRequest, EmbeddingInput},
    Client,
};

use crate::memory::store::StoreError;

/// OpenAI Embeddings client implementing [`Embedder`].
///
/// Generates vector embeddings using OpenAI's API. Default model is `text-embedding-3-small` (1536 dimensions).
///
/// **Interaction**: Implements [`Embedder`]; used by [`LanceStore`](crate::memory::LanceStore).
///
/// # Examples
///
/// ```ignore
/// use loom::memory::OpenAIEmbedder;
///
/// let embedder = OpenAIEmbedder::new("text-embedding-3-small");
/// let vectors = embedder.embed(&["Hello, world!"]).await?;
/// ```
///
/// # Runtime behaviour
///
/// [`embed`](Embedder::embed) is async and can be awaited directly from async Store methods.
/// Safe to use inside tokio runtime (e.g. from ReAct tools like `remember`).
pub struct OpenAIEmbedder {
    config: OpenAIConfig,
    model: String,
    dimensions: usize,
}

impl OpenAIEmbedder {
    /// Creates a new OpenAI embedder with the specified model.
    ///
    /// The API key is read from `OPENAI_API_KEY` environment variable.
    ///
    /// # Arguments
    ///
    /// * `model` - The embedding model to use (e.g., "text-embedding-3-small", "text-embedding-ada-002").
    ///
    /// # Supported models and their dimensions:
    ///
    /// - `text-embedding-3-small`: 1536 dimensions (default, cost-effective)
    /// - `text-embedding-3-large`: 3072 dimensions (higher quality)
    /// - `text-embedding-ada-002`: 1536 dimensions (legacy model)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let embedder = OpenAIEmbedder::new("text-embedding-3-small");
    /// ```
    pub fn new(model: impl Into<String>) -> Self {
        let model = model.into();
        let dimensions = Self::get_model_dimensions(&model);
        Self {
            config: OpenAIConfig::new(),
            model,
            dimensions,
        }
    }

    /// Creates a new OpenAI embedder with custom configuration.
    ///
    /// Allows specifying a custom API key, base URL, or other OpenAI-compatible provider settings.
    ///
    /// # Arguments
    ///
    /// * `config` - Custom OpenAI configuration (e.g., different API key or base URL).
    /// * `model` - The embedding model to use.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use async_openai::config::OpenAIConfig;
    /// use loom::memory::OpenAIEmbedder;
    ///
    /// let config = OpenAIConfig::new().with_api_key("sk-...");
    /// let embedder = OpenAIEmbedder::with_config(config, "text-embedding-3-small");
    /// ```
    pub fn with_config(config: OpenAIConfig, model: impl Into<String>) -> Self {
        let model = model.into();
        let dimensions = Self::get_model_dimensions(&model);
        Self {
            config,
            model,
            dimensions,
        }
    }

    /// Returns the vector dimension for a given model name.
    ///
    /// # Supported models:
    ///
    /// - `text-embedding-3-small`: 1536
    /// - `text-embedding-3-large`: 3072
    /// - `text-embedding-ada-002`: 1536
    fn get_model_dimensions(model: &str) -> usize {
        match model {
            "text-embedding-3-large" => 3072,
            "text-embedding-3-small" | "text-embedding-ada-002" => 1536,
            _ => 1536,
        }
    }

    /// Embeds a single text string.
    ///
    /// This is a convenience method for embedding a single text. For multiple texts,
    /// use [`embed`](Embedder::embed) which can batch multiple requests.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to embed.
    ///
    /// # Returns
    ///
    /// A vector of floats representing the text embedding.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let embedder = OpenAIEmbedder::new("text-embedding-3-small");
    /// let vector = embedder.embed_one("Hello, world!").await?;
    /// ```
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, StoreError> {
        let client = Client::with_config(self.config.clone());
        let request = CreateEmbeddingRequest {
            input: EmbeddingInput::String(text.to_string()),
            model: self.model.clone(),
            ..Default::default()
        };

        let response = client
            .embeddings()
            .create(request)
            .await
            .map_err(|e| StoreError::EmbeddingError(format!("OpenAI API error: {}", e)))?;

        if response.data.is_empty() {
            return Err(StoreError::EmbeddingError(
                "No embedding returned".to_string(),
            ));
        }

        Ok(response.data[0].embedding.clone())
    }
}

#[async_trait::async_trait]
impl crate::memory::Embedder for OpenAIEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, StoreError> {
        let inputs: Vec<String> = texts.iter().map(|&s| s.to_string()).collect();
        let input = if inputs.len() == 1 {
            EmbeddingInput::String(inputs[0].clone())
        } else {
            EmbeddingInput::StringArray(inputs)
        };

        let request = CreateEmbeddingRequest {
            input,
            model: self.model.clone(),
            ..Default::default()
        };

        let client = Client::with_config(self.config.clone());
        let response = client
            .embeddings()
            .create(request)
            .await
            .map_err(|e| StoreError::EmbeddingError(format!("OpenAI API error: {}", e)))?;

        Ok(response.data.into_iter().map(|e| e.embedding).collect())
    }

    fn dimension(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Embedder;

    #[test]
    fn test_get_model_dimensions() {
        assert_eq!(
            OpenAIEmbedder::get_model_dimensions("text-embedding-3-small"),
            1536
        );
        assert_eq!(
            OpenAIEmbedder::get_model_dimensions("text-embedding-3-large"),
            3072
        );
        assert_eq!(
            OpenAIEmbedder::get_model_dimensions("text-embedding-ada-002"),
            1536
        );
        assert_eq!(OpenAIEmbedder::get_model_dimensions("unknown-model"), 1536);
    }

    #[test]
    fn test_embedder_creation() {
        let embedder = OpenAIEmbedder::new("text-embedding-3-small");
        assert_eq!(embedder.model, "text-embedding-3-small");
        assert_eq!(embedder.dimension(), 1536);

        let embedder = OpenAIEmbedder::new("text-embedding-3-large");
        assert_eq!(embedder.dimension(), 3072);
    }

    #[test]
    fn test_embedder_with_custom_config() {
        use async_openai::config::OpenAIConfig;

        let config = OpenAIConfig::new().with_api_key("test-key");
        let embedder = OpenAIEmbedder::with_config(config, "text-embedding-3-small");
        assert_eq!(embedder.model, "text-embedding-3-small");
        assert_eq!(embedder.dimension(), 1536);
    }

    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY"]
    async fn test_openai_embed() {
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set for this test");

        let embedder = OpenAIEmbedder::new("text-embedding-3-small");
        let texts = vec!["Hello, world!", "The quick brown fox"];

        // Use embed_one().await to avoid calling sync embed() (which uses block_on) inside tokio runtime.
        let mut vectors = Vec::with_capacity(texts.len());
        for text in &texts {
            vectors.push(embedder.embed_one(text).await.unwrap());
        }

        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].len(), 1536);
        assert_eq!(vectors[1].len(), 1536);

        let one_vector = embedder.embed_one("Single text").await.unwrap();

        assert_eq!(one_vector.len(), 1536);
    }

    /// Verifies that async [`embed`](Embedder::embed) can be awaited from within a tokio runtime
    /// (e.g. from store.put inside a ReAct tool like `remember`).
    #[tokio::test]
    #[ignore = "Requires OPENAI_API_KEY"]
    async fn test_embed_from_within_tokio_runtime() {
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set for this test");

        let embedder = OpenAIEmbedder::new("text-embedding-3-small");
        let vectors = embedder.embed(&["hello from tokio"]).await.unwrap();
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), 1536);
    }
}
