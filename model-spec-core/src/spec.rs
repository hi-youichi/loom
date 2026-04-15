use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Classification tier for LLM models based on cost and capability.
///
/// Tiers are determined automatically by [`Model::tier()`] using a priority chain:
/// model family name → model id keywords → cost thresholds.
///
/// | Tier     | Typical use                        | Examples                              |
/// |----------|------------------------------------|---------------------------------------|
/// | Light    | Fast / cheap tasks, compaction     | haiku, mini, flash, air               |
/// | Standard | General-purpose agent work         | sonnet, gpt-4o, gemini-2.5-pro        |
/// | Heavy    | Complex reasoning, premium quality | opus, ultra, o1-pro, long             |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTier {
    /// Fast, low-cost models suitable for summarization, compaction, and bulk tasks.
    Light,
    /// Balanced models for general-purpose agent work.
    Standard,
    /// High-capability models for complex reasoning or premium-quality output.
    Heavy,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTier::Light => write!(f, "light"),
            ModelTier::Standard => write!(f, "standard"),
            ModelTier::Heavy => write!(f, "heavy"),
        }
    }
}

/// Provider metadata from models.dev
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl Model {
    /// Determine the model's tier based on family name, id keywords, and cost.
    ///
    /// # Priority order
    ///
    /// 1. **Family name suffix** — checked first as the strongest signal.
    /// 2. **Model id keywords** — fallback when family is absent or ambiguous.
    /// 3. **Cost thresholds** — final fallback based on input price ($/M tokens).
    ///
    /// # Family name rules
    ///
    /// | Pattern (case-insensitive suffix) | Tier   |
    /// |-----------------------------------|--------|
    /// | `flash`, `flashx`, `haiku`, `mini`, `air`, `airx` | Light  |
    /// | `opus`, `ultra`                   | Heavy  |
    /// | contains `o1-pro`                 | Heavy  |
    /// | ends with `long`                  | Heavy  |
    ///
    /// # Model id keyword rules
    ///
    /// Checked when family-based classification did not produce a result.
    /// Splits the model id on `-` and scans tokens.
    ///
    /// | Keywords                     | Tier   |
    /// |------------------------------|--------|
    /// | `flash`, `flashx`, `air`, `airx` or last token is `mini` | Light  |
    /// | `long`                       | Heavy  |
    ///
    /// # Cost thresholds (input price in $/M tokens)
    ///
    /// Applied when neither family nor id produced a classification.
    ///
    /// | Input cost         | Tier      |
    /// |--------------------|-----------|
    /// | `< $0.50`          | Light     |
    /// | `> $15.00`         | Heavy     |
    /// | between            | Standard  |
    ///
    /// If cost data is unavailable, defaults to [`ModelTier::Standard`].
    pub fn tier(&self) -> ModelTier {
        if let Some(ref family) = self.family {
            let f = family.to_lowercase();
            if f.ends_with("flash")
                || f.ends_with("flashx")
                || f.ends_with("haiku")
                || f.ends_with("mini")
                || f.ends_with("air")
                || f.ends_with("airx")
            {
                return ModelTier::Light;
            }
            if f.ends_with("opus")
                || f.ends_with("ultra")
                || f.contains("o1-pro")
                || f.ends_with("long")
            {
                return ModelTier::Heavy;
            }
        }

        let id = self.id.to_lowercase();
        let parts: Vec<&str> = id.split('-').collect();
        if parts.iter().any(|p| matches!(*p, "flash" | "flashx" | "air" | "airx"))
            || parts.last() == Some(&"mini")
        {
            return ModelTier::Light;
        }
        if parts.contains(&"long") {
            return ModelTier::Heavy;
        }

        if let Some(ref cost) = self.cost {
            if cost.input < 0.5 {
                return ModelTier::Light;
            }
            if cost.input > 15.0 {
                return ModelTier::Heavy;
            }
        }

        ModelTier::Standard
    }
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Input cost per 1M tokens (in USD)
    #[serde(default)]
    pub input: f64,

    /// Output cost per 1M tokens (in USD)
    #[serde(default)]
    pub output: f64,

    /// Cache read cost per 1M tokens (for prompt caching)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,

    /// Cache write cost per 1M tokens
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,

    /// Reasoning cost (for reasoning models)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<f64>,
}

