//! Progress reporting for long-running extension operations.
//!
//! Sends `anureo_progress` session/update notifications per §3.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct ProgressUpdate {
    pub operation_id: String,
    pub domain: String,
    pub method: String,
    pub status: ProgressStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStatus {
    Started,
    InProgress,
    Completed,
    Failed,
}

impl ProgressUpdate {
    pub fn started(operation_id: &str, domain: &str, method: &str) -> Self {
        Self {
            operation_id: operation_id.into(),
            domain: domain.into(),
            method: method.into(),
            status: ProgressStatus::Started,
            message: None,
            percent: None,
        }
    }

    pub fn in_progress(operation_id: &str, domain: &str, method: &str, percent: u8) -> Self {
        Self {
            operation_id: operation_id.into(),
            domain: domain.into(),
            method: method.into(),
            status: ProgressStatus::InProgress,
            message: None,
            percent: Some(percent),
        }
    }

    pub fn completed(operation_id: &str, domain: &str, method: &str) -> Self {
        Self {
            operation_id: operation_id.into(),
            domain: domain.into(),
            method: method.into(),
            status: ProgressStatus::Completed,
            message: None,
            percent: Some(100),
        }
    }

    pub fn failed(operation_id: &str, domain: &str, method: &str, msg: &str) -> Self {
        Self {
            operation_id: operation_id.into(),
            domain: domain.into(),
            method: method.into(),
            status: ProgressStatus::Failed,
            message: Some(msg.into()),
            percent: None,
        }
    }

    pub fn to_session_update(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}
