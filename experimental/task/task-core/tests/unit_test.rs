use task_core::*;

// ── TaskStatus tests ──

#[test]
fn task_status_as_str() {
    assert_eq!(models::TaskStatus::Pending.as_str(), "pending");
    assert_eq!(models::TaskStatus::InProgress.as_str(), "in_progress");
    assert_eq!(models::TaskStatus::Completed.as_str(), "completed");
    assert_eq!(models::TaskStatus::Cancelled.as_str(), "cancelled");
}

#[test]
fn task_status_parse_status() {
    assert_eq!(
        models::TaskStatus::parse_status("pending"),
        Some(models::TaskStatus::Pending)
    );
    assert_eq!(
        models::TaskStatus::parse_status("in_progress"),
        Some(models::TaskStatus::InProgress)
    );
    assert_eq!(
        models::TaskStatus::parse_status("completed"),
        Some(models::TaskStatus::Completed)
    );
    assert_eq!(
        models::TaskStatus::parse_status("cancelled"),
        Some(models::TaskStatus::Cancelled)
    );
    assert_eq!(models::TaskStatus::parse_status("unknown"), None);
    assert_eq!(models::TaskStatus::parse_status(""), None);
}

#[test]
fn task_status_all_values() {
    let values = models::TaskStatus::all_values();
    assert_eq!(
        values,
        &["pending", "in_progress", "completed", "cancelled"]
    );
}

#[test]
fn task_status_display() {
    assert_eq!(format!("{}", models::TaskStatus::Pending), "pending");
    assert_eq!(format!("{}", models::TaskStatus::InProgress), "in_progress");
    assert_eq!(format!("{}", models::TaskStatus::Completed), "completed");
    assert_eq!(format!("{}", models::TaskStatus::Cancelled), "cancelled");
}

#[test]
fn task_status_equality() {
    assert_eq!(models::TaskStatus::Pending, models::TaskStatus::Pending);
    assert_ne!(models::TaskStatus::Pending, models::TaskStatus::Completed);
}

// ── Task model tests ──

#[test]
fn task_metadata_value_empty() {
    let task = models::Task {
        id: "test-id".to_string(),
        name: "Test".to_string(),
        description: String::new(),
        assignee: String::new(),
        start_time: "2025-01-01T00:00:00Z".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        status: models::TaskStatus::Pending,
        metadata: "{}".to_string(),
    };
    let meta = task.metadata_value();
    assert!(meta.is_object());
    assert!(meta.as_object().unwrap().is_empty());
}

#[test]
fn task_metadata_value_with_data() {
    let task = models::Task {
        id: "test-id".to_string(),
        name: "Test".to_string(),
        description: String::new(),
        assignee: String::new(),
        start_time: "2025-01-01T00:00:00Z".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        status: models::TaskStatus::Pending,
        metadata: r#"{"key":"value","num":42}"#.to_string(),
    };
    let meta = task.metadata_value();
    assert_eq!(meta["key"], "value");
    assert_eq!(meta["num"], 42);
}

#[test]
fn task_metadata_value_invalid_json_returns_empty() {
    let task = models::Task {
        id: "test-id".to_string(),
        name: "Test".to_string(),
        description: String::new(),
        assignee: String::new(),
        start_time: "2025-01-01T00:00:00Z".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        status: models::TaskStatus::Pending,
        metadata: "invalid json".to_string(),
    };
    let meta = task.metadata_value();
    assert!(meta.is_object());
    assert!(meta.as_object().unwrap().is_empty());
}

#[test]
fn task_set_metadata_value() {
    let mut task = models::Task {
        id: "test-id".to_string(),
        name: "Test".to_string(),
        description: String::new(),
        assignee: String::new(),
        start_time: "2025-01-01T00:00:00Z".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        status: models::TaskStatus::Pending,
        metadata: "{}".to_string(),
    };
    let val = serde_json::json!({"foo": "bar"});
    task.set_metadata_value(&val);
    assert_eq!(task.metadata, r#"{"foo":"bar"}"#);
}

#[test]
fn task_serde_roundtrip() {
    let task = models::Task {
        id: "id-1".to_string(),
        name: "My Task".to_string(),
        description: "A test task".to_string(),
        assignee: "alice".to_string(),
        start_time: "2025-01-01T00:00:00Z".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        status: models::TaskStatus::InProgress,
        metadata: r#"{"x":1}"#.to_string(),
    };
    let json = serde_json::to_string(&task).unwrap();
    let de: models::Task = serde_json::from_str(&json).unwrap();
    assert_eq!(de.id, "id-1");
    assert_eq!(de.name, "My Task");
    assert_eq!(de.status, models::TaskStatus::InProgress);
    assert_eq!(de.metadata, r#"{"x":1}"#);
}

// ── parse_status function tests ──

#[test]
fn parse_status_valid() {
    assert!(params::parse_status("pending").is_ok());
    assert!(params::parse_status("in_progress").is_ok());
    assert!(params::parse_status("completed").is_ok());
    assert!(params::parse_status("cancelled").is_ok());
}

#[test]
fn parse_status_invalid() {
    let result = params::parse_status("invalid");
    let err = result.unwrap_err();
    assert!(err.contains("invalid status"));
    assert!(err.contains("Valid values:"));
}

// ── ShowError tests ──

#[test]
fn show_error_not_found_display() {
    let err = db::ShowError::NotFound("abc".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("task not found"));
    assert!(msg.contains("abc"));
}

#[test]
fn show_error_ambiguous_display() {
    let err = db::ShowError::Ambiguous {
        prefix: "ab".to_string(),
        matches: vec![
            ("abc12345".to_string(), "Task A".to_string()),
            ("abd67890".to_string(), "Task B".to_string()),
        ],
    };
    let msg = format!("{}", err);
    assert!(msg.contains("ambiguous"));
    assert!(msg.contains("ab"));
    assert!(msg.contains("2 tasks"));
}

#[test]
fn show_error_db_error_display() {
    let err = db::ShowError::DbError("connection failed".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("database error"));
    assert!(msg.contains("connection failed"));
}

#[test]
fn task_db_error_display() {
    let err = db::TaskDbError::Other("custom error".to_string());
    let msg = format!("{}", err);
    assert_eq!(msg, "custom error");
}

// ── ListParams default tests ──

#[test]
fn list_params_default() {
    let lp = params::ListParams::default();
    assert!(lp.status.is_none());
    assert!(lp.assignee.is_none());
    assert!(lp.name.is_none());
    assert_eq!(lp.sort_by, "created_at");
    assert_eq!(lp.sort_order, "desc");
    assert_eq!(lp.limit, 20);
    assert_eq!(lp.page, 1);
}

// ── CreateParams default tests ──

#[test]
fn create_params_default() {
    let cp = params::CreateParams::default();
    assert!(cp.name.is_empty());
    assert!(cp.description.is_empty());
    assert!(cp.assignee.is_empty());
    assert!(cp.start_time.is_none());
    assert_eq!(cp.status, models::TaskStatus::Pending);
}
