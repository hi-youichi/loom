use crate::models::TaskStatus;

pub fn parse_status(s: &str) -> Result<TaskStatus, String> {
    TaskStatus::from_str(s).ok_or_else(|| {
        format!(
            "invalid status '{}'. Valid values: {}",
            s,
            TaskStatus::all_values().join(", ")
        )
    })
}

pub struct CreateParams {
    pub name: String,
    pub description: String,
    pub assignee: String,
    pub start_time: Option<String>,
    pub status: TaskStatus,
}

pub struct ListParams {
    pub status: Option<TaskStatus>,
    pub assignee: Option<String>,
    pub name: Option<String>,
    pub sort_by: String,
    pub sort_order: String,
    pub limit: u32,
    pub page: u32,
}

impl Default for ListParams {
    fn default() -> Self {
        Self {
            status: None,
            assignee: None,
            name: None,
            sort_by: "created_at".to_string(),
            sort_order: "desc".to_string(),
            limit: 20,
            page: 1,
        }
    }
}

pub struct UpdateParams {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub assignee: Option<String>,
    pub start_time: Option<String>,
    pub status: Option<TaskStatus>,
}
