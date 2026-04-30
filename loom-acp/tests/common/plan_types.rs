use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct PlanNotification {
    #[serde(rename = "sessionUpdate")]
    pub session_update: String,
    pub entries: Vec<PlanEntry>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct PlanEntry {
    pub content: String,
    pub priority: PlanEntryPriority,
    pub status: PlanEntryStatus,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanEntryPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanEntryStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct SessionUpdateNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: SessionUpdateParams,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct SessionUpdateParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub update: SessionUpdateBody,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct SessionUpdateBody {
    #[serde(rename = "sessionUpdate")]
    pub session_update: String,
    #[serde(default)]
    pub entries: Option<Vec<PlanEntry>>,
    #[serde(default)]
    pub content: Option<String>,
}
