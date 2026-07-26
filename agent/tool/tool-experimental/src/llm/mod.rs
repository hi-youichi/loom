//! LlmTool — direct LLM invocation with multi-provider discovery.
//!
//! Exposes a single `llm` tool that supports four actions:
//! - `invoke` (default): call an LLM with messages, supports multimodal inputs
//! - `list_providers`: list available providers (from pre-loaded data)
//! - `list_models`: list models with detailed metadata from models.dev
//! - `model_info`: get detailed info for a specific model
//!
//! The provider/model catalog is pre-loaded at agent build time and held in
//! `Arc<LlmToolData>`, so all three discovery actions are pure in-memory
//! lookups with zero network I/O at runtime.

pub mod content;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use model_spec_core::Model;
use serde_json::{json, Value};

use loom_llm::message::{ContentPart, Message, UserContent};
use loom_llm::{ChatOpenAICompat, LlmClient, ToolCallContent, ToolSourceError, ToolSpec};
use tool_core::tool_name::TOOL_LLM;
use tool_core::{Tool, ToolCallContext};

use self::content::parse_message;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Pre-loaded provider + model catalog passed from the build pipeline.
///
/// At build time, `tool_source.rs` loads the list of `ProviderConfig` from
/// XDG config, fetches the models.dev metadata for each, and packages the
/// results into this struct. The tool holds it behind an `Arc` and performs
/// only in-memory lookups thereafter.
pub struct LlmToolData {
    /// Default provider name when the agent omits `provider`.
    pub default_provider: String,
    /// Default model name when the agent omits `model` (in `invoke`).
    pub default_model: String,
    /// All configured providers, each with their pre-loaded model catalog.
    pub providers: Vec<LlmProviderData>,
}

/// A single provider's connection info plus its pre-loaded model catalog.
pub struct LlmProviderData {
    /// Provider name (e.g. `"openai"`, `"bigmodel"`).
    pub name: String,
    /// OpenAI-compatible API base URL.
    pub base_url: String,
    /// API key.
    pub api_key: String,
    /// Models available on this provider, sourced from models.dev.
    pub models: Vec<Model>,
}

// ---------------------------------------------------------------------------
// LlmToolConfig
// ---------------------------------------------------------------------------

/// Per-instance safety/sanity limits for the tool.
#[derive(Clone, Debug)]
pub struct LlmToolConfig {
    /// Max number of messages per call (default 50).
    pub max_messages: usize,
    /// Max total text characters across all messages (default 100_000).
    pub max_text_chars: usize,
    /// Max file size in bytes for `_path` content parts (default 10 MB).
    pub max_file_size: usize,
    /// Optional allow-list of model IDs; `None` means no restriction.
    pub allowed_models: Option<Vec<String>>,
}

impl Default for LlmToolConfig {
    fn default() -> Self {
        Self {
            max_messages: 50,
            max_text_chars: 100_000,
            max_file_size: 10_000_000,
            allowed_models: None,
        }
    }
}

// ---------------------------------------------------------------------------
// LlmTool
// ---------------------------------------------------------------------------

/// LlmTool — direct LLM invocation with provider/model discovery.
pub struct LlmTool {
    /// Pre-loaded provider + model catalog.
    data: Arc<LlmToolData>,
    /// Working folder for `_path` content part resolution.
    working_folder: Option<Arc<PathBuf>>,
    /// Per-instance safety/sanity limits.
    config: LlmToolConfig,
}

impl LlmTool {
    /// Create a new `LlmTool` from pre-loaded data.
    pub fn new(
        data: Arc<LlmToolData>,
        working_folder: Option<Arc<PathBuf>>,
        config: LlmToolConfig,
    ) -> Self {
        Self {
            data,
            working_folder,
            config,
        }
    }

    /// Look up a provider by name; returns `None` if not found.
    fn find_provider(&self, name: &str) -> Option<&LlmProviderData> {
        self.data.providers.iter().find(|p| p.name == name)
    }

