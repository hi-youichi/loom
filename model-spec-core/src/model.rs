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

    #[serde(default)]
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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<ModelLimit>,
}

impl Model {
    pub fn tier(&self) -> ModelTier {
        tier_of(&self.id, self.family.as_deref(), self.cost.as_ref())
    }
}
