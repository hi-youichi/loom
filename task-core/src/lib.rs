pub mod db;
pub mod models;
pub mod params;

pub use db::{ShowError, TaskDb, TaskDbError, TaskList};
pub use models::{Task, TaskStatus};
pub use params::{parse_status, CreateParams, ListParams, UpdateParams};

impl Default for CreateParams {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            assignee: String::new(),
            start_time: None,
            status: TaskStatus::Pending,
        }
    }
}
