use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use task_core::{CreateParams, ListParams, TaskDb, TaskStatus, UpdateParams, parse_status};

#[derive(Parser)]
#[command(name = "task-mcp-server")]
struct Args {
    #[arg(long)]
    db_path: PathBuf,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TaskCreateArgs {
    #[schemars(description = "Task name")]
    name: String,
    #[schemars(description = "Task description")]
    description: Option<String>,
    #[schemars(description = "Assignee")]
    assignee: Option<String>,
    #[schemars(description = "Start time (ISO 8601)")]
    start_time: Option<String>,
    #[schemars(description = "Task status")]
    status: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TaskShowArgs {
    #[schemars(description = "Task ID or prefix")]
    id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TaskListArgs {
    #[schemars(description = "Filter by status")]
    status: Option<String>,
    #[schemars(description = "Filter by assignee")]
    assignee: Option<String>,
    #[schemars(description = "Filter by name (substring)")]
    name: Option<String>,
    #[schemars(description = "Sort field")]
    sort_by: Option<String>,
    #[schemars(description = "Sort order")]
    sort_order: Option<String>,
    #[schemars(description = "Page size")]
    limit: Option<u32>,
    #[schemars(description = "Page number")]
    page: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TaskUpdateArgs {
    #[schemars(description = "Task ID or prefix")]
    id: String,
    #[schemars(description = "New name")]
    name: Option<String>,
    #[schemars(description = "New description")]
    description: Option<String>,
    #[schemars(description = "New assignee")]
    assignee: Option<String>,
    #[schemars(description = "New start time")]
    start_time: Option<String>,
    #[schemars(description = "New status")]
    status: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TaskDeleteArgs {
    #[schemars(description = "Task ID or prefix")]
    id: String,
}

fn internal_error(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

#[derive(Clone)]
struct TaskServer {
    db: Arc<TaskDb>,
    tool_router: ToolRouter<TaskServer>,
}

#[tool_router]
impl TaskServer {
    fn new(db: Arc<TaskDb>) -> Self {
        Self {
            db,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Create a new task")]
    async fn task_create(
        &self,
        Parameters(args): Parameters<TaskCreateArgs>,
    ) -> Result<String, McpError> {
        let status = match args.status.as_deref() {
            Some(s) => parse_status(s).map_err(internal_error)?,
            None => TaskStatus::Pending,
        };
        let params = CreateParams {
            name: args.name,
            description: args.description.unwrap_or_default(),
            assignee: args.assignee.unwrap_or_default(),
            start_time: args.start_time,
            status,
        };
        let task = self.db.create_task(&params).await.map_err(internal_error)?;
        serde_json::to_string_pretty(&task).map_err(internal_error)
    }

    #[tool(description = "Show a task by ID")]
    async fn task_show(
        &self,
        Parameters(args): Parameters<TaskShowArgs>,
    ) -> Result<String, McpError> {
        let task = self.db.show_task(&args.id).await.map_err(internal_error)?;
        serde_json::to_string_pretty(&task).map_err(internal_error)
    }

    #[tool(description = "List tasks with filters")]
    async fn task_list(
        &self,
        Parameters(args): Parameters<TaskListArgs>,
    ) -> Result<String, McpError> {
        let status = match args.status.as_deref() {
            Some(s) => Some(parse_status(s).map_err(internal_error)?),
            None => None,
        };
        let params = ListParams {
            status,
            assignee: args.assignee,
            name: args.name,
            sort_by: args.sort_by.unwrap_or_else(|| "created_at".to_string()),
            sort_order: args.sort_order.unwrap_or_else(|| "desc".to_string()),
            limit: args.limit.unwrap_or(20),
            page: args.page.unwrap_or(1),
        };
        let list = self.db.list_tasks(&params).await.map_err(internal_error)?;
        serde_json::to_string_pretty(&list).map_err(internal_error)
    }

    #[tool(description = "Update an existing task")]
    async fn task_update(
        &self,
        Parameters(args): Parameters<TaskUpdateArgs>,
    ) -> Result<String, McpError> {
        let status = match args.status.as_deref() {
            Some(s) => Some(parse_status(s).map_err(internal_error)?),
            None => None,
        };
        let params = UpdateParams {
            id: args.id,
            name: args.name,
            description: args.description,
            assignee: args.assignee,
            start_time: args.start_time,
            status,
        };
        let task = self.db.update_task(&params).await.map_err(internal_error)?;
        serde_json::to_string_pretty(&task).map_err(internal_error)
    }

    #[tool(description = "Delete a task by ID")]
    async fn task_delete(
        &self,
        Parameters(args): Parameters<TaskDeleteArgs>,
    ) -> Result<String, McpError> {
        let deleted = self
            .db
            .delete_task(&args.id)
            .await
            .map_err(internal_error)?;
        serde_json::to_string_pretty(&serde_json::json!({
            "id": deleted.id,
            "name": deleted.name,
            "deleted": true,
        }))
        .map_err(internal_error)
    }
}

#[tool_handler]
impl ServerHandler for TaskServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation::from_build_env(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let db = TaskDb::open(&args.db_path)
        .await
        .expect("failed to open tasks.db");
    let server = TaskServer::new(Arc::new(db));
    server.serve(stdio()).await.expect("task-mcp-server error");
}