impl Cost {
    /// Create new cost specification
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_read: None,
            cache_write: None,
            reasoning: None,
        }
    }

    /// Get input cost in USD
    pub fn input_cost_usd(&self) -> f64 {
        self.input
    }

    /// Get output cost in USD
    pub fn output_cost_usd(&self) -> f64 {
        self.output
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, family: Option<&str>, cost: Option<Cost>) -> Model {
        Model {
            id: id.to_string(),
            name: id.to_string(),
            family: family.map(|s| s.to_string()),
            attachment: false,
            reasoning: false,
            tool_call: false,
            temperature: true,
            structured_output: None,
            knowledge: None,
            release_date: None,
            last_updated: None,
            modalities: Modalities::default(),
            open_weights: false,
            cost,
            limit: None,
        }
    }

    #[test]
    fn tier_anthropic_family() {
        let m = model("claude-haiku-3.5", Some("claude-haiku"), None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("claude-sonnet-4", Some("claude-sonnet"), None);
        assert_eq!(m.tier(), ModelTier::Standard);

        let m = model("claude-opus-4", Some("claude-opus"), None);
        assert_eq!(m.tier(), ModelTier::Heavy);
    }

    #[test]
    fn tier_openai_family() {
        let m = model("gpt-4o-mini", Some("gpt-4o-mini"), None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("gpt-4o", Some("gpt-4o"), None);
        assert_eq!(m.tier(), ModelTier::Standard);
    }

    #[test]
    fn tier_google_family() {
        let m = model("gemini-2.5-flash", Some("gemini-2.5-flash"), None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("gemini-2.5-pro", Some("gemini-2.5-pro"), None);
        assert_eq!(m.tier(), ModelTier::Standard);
    }

    #[test]
    fn tier_glm_flash() {
        let m = model("glm-4-flash", None, None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("glm-4-flashx", None, None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("glm-z1-flash", None, None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("glm-z1-flashx", None, None);
        assert_eq!(m.tier(), ModelTier::Light);
    }

    #[test]
    fn tier_glm_air() {
        let m = model("glm-4-air", None, None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("glm-4-airx", None, None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("glm-z1-air", None, None);
        assert_eq!(m.tier(), ModelTier::Light);

        let m = model("glm-z1-airx", None, None);
        assert_eq!(m.tier(), ModelTier::Light);
    }

    #[test]
    fn tier_glm_standard() {
        let m = model("glm-4-plus", None, None);
        assert_eq!(m.tier(), ModelTier::Standard);

        let m = model("glm-4.6", None, None);
        assert_eq!(m.tier(), ModelTier::Standard);

        let m = model("glm-5", None, None);
        assert_eq!(m.tier(), ModelTier::Standard);
    }

    #[test]
    fn tier_glm_long() {
        let m = model("glm-4-long", None, None);
        assert_eq!(m.tier(), ModelTier::Heavy);
    }

    #[test]
    fn tier_deepseek() {
        let m = model("deepseek-chat", Some("deepseek-chat"), None);
        assert_eq!(m.tier(), ModelTier::Standard);

        let m = model("deepseek-reasoner", Some("deepseek-reasoner"), None);
        assert_eq!(m.tier(), ModelTier::Standard);
    }

    #[test]
    fn tier_cost_fallback() {
        let cheap = model("unknown-model", None, Some(Cost::new(0.1, 0.1)));
        assert_eq!(cheap.tier(), ModelTier::Light);

        let mid = model("unknown-model", None, Some(Cost::new(3.0, 15.0)));
        assert_eq!(mid.tier(), ModelTier::Standard);

        let expensive = model("unknown-model", None, Some(Cost::new(30.0, 120.0)));
        assert_eq!(expensive.tier(), ModelTier::Heavy);
    }

    #[test]
    fn tier_family_suffix_priority_over_cost() {
        let m = model("some-flash-model", Some("some-flash"), Some(Cost::new(30.0, 120.0)));
        assert_eq!(m.tier(), ModelTier::Light);
    }

    #[test]
    fn tier_default_is_standard() {
        let m = model("random-model", None, None);
        assert_eq!(m.tier(), ModelTier::Standard);
    }

    #[test]
    fn tier_serde_roundtrip() {
        let tier = ModelTier::Light;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"light\"");
        let de: ModelTier = serde_json::from_str(&json).unwrap();
        assert_eq!(de, ModelTier::Light);
    }
}
