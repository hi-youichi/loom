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
    #[serde(
        serialize_with = "serialize_status",
        deserialize_with = "deserialize_status"
    )]
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

    pub fn parse_status(s: &str) -> Option<Self> {
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
    fn encode_by_ref(
        &self,
        buf: &mut Vec<SqliteArgumentValue<'_>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        <&str as sqlx::Encode<'_, sqlx::Sqlite>>::encode_by_ref(&self.as_str(), buf)
    }
}

impl sqlx::Decode<'_, sqlx::Sqlite> for TaskStatus {
    fn decode(value: SqliteValueRef<'_>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        TaskStatus::parse_status(&s).ok_or_else(|| format!("invalid status: {}", s).into())
    }
}

fn serialize_status<S: Serializer>(status: &TaskStatus, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(status.as_str())
}

fn deserialize_status<'de, D: Deserializer<'de>>(d: D) -> Result<TaskStatus, D::Error> {
    let s = String::deserialize(d)?;
    TaskStatus::parse_status(&s)
        .ok_or_else(|| serde::de::Error::custom(format!("invalid status: {}", s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TaskStatus::as_str ──

    #[test]
    fn task_status_as_str() {
        assert_eq!(TaskStatus::Pending.as_str(), "pending");
        assert_eq!(TaskStatus::InProgress.as_str(), "in_progress");
        assert_eq!(TaskStatus::Completed.as_str(), "completed");
        assert_eq!(TaskStatus::Cancelled.as_str(), "cancelled");
    }

    // ── TaskStatus::parse_status ──

    #[test]
    fn parse_status_valid() {
        assert_eq!(
            TaskStatus::parse_status("pending"),
            Some(TaskStatus::Pending)
        );
        assert_eq!(
            TaskStatus::parse_status("in_progress"),
            Some(TaskStatus::InProgress)
        );
        assert_eq!(
            TaskStatus::parse_status("completed"),
            Some(TaskStatus::Completed)
        );
        assert_eq!(
            TaskStatus::parse_status("cancelled"),
            Some(TaskStatus::Cancelled)
        );
    }

    #[test]
    fn parse_status_invalid() {
        assert_eq!(TaskStatus::parse_status("unknown"), None);
        assert_eq!(TaskStatus::parse_status(""), None);
        assert_eq!(TaskStatus::parse_status("PENDING"), None);
    }

    // ── TaskStatus::all_values ──

    #[test]
    fn all_values_contains_all() {
        let vals = TaskStatus::all_values();
        assert_eq!(vals, &["pending", "in_progress", "completed", "cancelled"]);
    }

    // ── TaskStatus::Display ──

    #[test]
    fn display() {
        assert_eq!(format!("{}", TaskStatus::Pending), "pending");
        assert_eq!(format!("{}", TaskStatus::InProgress), "in_progress");
    }

    // ── TaskStatus equality ──

    #[test]
    fn equality() {
        assert_eq!(TaskStatus::Pending, TaskStatus::Pending);
        assert_ne!(TaskStatus::Pending, TaskStatus::Completed);
    }

    // ── Serialization ──

    #[test]
    fn serialize_status_via_task() {
        // TaskStatus doesn't derive Serialize directly; test via Task serialization
        let task = Task {
            id: "1".into(),
            name: "n".into(),
            description: String::new(),
            assignee: String::new(),
            start_time: String::new(),
            created_at: String::new(),
            status: TaskStatus::InProgress,
            metadata: String::new(),
        };
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("\"status\":\"in_progress\""));
    }

    #[test]
    fn deserialize_status_via_task() {
        let json = r#"{"id":"1","name":"n","description":"d","assignee":"a","start_time":"s","created_at":"c","status":"completed","metadata":""}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
    }

    #[test]
    fn deserialize_status_invalid_via_task() {
        let json = r#"{"id":"1","name":"n","description":"d","assignee":"a","start_time":"s","created_at":"c","status":"bogus","metadata":""}"#;
        let result = serde_json::from_str::<Task>(json);
        assert!(result.is_err());
    }

    // ── Task struct serialization ──

    #[test]
    fn task_serialization_roundtrip() {
        let task = Task {
            id: "t-1".to_string(),
            name: "test task".to_string(),
            description: "desc".to_string(),
            assignee: "alice".to_string(),
            start_time: "2025-01-01T00:00:00Z".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: TaskStatus::InProgress,
            metadata: "{\"key\":123}".to_string(),
        };
        let json = serde_json::to_string(&task).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "t-1");
        assert_eq!(parsed.name, "test task");
        assert_eq!(parsed.status, TaskStatus::InProgress);
        assert_eq!(parsed.metadata, "{\"key\":123}");
    }

    #[test]
    fn task_default_metadata() {
        let json = r#"{"id":"1","name":"n","description":"d","assignee":"a","start_time":"s","created_at":"c","status":"pending"}"#;
        let task: Task = serde_json::from_str(json).unwrap();
        assert_eq!(task.metadata, "");
    }

    // ── Task::metadata_value ──

    #[test]
    fn metadata_value_valid_json() {
        let task = Task {
            id: "1".into(),
            name: "n".into(),
            description: String::new(),
            assignee: String::new(),
            start_time: String::new(),
            created_at: String::new(),
            status: TaskStatus::Pending,
            metadata: r#"{"x":42}"#.to_string(),
        };
        let val = task.metadata_value();
        assert_eq!(val["x"], 42);
    }

    #[test]
    fn metadata_value_invalid_json_returns_empty_object() {
        let task = Task {
            id: "1".into(),
            name: "n".into(),
            description: String::new(),
            assignee: String::new(),
            start_time: String::new(),
            created_at: String::new(),
            status: TaskStatus::Pending,
            metadata: "not json".to_string(),
        };
        let val = task.metadata_value();
        assert_eq!(val, serde_json::json!({}));
    }

    // ── Task::set_metadata_value ──

    #[test]
    fn set_metadata_value() {
        let mut task = Task {
            id: "1".into(),
            name: "n".into(),
            description: String::new(),
            assignee: String::new(),
            start_time: String::new(),
            created_at: String::new(),
            status: TaskStatus::Pending,
            metadata: "{}".to_string(),
        };
        task.set_metadata_value(&serde_json::json!({"updated": true}));
        assert_eq!(task.metadata, "{\"updated\":true}");
    }
}
