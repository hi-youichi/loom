//! Shared protocol types.

use serde::{Deserialize, Serialize};

/// Agent type for run requests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    React,
    Dup,
    Tot,
    Got,
}

/// Output format for tool_show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolShowOutput {
    Json,
    Yaml,
}

/// Filter for agent list by source type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSourceFilter {
    BuiltIn,
    Project,
    User,
}

/// Agent source type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSource {
    BuiltIn,
    Project,
    User,
}