    // ── list_providers ──
    fn handle_list_providers(&self) -> Result<ToolCallContent, ToolSourceError> {
        let providers: Vec<Value> = self
            .data
            .providers
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "models_count": p.models.len(),
                })
            })
            .collect();
        Ok(ToolCallContent::text(
            json!({ "providers": providers }).to_string(),
        ))
    }

    // ── list_models ──
    fn handle_list_models(
        &self,
        provider: Option<&str>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let provider_name = provider.unwrap_or(&self.data.default_provider);
        let provider_data = self.find_provider(provider_name).ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("provider '{}' 不存在", provider_name))
        })?;
        let models: Vec<Value> = provider_data.models.iter().map(model_to_json).collect();
        Ok(ToolCallContent::text(
            json!({
                "provider": provider_name,
                "models": models,
            })
            .to_string(),
        ))
    }

    // ── model_info ──
    fn handle_model_info(
        &self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let provider_name = provider.unwrap_or(&self.data.default_provider);
        let model_id = model
            .ok_or_else(|| ToolSourceError::InvalidInput("model_info 需要 model 参数".into()))?;
        let provider_data = self.find_provider(provider_name).ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("provider '{}' 不存在", provider_name))
        })?;
        let model_data = provider_data
            .models
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| {
                ToolSourceError::InvalidInput(format!(
                    "model '{}' 不存在于 provider '{}'",
                    model_id, provider_name
                ))
            })?;
        Ok(ToolCallContent::text(model_to_json(model_data).to_string()))
    }

    // ── invoke ──
    async fn handle_invoke(&self, args: Value) -> Result<ToolCallContent, ToolSourceError> {
        // 1. Resolve provider.
        let provider_name = args
            .get("provider")
            .and_then(|p| p.as_str())
            .unwrap_or(&self.data.default_provider);
        let provider = self.find_provider(provider_name).ok_or_else(|| {
            ToolSourceError::InvalidInput(format!("provider '{}' 不存在", provider_name))
        })?;

        // 2. Parse messages.
        let messages_raw = args
            .get("messages")
            .and_then(|m| m.as_array())
            .ok_or_else(|| ToolSourceError::InvalidInput("缺少 messages".into()))?;
        if messages_raw.is_empty() {
            return Err(ToolSourceError::InvalidInput("messages 为空".into()));
        }
        if messages_raw.len() > self.config.max_messages {
            return Err(ToolSourceError::InvalidInput(format!(
                "消息数 {} 超过上限 {}",
                messages_raw.len(),
                self.config.max_messages
            )));
        }
        let wf = self.working_folder.as_deref().map(|p| p.as_path());
        let messages: Vec<Message> = messages_raw
            .iter()
            .map(|m| parse_message(m, wf, self.config.max_file_size))
            .collect::<Result<_, _>>()?;

        // 3. Sanity-check total text length.
        let total_text: usize = messages
            .iter()
            .map(|m| match m {
                Message::System(s) => s.len(),
                Message::User(UserContent::Text(s)) => s.len(),
                Message::User(UserContent::Multimodal(parts)) => parts
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.len()),
                        _ => None,
                    })
                    .sum(),
                Message::Assistant(p) => p.content.len(),
                _ => 0,
            })
            .sum();
        if total_text > self.config.max_text_chars {
            return Err(ToolSourceError::InvalidInput(format!(
                "文本总量 {} 超过上限 {} 字符",
                total_text, self.config.max_text_chars
            )));
        }

        // 4. Resolve model + allow-list check.
        let model = args
            .get("model")
            .and_then(|m| m.as_str())
            .map(String::from)
            .unwrap_or_else(|| self.data.default_model.clone());
        if let Some(ref allowed) = self.config.allowed_models {
            if !allowed.iter().any(|m| m == &model) {
                return Err(ToolSourceError::InvalidInput(format!(
                    "模型 {} 不在允许列表中",
                    model
                )));
            }
        }

        // 5. Optional generation params.
        let temperature = args
            .get("temperature")
            .and_then(|t| t.as_f64())
            .map(|f| f as f32);
        let max_tokens = args
            .get("max_tokens")
            .and_then(|t| t.as_u64())
            .map(|n| n as u32);
        let top_p = args.get("top_p").and_then(|t| t.as_f64()).map(|f| f as f32);
        let response_format = args.get("response_format").cloned();
        let reasoning_effort = args
            .get("reasoning_effort")
            .and_then(|r| r.as_str())
            .map(String::from);
        let seed = args.get("seed").and_then(|s| s.as_i64()).map(|n| n as u32);

        // 6. Build client + call.
        let mut client =
            ChatOpenAICompat::with_config(&provider.base_url, &provider.api_key, &model);
        if let Some(temp) = temperature {
            client = client.with_temperature(temp);
        }
        if let Some(max_tok) = max_tokens {
            client = client.with_max_tokens(max_tok);
        }
        if let Some(p) = top_p {
            client = client.with_top_p(p);
        }
        if let Some(ref rf) = response_format {
            client = client.with_response_format(rf.clone());
        }
        if let Some(ref effort) = reasoning_effort {
            client = client.with_reasoning_effort(effort);
        }
        if let Some(s) = seed {
            client = client.with_seed(s);
        }

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            client.invoke(&messages),
        )
        .await
        .map_err(|_| ToolSourceError::Transport("LLM 调用超时 (120s)".to_string()))?
        .map_err(|e| ToolSourceError::Transport(format!("LLM 调用失败: {}", e)))?;

        tracing::info!(
            tool = "llm",
            provider = %provider_name,
            model = %model,
            prompt_tokens = ?response.usage.as_ref().map(|u| u.prompt_tokens),
            completion_tokens = ?response.usage.as_ref().map(|u| u.completion_tokens),
            finish_reason = ?response.finish_reason,
            "llm tool invocation completed"
        );

        let result = json!({
            "content": response.content,
            "model": model,
            "usage": response.usage.as_ref().map(|u| json!({
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
            })),
            "finish_reason": response.finish_reason,
        });

        Ok(ToolCallContent::text(result.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tool trait impl
// ---------------------------------------------------------------------------

#[async_trait]
impl Tool for LlmTool {
    fn name(&self) -> &str {
        TOOL_LLM
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            TOOL_LLM,
            Some(
                "Direct LLM invocation tool. Supports multiple actions: \
                 - 'invoke' (default): Call an LLM with messages, supports multimodal inputs. \
                 - 'list_providers': List available providers. \
                 - 'list_models': List models with detailed metadata from models.dev. \
                 - 'model_info': Get detailed info for a specific model."
                    .to_string(),
            ),
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["invoke", "list_providers", "list_models", "model_info"],
                        "description": "Action to perform. Default: 'invoke'. \
                                         Use 'list_providers' to see available providers, \
                                         'list_models' to browse models with pricing/capabilities, \
                                         'model_info' for a single model's full details."
                    },
                    "provider": {
                        "type": "string",
                        "description": "Provider name for the action. Defaults to the first configured provider."
                    },
                    "model": {
                        "type": "string",
                        "description": "Model ID. For 'invoke': overrides default model. \
                                         For 'model_info': specifies which model to query."
                    },
                    "messages": {
                        "type": "array",
                        "description": "Chat messages in OpenAI format (action='invoke' only). \
                                         User message content can be a string or an array of content parts.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "role": {
                                    "type": "string",
                                    "enum": ["system", "user", "assistant"]
                                },
                                "content": {
                                    "description": "Message content. \
                                                     String for text-only; array for multimodal.",
                                    "oneOf": [
                                        { "type": "string" },
                                        {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "type": {
                                                        "type": "string",
                                                        "enum": [
                                                            "text",
                                                            "image_url", "input_audio",
                                                            "image_path", "audio_path",
                                                            "video_url", "video_base64", "video_path",
                                                            "pdf_url", "pdf_base64", "pdf_path"
                                                        ]
                                                    }
                                                },
                                                "required": ["type"]
                                            }
                                        }
                                    ]
                                }
                            },
                            "required": ["role", "content"]
                        }
                    },
                    "temperature": {
                        "type": "number",
                        "description": "Sampling temperature 0.0–2.0. Optional."
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Maximum number of tokens to generate in the completion. Optional."
                    },
                    "top_p": {
                        "type": "number",
                        "description": "Nucleus sampling probability (0.0–1.0). Alternative to temperature. Optional."
                    },
                    "response_format": {
                        "type": "object",
                        "description": "Output format specification. Use {\"type\":\"json_object\"} for JSON mode. Optional.",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["text", "json_object"]
                            }
                        },
                        "required": ["type"]
                    },
                    "reasoning_effort": {
                        "type": "string",
                        "enum": ["auto", "none", "minimal", "low", "medium", "high", "xhigh"],
                        "description": "Reasoning effort level (for o1/o3 series models). Optional."
                    },
                    "seed": {
                        "type": "integer",
                        "description": "Random seed for reproducible outputs. Optional."
                    }
                }
            }),
        )
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let action = args
            .get("action")
            .and_then(|a| a.as_str())
            .unwrap_or("invoke");

        match action {
            "list_providers" => self.handle_list_providers(),
            "list_models" => self.handle_list_models(args.get("provider").and_then(|p| p.as_str())),
            "model_info" => self.handle_model_info(
                args.get("provider").and_then(|p| p.as_str()),
                args.get("model").and_then(|m| m.as_str()),
            ),
            "invoke" => self.handle_invoke(args).await,
            other => Err(ToolSourceError::InvalidInput(format!(
                "未知 action: {}",
                other
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Model → JSON helper
// ---------------------------------------------------------------------------

/// Serialize a `model_spec_core::Model` to a flat, agent-friendly JSON object.
fn model_to_json(m: &Model) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "description": m.description,
        "family": m.family,
        "reasoning": m.reasoning,
        "tool_call": m.tool_call,
        "structured_output": m.structured_output,
        "temperature": m.temperature,
        "attachment": m.attachment,
        "open_weights": m.open_weights,
        "modalities": {
            "input": m.modalities.input,
            "output": m.modalities.output,
        },
        "limit": {
            "context": m.limit.context,
            "input": m.limit.input,
            "output": m.limit.output,
        },
        "cost": m.cost.as_ref().map(|c| json!({
            "input": c.input,
            "output": c.output,
            "reasoning": c.reasoning,
            "cache_read": c.cache_read,
            "cache_write": c.cache_write,
            "input_audio": c.input_audio,
            "output_audio": c.output_audio,
        })),
        "knowledge": m.knowledge,
        "release_date": m.release_date,
        "last_updated": m.last_updated,
    })
}
