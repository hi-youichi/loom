use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::tier::{tier_of, ModelTier};

use super::cost::Cost;
use super::limit::{Modalities, ModelLimit};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Alpha,
    Beta,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderShape {
    Responses,
    Completions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEffort {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "minimal")]
    Minimal,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "xhigh")]
    Xhigh,
    #[serde(rename = "max")]
    Max,
    #[serde(rename = "default")]
    Default,
}

/// A reasoning option entry, corresponding to models.dev's
/// `ReasoningOption` discriminated union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ReasoningOption {
    #[serde(rename = "toggle")]
    Toggle,

    #[serde(rename = "effort")]
    Effort { values: Vec<Option<ReasoningEffort>> },

    #[serde(rename = "budget_tokens")]
    BudgetTokens {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
    },
}

/// Whether interleaved reasoning is supported.
///
/// In models.dev this is either the literal `true` or an object
/// `{ field: "reasoning_content" | "reasoning_details" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interleaved {
    Simple,

    Field {
        field: InterleavedField,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterleavedField {
    #[serde(rename = "reasoning_content")]
    ReasoningContent,

    #[serde(rename = "reasoning_details")]
    ReasoningDetails,
}

/// Per-mode experimental overrides (cost and/or provider tweaks).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Experimental {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<HashMap<String, ExperimentalMode>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ExperimentalMode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ExperimentalProviderConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExperimentalProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<HashMap<String, JsonValue>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

/// Model-level provider configuration overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<ProviderShape>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<HashMap<String, JsonValue>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,

    pub name: String,

    #[serde(default)]
    pub description: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    #[serde(default)]
    pub attachment: bool,

    #[serde(default)]
    pub reasoning: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_options: Option<Vec<ReasoningOption>>,

    #[serde(default = "default_true")]
    pub tool_call: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interleaved: Option<Interleaved>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<String>,

    #[serde(default)]
    pub release_date: String,

    #[serde(default)]
    pub last_updated: String,

    #[serde(default)]
    pub modalities: Modalities,

    #[serde(default)]
    pub open_weights: bool,

    #[serde(default)]
    pub limit: ModelLimit,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ModelStatus>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Experimental>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ModelProviderConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

impl Model {
    pub fn is_reasoning(&self) -> bool {
        self.reasoning
    }

    pub fn supports_tools(&self) -> bool {
        self.tool_call
    }

    pub fn supports_vision(&self) -> bool {
        self.modalities.supports_vision()
    }

    pub fn supports_audio(&self) -> bool {
        self.modalities.supports_audio()
    }

    pub fn context_window(&self) -> u32 {
        self.limit.context
    }

    pub fn max_output_tokens(&self) -> u32 {
        self.limit.output
    }

    pub fn input_price_per_million(&self) -> Option<f64> {
        self.cost.as_ref().map(|c| c.input)
    }

    pub fn output_price_per_million(&self) -> Option<f64> {
        self.cost.as_ref().map(|c| c.output)
    }

    pub fn tier(&self) -> ModelTier {
        tier_of(&self.id, self.family.as_deref(), self.cost.as_ref())
    }

    pub fn minimal(id: impl Into<String>, limit: ModelLimit) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            limit,
            ..Default::default()
        }
    }
}

impl Default for Model {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            family: None,
            attachment: false,
            reasoning: false,
            reasoning_options: None,
            tool_call: true,
            interleaved: None,
            structured_output: None,
            temperature: None,
            knowledge: None,
            release_date: String::new(),
            last_updated: String::new(),
            modalities: Modalities::default(),
            open_weights: false,
            limit: ModelLimit::default(),
            status: None,
            experimental: None,
            provider: None,
            cost: None,
        }
    }
}
