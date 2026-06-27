use serde::{Deserialize, Serialize};

use crate::cost::Cost;
use crate::limit::{Modalities, ModelLimit};
use crate::tier::{tier_of, ModelTier};

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,

    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    #[serde(default)]
    pub attachment: bool,

    #[serde(default)]
    pub reasoning: bool,

    #[serde(default = "default_true")]
    pub tool_call: bool,

    #[serde(default = "default_true")]
    pub temperature: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,

    #[serde(default)]
    pub modalities: Modalities,

    #[serde(default)]
    pub open_weights: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,

    #[serde(default)]
    pub limit: ModelLimit,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            family: None,
            attachment: false,
            reasoning: false,
            tool_call: true,
            temperature: true,
            structured_output: None,
            knowledge: None,
            release_date: None,
            last_updated: None,
            modalities: Modalities::default(),
            open_weights: false,
            cost: None,
            limit: ModelLimit::default(),
        }
    }
}

impl Model {
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
