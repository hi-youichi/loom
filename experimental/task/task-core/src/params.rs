use crate::models::TaskStatus;

pub fn parse_status(s: &str) -> Result<TaskStatus, String> {
    TaskStatus::parse_status(s).ok_or_else(|| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_valid() {
        assert!(parse_status("pending").is_ok());
        assert_eq!(parse_status("pending").unwrap(), TaskStatus::Pending);
        assert_eq!(parse_status("in_progress").unwrap(), TaskStatus::InProgress);
        assert_eq!(parse_status("completed").unwrap(), TaskStatus::Completed);
        assert_eq!(parse_status("cancelled").unwrap(), TaskStatus::Cancelled);
    }

    #[test]
    fn parse_status_invalid() {
        let err = parse_status("bogus").unwrap_err();
        assert!(err.contains("invalid status 'bogus'"));
        assert!(err.contains("pending"));
    }

    #[test]
    fn list_params_default() {
        let p = ListParams::default();
        assert!(p.status.is_none());
        assert!(p.assignee.is_none());
        assert!(p.name.is_none());
        assert_eq!(p.sort_by, "created_at");
        assert_eq!(p.sort_order, "desc");
        assert_eq!(p.limit, 20);
        assert_eq!(p.page, 1);
    }
}
