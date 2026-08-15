use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ExtensionContext, ExtensionError, ExtensionHandler};

const MIN_TOKENS: u32 = 64;
const MAX_TOKENS: u32 = 2048;
const DEFAULT_TOKENS: u32 = 1024;
const MAX_INPUT_BYTES: usize = 1_048_576;
const MAX_INSTRUCTIONS_BYTES: usize = 65_536;
const MAX_SESSION_ID_BYTES: usize = 256;
const GENERATION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmallModelDescribeRequest {
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmallModelDescribeResponse {
    pub available: bool,
    pub preferred_provider: Option<String>,
    pub preferred_model: Option<String>,
    pub fallback_provider: Option<String>,
    pub fallback_model: Option<String>,
    pub restrict_to_preferred_provider: bool,
    pub supported_tasks: Vec<String>,
    pub max_tokens: u32,
    pub estimated_latency_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SmallModelGenerateRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    pub task: String,
    pub input: String,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub restrict_to_preferred_provider: bool,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    DEFAULT_TOKENS
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmallModelGenerateResponse {
    pub result: Option<String>,
    pub model_used: Option<String>,
    pub provider_used: Option<String>,
    pub fell_back: bool,
    pub tokens_used: Option<TokenUsage>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsage {
    pub input: u32,
    pub output: u32,
}

#[derive(Debug, Clone)]
pub struct SmallModelConfiguration {
    pub preferred: Option<ProviderSpec>,
    pub fallback: Option<ProviderSpec>,
    pub estimated_latency_ms: u32,
}

pub type SmallModelConfig = SmallModelConfiguration;

#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub provider: String,
    pub model: String,
}

pub type ProviderConfig = ProviderSpec;

#[derive(Debug, Clone)]
pub struct ProviderGeneration {
    pub result: String,
    pub model: String,
    pub provider: String,
    pub tokens: TokenUsage,
}

#[derive(Debug)]
pub enum SessionResolutionError {
    NotFound,
    Internal,
    Unauthorized,
}

#[derive(Debug)]
pub enum ProviderError {
    NotFound,
    RateLimited,
    SafetyRejected,
    Failed,
}

#[async_trait]
pub trait SmallModelSessionResolver: Send + Sync {
    async fn resolve(
        &self,
        session_id: Option<&str>,
        ctx: &ExtensionContext,
    ) -> Result<SmallModelConfiguration, SessionResolutionError>;
}

#[async_trait]
pub trait SmallModelProvider: Send + Sync {
    async fn generate(
        &self,
        provider: &ProviderSpec,
        request: &SmallModelGenerateRequest,
        max_tokens: u32,
    ) -> Result<ProviderGeneration, ProviderError>;
}

struct DefaultSessionResolver;

#[async_trait]
impl SmallModelSessionResolver for DefaultSessionResolver {
    async fn resolve(
        &self,
        session_id: Option<&str>,
        ctx: &ExtensionContext,
    ) -> Result<SmallModelConfiguration, SessionResolutionError> {
        if ctx.principal.trim().is_empty() {
            return Err(SessionResolutionError::Unauthorized);
        }
        if let Some(requested) = session_id {
            if requested != ctx.session_id.as_deref().unwrap_or_default() {
                return Err(SessionResolutionError::NotFound);
            }
        }
        Ok(SmallModelConfiguration {
            preferred: Some(ProviderSpec {
                provider: "default".into(),
                model: "small-model".into(),
            }),
            fallback: Some(ProviderSpec {
                provider: "fallback".into(),
                model: "small-model-fallback".into(),
            }),
            estimated_latency_ms: 2000,
        })
    }
}

struct DefaultProvider;

#[async_trait]
impl SmallModelProvider for DefaultProvider {
    async fn generate(
        &self,
        provider: &ProviderSpec,
        request: &SmallModelGenerateRequest,
        max_tokens: u32,
    ) -> Result<ProviderGeneration, ProviderError> {
        let result = request.input.trim().to_string();
        if result.is_empty() {
            return Err(ProviderError::Failed);
        }
        Ok(ProviderGeneration {
            result,
            model: provider.model.clone(),
            provider: provider.provider.clone(),
            tokens: TokenUsage {
                input: request.input.len().min(u32::MAX as usize) as u32,
                output: max_tokens,
            },
        })
    }
}

pub struct SmallModelHandler {
    resolver: Arc<dyn SmallModelSessionResolver>,
    provider: Arc<dyn SmallModelProvider>,
}

impl SmallModelHandler {
    pub fn new() -> Self {
        Self::with_dependencies(Arc::new(DefaultSessionResolver), Arc::new(DefaultProvider))
    }

    pub fn with_dependencies(
        resolver: Arc<dyn SmallModelSessionResolver>,
        provider: Arc<dyn SmallModelProvider>,
    ) -> Self {
        Self { resolver, provider }
    }

    fn internal() -> ExtensionError {
        ExtensionError {
            code: -32603,
            message: "internal_error".into(),
            data: None,
        }
    }

    fn provider_error() -> ExtensionError {
        ExtensionError {
            code: -32009,
            message: "provider_error".into(),
            data: None,
        }
    }

    fn timeout() -> ExtensionError {
        ExtensionError {
            code: -32010,
            message: "timeout".into(),
            data: None,
        }
    }

    fn rate_limited() -> ExtensionError {
        ExtensionError {
            code: -32008,
            message: "rate_limited".into(),
            data: None,
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, ExtensionError> {
        if !params.is_object() {
            return Err(ExtensionError::invalid_params("params must be an object"));
        }
        serde_json::from_value(params).map_err(|_| ExtensionError::invalid_params("invalid params"))
    }

    fn validate_session(session_id: Option<&str>) -> Result<(), ExtensionError> {
        if session_id
            .is_some_and(|value| value.trim().is_empty() || value.len() > MAX_SESSION_ID_BYTES)
        {
            return Err(ExtensionError::invalid_params("sessionId is invalid"));
        }
        Ok(())
    }

    fn validate_request(request: &SmallModelGenerateRequest) -> Result<u32, ExtensionError> {
        if request.task != "commit_message"
            && request.task != "pr_description"
            && request.task != "recap"
            && request.task != "general"
        {
            return Err(ExtensionError::invalid_params("unsupported task"));
        }
        if request.input.trim().is_empty() || request.input.len() > MAX_INPUT_BYTES {
            return Err(ExtensionError::invalid_params("input is invalid"));
        }
        if request
            .instructions
            .as_ref()
            .is_some_and(|value| value.len() > MAX_INSTRUCTIONS_BYTES)
        {
            return Err(ExtensionError::invalid_params("instructions is too long"));
        }
        Ok(request.max_tokens.clamp(MIN_TOKENS, MAX_TOKENS))
    }

    fn usable(provider: Option<&ProviderSpec>) -> bool {
        provider.is_some_and(|value| {
            !value.provider.trim().is_empty() && !value.model.trim().is_empty()
        })
    }

    fn empty_response() -> Result<Value, ExtensionError> {
        serde_json::to_value(SmallModelGenerateResponse {
            result: None,
            model_used: None,
            provider_used: None,
            fell_back: false,
            tokens_used: None,
            generated_at: Utc::now(),
        })
        .map_err(|_| Self::internal())
    }

    fn response(generation: ProviderGeneration, fell_back: bool) -> Result<Value, ExtensionError> {
        if generation.result.trim().is_empty()
            || generation.model.trim().is_empty()
            || generation.provider.trim().is_empty()
        {
            return Err(Self::provider_error());
        }
        serde_json::to_value(SmallModelGenerateResponse {
            result: Some(generation.result),
            model_used: Some(generation.model),
            provider_used: Some(generation.provider),
            fell_back,
            tokens_used: Some(generation.tokens),
            generated_at: Utc::now(),
        })
        .map_err(|_| Self::internal())
    }
}

#[async_trait]
impl ExtensionHandler for SmallModelHandler {
    async fn handle(
        &self,
        method: &str,
        params: Value,
        ctx: &ExtensionContext,
    ) -> Result<Value, ExtensionError> {
        match method {
            "describe" => {
                let request: SmallModelDescribeRequest = Self::parse(params)?;
                Self::validate_session(request.session_id.as_deref())?;
                let session_id = request.session_id.as_deref().or(ctx.session_id.as_deref());
                let config = self
                    .resolver
                    .resolve(session_id, ctx)
                    .await
                    .map_err(|error| match error {
                        SessionResolutionError::NotFound => {
                            ExtensionError::not_found("session not found")
                        }
                        SessionResolutionError::Unauthorized => {
                            ExtensionError::forbidden("small-model authorization required")
                        }
                        SessionResolutionError::Internal => Self::internal(),
                    })?;
                let response = SmallModelDescribeResponse {
                    available: Self::usable(config.preferred.as_ref())
                        || Self::usable(config.fallback.as_ref()),
                    preferred_provider: config.preferred.as_ref().map(|v| v.provider.clone()),
                    preferred_model: config.preferred.as_ref().map(|v| v.model.clone()),
                    fallback_provider: config.fallback.as_ref().map(|v| v.provider.clone()),
                    fallback_model: config.fallback.as_ref().map(|v| v.model.clone()),
                    restrict_to_preferred_provider: false,
                    supported_tasks: vec![
                        "commit_message".into(),
                        "pr_description".into(),
                        "recap".into(),
                        "general".into(),
                    ],
                    max_tokens: DEFAULT_TOKENS,
                    estimated_latency_ms: config.estimated_latency_ms,
                };
                serde_json::to_value(response).map_err(|_| Self::internal())
            }
            "generate" => {
                let request: SmallModelGenerateRequest = Self::parse(params)?;
                Self::validate_session(request.session_id.as_deref())?;
                let max_tokens = Self::validate_request(&request)?;
                let session_id = request.session_id.as_deref().or(ctx.session_id.as_deref());
                let config = self
                    .resolver
                    .resolve(session_id, ctx)
                    .await
                    .map_err(|error| match error {
                        SessionResolutionError::NotFound => {
                            ExtensionError::not_found("session not found")
                        }
                        SessionResolutionError::Unauthorized => {
                            ExtensionError::forbidden("small-model authorization required")
                        }
                        SessionResolutionError::Internal => Self::internal(),
                    })?;
                let Some(preferred) = config.preferred.clone() else {
                    return Self::empty_response();
                };
                let preferred_result = tokio::time::timeout(
                    GENERATION_TIMEOUT,
                    self.provider.generate(&preferred, &request, max_tokens),
                )
                .await;
                let preferred_rate_limited =
                    matches!(&preferred_result, Ok(Err(ProviderError::RateLimited)));
                match preferred_result {
                    Ok(Ok(generation)) => Self::response(generation, false),
                    Err(_) => Err(Self::timeout()),
                    Ok(Err(ProviderError::NotFound)) if request.restrict_to_preferred_provider => {
                        Self::empty_response()
                    }
                    Ok(Err(ProviderError::RateLimited))
                        if request.restrict_to_preferred_provider =>
                    {
                        Self::empty_response()
                    }
                    Ok(Err(ProviderError::NotFound)) | Ok(Err(ProviderError::RateLimited)) => {
                        let Some(fallback) = config.fallback else {
                            return if preferred_rate_limited {
                                Err(Self::rate_limited())
                            } else {
                                Self::empty_response()
                            };
                        };
                        match tokio::time::timeout(
                            GENERATION_TIMEOUT,
                            self.provider.generate(&fallback, &request, max_tokens),
                        )
                        .await
                        {
                            Err(_) => Err(Self::timeout()),
                            Ok(Ok(generation)) => Self::response(generation, true),
                            Ok(Err(ProviderError::RateLimited)) => Err(Self::rate_limited()),
                            Ok(Err(ProviderError::NotFound)) => Self::empty_response(),
                            Ok(Err(ProviderError::SafetyRejected | ProviderError::Failed)) => {
                                Err(Self::provider_error())
                            }
                        }
                    }
                    Ok(Err(ProviderError::SafetyRejected | ProviderError::Failed)) => {
                        Err(Self::provider_error())
                    }
                }
            }
            _ => Err(ExtensionError::method_not_found()),
        }
    }

    fn capabilities(&self) -> Value {
        serde_json::json!({ "describe": true, "generate": true })
    }
}

impl Default for SmallModelHandler {
    fn default() -> Self {
        Self::new()
    }
}
