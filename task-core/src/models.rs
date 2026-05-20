use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub assignee: String,
    pub start_time: String,
    pub created_at: String,
    #[serde(serialize_with = "serialize_status", deserialize_with = "deserialize_status")]
    pub status: TaskStatus,
    #[serde(default)]
    pub metadata: String,
}

impl Task {
    pub fn metadata_value(&self) -> serde_json::Value {
        serde_json::from_str(&self.metadata).unwrap_or_else(|_| serde_json::json!({}))
    }

    pub fn set_metadata_value(&mut self, val: &serde_json::Value) {
        self.metadata = val.to_string();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            "cancelled" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }

    pub fn all_values() -> &'static [&'static str] {
        &["pending", "in_progress", "completed", "cancelled"]
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

use sqlx::sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef};

impl sqlx::Type<sqlx::Sqlite> for TaskStatus {
    fn type_info() -> SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
}

impl sqlx::Encode<'_, sqlx::Sqlite> for TaskStatus {
    fn encode_by_ref(&self, buf: &mut Vec<SqliteArgumentValue<'_>>) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <&str as sqlx::Encode<'_, sqlx::Sqlite>>::encode_by_ref(&self.as_str(), buf)
    }
}

impl sqlx::Decode<'_, sqlx::Sqlite> for TaskStatus {
    fn decode(value: SqliteValueRef<'_>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        TaskStatus::from_str(&s)
            .ok_or_else(|| format!("invalid status: {}", s).into())
    }
}

fn serialize_status<S: Serializer>(status: &TaskStatus, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(status.as_str())
}

fn deserialize_status<'de, D: Deserializer<'de>>(d: D) -> Result<TaskStatus, D::Error> {
    let s = String::deserialize(d)?;
    TaskStatus::from_str(&s)
        .ok_or_else(|| serde::de::Error::custom(format!("invalid status: {}", s)))
}
