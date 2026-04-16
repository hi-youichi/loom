use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::Model;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,

    pub name: String,

    #[serde(default)]
    pub env: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    pub models: HashMap<String, Model>,
}
