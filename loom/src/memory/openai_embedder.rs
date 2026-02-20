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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn read_http_request(stream: &mut TcpStream) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = stream.read(&mut tmp).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = pos + 4;
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = buf[header_end..].to_vec();
                while body.len() < content_length {
                    let m = stream.read(&mut tmp).await.unwrap();
                    if m == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..m]);
                }
                let _ = &body[..content_length];
                return;
            }
        }
    }

    async fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
            status,
            body.len(),
            body
        );
        stream.write_all(resp.as_bytes()).await.unwrap();
    }

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

    #[tokio::test]
    async fn embed_and_embed_one_work_with_local_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_http_request(&mut stream).await;
                let body = if idx == 0 {
                    serde_json::json!({
                        "object":"list",
                        "data":[{"object":"embedding","index":0,"embedding":[0.1,0.2,0.3]}],
                        "model":"text-embedding-3-small",
                        "usage":{"prompt_tokens":1,"total_tokens":1}
                    })
                    .to_string()
                } else {
                    serde_json::json!({
                        "object":"list",
                        "data":[
                            {"object":"embedding","index":0,"embedding":[1.0,1.1]},
                            {"object":"embedding","index":1,"embedding":[2.0,2.1]}
                        ],
                        "model":"text-embedding-3-small",
                        "usage":{"prompt_tokens":2,"total_tokens":2}
                    })
                    .to_string()
                };
                write_http_response(&mut stream, "200 OK", &body).await;
            }
        });

        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base(format!("http://{}", addr));
        let embedder = OpenAIEmbedder::with_config(config, "text-embedding-3-small");
        let one = embedder.embed_one("hello").await.unwrap();
        assert_eq!(one, vec![0.1, 0.2, 0.3]);
        let many = embedder.embed(&["a", "b"]).await.unwrap();
        assert_eq!(many.len(), 2);
        assert_eq!(many[0], vec![1.0, 1.1]);
        assert_eq!(many[1], vec![2.0, 2.1]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn embed_one_returns_error_when_response_has_no_data() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_request(&mut stream).await;
            let body = serde_json::json!({
                "object":"list",
                "data":[],
                "model":"text-embedding-3-small",
                "usage":{"prompt_tokens":0,"total_tokens":0}
            })
            .to_string();
            write_http_response(&mut stream, "200 OK", &body).await;
        });

        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base(format!("http://{}", addr));
        let embedder = OpenAIEmbedder::with_config(config, "text-embedding-3-small");
        let err = embedder.embed_one("hello").await.unwrap_err();
        assert!(err.to_string().contains("No embedding returned"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn embed_returns_error_on_http_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_request(&mut stream).await;
            write_http_response(
                &mut stream,
                "500 Internal Server Error",
                r#"{"error":{"message":"boom"}}"#,
            )
            .await;
        });

        let config = OpenAIConfig::new()
            .with_api_key("test-key")
            .with_api_base(format!("http://{}", addr));
        let embedder = OpenAIEmbedder::with_config(config, "text-embedding-3-small");
        let err = embedder.embed(&["hello"]).await.unwrap_err();
        assert!(err.to_string().contains("OpenAI API error"));
        server.await.unwrap();
    }
}
