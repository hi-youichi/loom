use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Provider metadata from models.dev
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provider {
    /// Provider ID (e.g., "anthropic", "openai")
    pub id: String,

    /// Provider display name (e.g., "Anthropic", "OpenAI")
    pub name: String,

    /// Environment variable names for API keys
    #[serde(default)]
    pub env: Vec<String>,

    /// NPM package name (e.g., "@ai-sdk/anthropic")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,

    /// Documentation URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,

    /// API base URL from models.dev `api` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    /// Models provided by this provider
    pub models: HashMap<String, Model>,
}

/// Complete model metadata from models.dev
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    /// Model ID (e.g., "claude-3-5-sonnet-20241022")
    pub id: String,

    /// Model display name (e.g., "Claude Sonnet 3.5 v2")
    pub name: String,

    /// Model family (e.g., "claude-sonnet", "gpt")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    /// Supports file attachments
    #[serde(default)]
    pub attachment: bool,

    /// Supports extended reasoning (e.g., o1 models)
    #[serde(default)]
    pub reasoning: bool,

    /// Supports tool/function calling
    #[serde(default)]
    pub tool_call: bool,

    /// Supports temperature parameter
    #[serde(default = "default_true")]
    pub temperature: bool,

    /// Supports structured output (JSON mode)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,

    /// Knowledge cutoff date (e.g., "2024-04-30")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<String>,

    /// Release date (e.g., "2024-10-22")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,

    /// Last updated date (e.g., "2024-10-22")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,

    /// Input and output modalities
    #[serde(default)]
    pub modalities: Modalities,

    /// Model uses open weights (open source)
    #[serde(default)]
    pub open_weights: bool,

    /// Pricing information
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,

    /// Token limits
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<ModelLimit>,
}

fn default_true() -> bool {
    true
}

/// Input and output modalities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Modalities {
    /// Supported input modalities
    #[serde(default)]
    pub input: Vec<ModalityType>,

    /// Supported output modalities
    #[serde(default)]
    pub output: Vec<ModalityType>,
}

/// Modality types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModalityType {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
}

impl Modalities {
    /// Check if supports text input
    pub fn supports_text(&self) -> bool {
        self.input.contains(&ModalityType::Text)
    }

    /// Check if supports image input (vision)
    pub fn supports_vision(&self) -> bool {
        self.input.contains(&ModalityType::Image)
    }

    /// Check if supports audio input
    pub fn supports_audio(&self) -> bool {
        self.input.contains(&ModalityType::Audio)
    }

    /// Check if supports video input
    pub fn supports_video(&self) -> bool {
        self.input.contains(&ModalityType::Video)
    }

    /// Check if supports PDF input
    pub fn supports_pdf(&self) -> bool {
        self.input.contains(&ModalityType::Pdf)
    }
}

/// Pricing information (costs per 1M tokens in USD)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    /// Input cost per 1M tokens (in USD * 100 to avoid floating point)
    #[serde(default)]
    pub input: u32,

    /// Output cost per 1M tokens (in USD * 100)
    #[serde(default)]
    pub output: u32,

    /// Cache read cost per 1M tokens (for prompt caching)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u32>,

    /// Cache write cost per 1M tokens
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u32>,
}

impl Cost {
    /// Create new cost specification
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input: (input * 100.0) as u32,
            output: (output * 100.0) as u32,
            cache_read: None,
            cache_write: None,
        }
    }

    /// Get input cost in USD
    pub fn input_cost_usd(&self) -> f64 {
        self.input as f64 / 100.0
    }

    /// Get output cost in USD
    pub fn output_cost_usd(&self) -> f64 {
        self.output as f64 / 100.0
    }

    /// Estimate cost for given token counts
    pub fn estimate(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = self.input_cost_usd() * (input_tokens as f64 / 1_000_000.0);
        let output_cost = self.output_cost_usd() * (output_tokens as f64 / 1_000_000.0);
        input_cost + output_cost
    }
}

/// Token limits for a model
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLimit {
    /// Context (input) token limit
    pub context: u32,

    /// Output token limit
    pub output: u32,

    /// Cache read token limit (for prompt caching)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u32>,

    /// Cache write token limit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u32>,
}

impl ModelLimit {
    /// Create new model limits
    pub fn new(context: u32, output: u32) -> Self {
        Self {
            context,
            output,
            cache_read: None,
            cache_write: None,
        }
    }

    /// Set cache read limit
    pub fn with_cache_read(mut self, limit: u32) -> Self {
        self.cache_read = Some(limit);
        self
    }

    /// Set cache write limit
    pub fn with_cache_write(mut self, limit: u32) -> Self {
        self.cache_write = Some(limit);
        self
    }
}
