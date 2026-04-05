//! Model specification for runtime and compatibility wrappers.

pub use model_spec_core::spec::{Cost, Modalities, ModalityType, Model, ModelLimit, Provider};
use serde::{Deserialize, Serialize};

/// Legacy ModelSpec for backward compatibility
/// Wraps ModelLimit to maintain existing API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Context (input) token limit
    pub context_limit: u32,

    /// Output token limit
    pub output_limit: u32,

    /// Optional cache read token limit
    #[serde(default)]
    pub cache_read: Option<u32>,

    /// Optional cache write token limit
    #[serde(default)]
    pub cache_write: Option<u32>,

    /// Full model metadata (optional, for extended information)
    #[serde(skip)]
    pub full_model: Option<Model>,
}

impl ModelSpec {
    /// Create a new ModelSpec with required limits
    pub fn new(context_limit: u32, output_limit: u32) -> Self {
        Self {
            context_limit,
            output_limit,
            cache_read: None,
            cache_write: None,
            full_model: None,
        }
    }

    /// Create from ModelLimit
    pub fn from_limit(limit: &ModelLimit) -> Self {
        Self {
            context_limit: limit.context,
            output_limit: limit.output,
            cache_read: limit.cache_read,
            cache_write: limit.cache_write,
            full_model: None,
        }
    }

    /// Create from full Model
    pub fn from_model(model: &Model) -> Option<Self> {
        let limit = model.limit.as_ref()?;
        Some(Self {
            context_limit: limit.context,
            output_limit: limit.output,
            cache_read: limit.cache_read,
            cache_write: limit.cache_write,
            full_model: Some(model.clone()),
        })
    }

    /// Set optional cache read limit
    pub fn with_cache_read(mut self, limit: u32) -> Self {
        self.cache_read = Some(limit);
        self
    }

    /// Set optional cache write limit
    pub fn with_cache_write(mut self, limit: u32) -> Self {
        self.cache_write = Some(limit);
        self
    }

    /// Get modalities if available
    pub fn modalities(&self) -> Option<&Modalities> {
        self.full_model.as_ref().map(|m| &m.modalities)
    }

    /// Get cost if available
    pub fn cost(&self) -> Option<&Cost> {
        self.full_model.as_ref().and_then(|m| m.cost.as_ref())
    }

    /// Check if supports vision (image input)
    pub fn supports_vision(&self) -> bool {
        self.full_model
            .as_ref()
            .map(|m| m.modalities.supports_vision())
            .unwrap_or(false)
    }

    /// Check if supports audio input
    pub fn supports_audio(&self) -> bool {
        self.full_model
            .as_ref()
            .map(|m| m.modalities.supports_audio())
            .unwrap_or(false)
    }

    /// Check if supports video input
    pub fn supports_video(&self) -> bool {
        self.full_model
            .as_ref()
            .map(|m| m.modalities.supports_video())
            .unwrap_or(false)
    }

    /// Check if supports PDF input
    pub fn supports_pdf(&self) -> bool {
        self.full_model
            .as_ref()
            .map(|m| m.modalities.supports_pdf())
            .unwrap_or(false)
    }

    /// Check if supports tool calling
    pub fn supports_tool_call(&self) -> bool {
        self.full_model
            .as_ref()
            .map(|m| m.tool_call)
            .unwrap_or(true) // Default to true for backward compatibility
    }

    /// Estimate cost for given token counts
    pub fn estimate_cost(&self, input_tokens: u32, output_tokens: u32) -> Option<f64> {
        self.cost()
            .map(|cost| cost.estimate(input_tokens, output_tokens))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_spec_new_sets_required_limits_without_cache_fields() {
        let spec = ModelSpec::new(2048, 512);
        assert_eq!(spec.context_limit, 2048);
        assert_eq!(spec.output_limit, 512);
        assert_eq!(spec.cache_read, None);
        assert_eq!(spec.cache_write, None);
    }

    #[test]
    fn model_spec_cache_builder_methods_set_optional_limits() {
        let spec = ModelSpec::new(4096, 1024)
            .with_cache_read(128)
            .with_cache_write(64);
        assert_eq!(spec.cache_read, Some(128));
        assert_eq!(spec.cache_write, Some(64));
    }

    #[test]
    fn modalities_checks() {
        let modalities = Modalities {
            input: vec![ModalityType::Text, ModalityType::Image, ModalityType::Pdf],
            output: vec![ModalityType::Text],
        };

        assert!(modalities.supports_text());
        assert!(modalities.supports_vision());
        assert!(modalities.supports_pdf());
        assert!(!modalities.supports_audio());
        assert!(!modalities.supports_video());
    }

    #[test]
    fn cost_estimation() {
        let cost = Cost::new(3.0, 15.0);

        assert_eq!(cost.input_cost_usd(), 3.0);
        assert_eq!(cost.output_cost_usd(), 15.0);

        let estimated = cost.estimate(1_000_000, 1_000_000);
        assert!((estimated - 18.0).abs() < 0.01);
    }

    #[test]
    fn model_limit_builder_methods() {
        let limit = ModelLimit::new(128_000, 16_384)
            .with_cache_read(128_000)
            .with_cache_write(64_000);

        assert_eq!(limit.context, 128_000);
        assert_eq!(limit.output, 16_384);
        assert_eq!(limit.cache_read, Some(128_000));
        assert_eq!(limit.cache_write, Some(64_000));
    }

    #[test]
    fn model_spec_from_model() {
        let model = Model {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            family: Some("test".to_string()),
            attachment: true,
            reasoning: false,
            tool_call: true,
            temperature: true,
            structured_output: Some(true),
            knowledge: Some("2024-01-01".to_string()),
            release_date: Some("2024-01-01".to_string()),
            last_updated: Some("2024-01-01".to_string()),
            modalities: Modalities {
                input: vec![ModalityType::Text, ModalityType::Image],
                output: vec![ModalityType::Text],
            },
            open_weights: false,
            cost: Some(Cost::new(2.5, 10.0)),
            limit: Some(ModelLimit::new(128_000, 16_384)),
        };

        let spec = ModelSpec::from_model(&model).unwrap();

        assert_eq!(spec.context_limit, 128_000);
        assert_eq!(spec.output_limit, 16_384);
        assert!(spec.supports_vision());
        assert!(!spec.supports_audio());
        assert!(spec.supports_tool_call());

        let estimated_cost = spec.estimate_cost(100_000, 10_000).unwrap();
        assert!((estimated_cost - 0.35).abs() < 0.01); // 0.25 + 0.10
    }
}
