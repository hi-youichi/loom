use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use mcp_core::stdio::{deserialize_message, serialize_message, JsonRpcMessage};
use mcp_core::types::{
    BaseMetadata, CallToolResult, ContentBlock, Icons, Implementation, TextContent, Tool,
};
use mcp_server::server::handlers::ToolHandler;
use mcp_server::{McpServer, ServerOptions};
use task_core::{parse_status, CreateParams, ListParams, TaskDb, UpdateParams};

#[derive(Parser)]
#[command(name = "task-mcp-server")]
struct Args {
    #[arg(long)]
    db_path: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let db = TaskDb::open(&args.db_path).await.expect("failed to open tasks.db");
    let db = Arc::new(db);

    let mut server = McpServer::new(
        Implementation {
            base: BaseMetadata {
                name: "task".into(),
                title: Some("Task Management".into()),
            },
            icons: Icons { icons: None },
            version: "0.1.0".into(),
            website_url: None,
            description: None,
        },
        ServerOptions::default(),
    );

    server
        .register_tool(
            make_tool("task_create", "Create a new task", CREATE_SCHEMA),
            Handler::Create(db.clone()),
        )
        .unwrap();
    server
        .register_tool(
            make_tool("task_show", "Show a task by ID", SHOW_SCHEMA),
            Handler::Show(db.clone()),
        )
        .unwrap();
    server
        .register_tool(
            make_tool("task_list", "List tasks with filters", LIST_SCHEMA),
            Handler::List(db.clone()),
        )
        .unwrap();
    server
        .register_tool(
            make_tool("task_update", "Update an existing task", UPDATE_SCHEMA),
            Handler::Update(db.clone()),
        )
        .unwrap();
    server
        .register_tool(
            make_tool("task_delete", "Delete a task by ID", DELETE_SCHEMA),
            Handler::Delete(db),
        )
        .unwrap();

    let server = Arc::new(std::sync::Mutex::new(server));
    run_stdio(server).await;
}

async fn run_stdio(server: Arc<std::sync::Mutex<McpServer>>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("stdin read error: {}", e);
                break;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg = match deserialize_message(trimmed) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("deserialize error: {}", e);
                continue;
            }
        };

        match msg {
            JsonRpcMessage::Request(req) => {
                let srv = server.lock().unwrap();
                match srv.server().handle_request(req, None).await {
                    Ok(result) => {
                        let out = serialize_message(&JsonRpcMessage::Result(result));
                        match out {
                            Ok(s) => {
                                let _ = writer.write_all(s.as_bytes()).await;
                                let _ = writer.flush().await;
                            }
                            Err(e) => eprintln!("serialize error: {}", e),
                        }
                    }
                    Err(e) => eprintln!("handle_request error: {}", e),
                }
            }
            JsonRpcMessage::Notification(notif) => {
                let srv = server.lock().unwrap();
                let _ = srv.server().handle_notification(notif, None).await;
            }
            JsonRpcMessage::Result(_) => {}
        }
    }
}

fn make_tool(name: &str, description: &str, schema_str: &str) -> Tool {
    Tool {
        base: BaseMetadata {
            name: name.to_string(),
            title: None,
        },
        icons: Icons { icons: None },
        description: Some(description.to_string()),
        input_schema: serde_json::from_str(schema_str).expect("invalid tool schema"),
        output_schema: None,
        annotations: None,
        execution: None,
        meta: None,
    }
}

const CREATE_SCHEMA: &str = r#"{"type":"object","properties":{"name":{"type":"string","description":"Task name"},"description":{"type":"string","description":"Task description"},"assignee":{"type":"string","description":"Assignee"},"start_time":{"type":"string","description":"Start time (ISO 8601)"},"status":{"type":"string","enum":["pending","in_progress","completed","cancelled"],"description":"Task status"}},"required":["name"]}"#;
const SHOW_SCHEMA: &str = r#"{"type":"object","properties":{"id":{"type":"string","description":"Task ID or prefix"}},"required":["id"]}"#;
const LIST_SCHEMA: &str = r#"{"type":"object","properties":{"status":{"type":"string","enum":["pending","in_progress","completed","cancelled"],"description":"Filter by status"},"assignee":{"type":"string","description":"Filter by assignee"},"name":{"type":"string","description":"Filter by name (substring)"},"sort_by":{"type":"string","enum":["created_at","start_time","name","status"],"description":"Sort field"},"sort_order":{"type":"string","enum":["asc","desc"],"description":"Sort order"},"limit":{"type":"integer","description":"Page size"},"page":{"type":"integer","description":"Page number"}}}"#;
const UPDATE_SCHEMA: &str = r#"{"type":"object","properties":{"id":{"type":"string","description":"Task ID or prefix"},"name":{"type":"string","description":"New name"},"description":{"type":"string","description":"New description"},"assignee":{"type":"string","description":"New assignee"},"start_time":{"type":"string","description":"New start time"},"status":{"type":"string","enum":["pending","in_progress","completed","cancelled"],"description":"New status"}},"required":["id"]}"#;
const DELETE_SCHEMA: &str = r#"{"type":"object","properties":{"id":{"type":"string","description":"Task ID or prefix"}},"required":["id"]}"#;

enum Handler {
    Create(Arc<TaskDb>),
    Show(Arc<TaskDb>),
    List(Arc<TaskDb>),
    Update(Arc<TaskDb>),
    Delete(Arc<TaskDb>),
}

#[async_trait::async_trait]
impl ToolHandler for Handler {
    async fn call(
        &self,
        arguments: Option<serde_json::Value>,
        _context: mcp_core::protocol::RequestContext,
    ) -> Result<CallToolResult, mcp_server::ServerError> {
        let args = arguments.unwrap_or_default();
        match self {
            Handler::Create(db) => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let status_str = args
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending");
                let status =
                    parse_status(status_str).map_err(|e| mcp_server::ServerError::Handler(e))?;
                let params = CreateParams {
                    name,
                    description: args
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    assignee: args
                        .get("assignee")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    start_time: args
                        .get("start_time")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    status,
                };
                let task = db.create_task(&params).await
                    .map_err(|e| mcp_server::ServerError::Handler(e.to_string()))?;
                ok_text(&task)
            }
            Handler::Show(db) => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let task = db.show_task(id).await
                    .map_err(|e| mcp_server::ServerError::Handler(e.to_string()))?;
                ok_text(&task)
            }
            Handler::List(db) => {
                let status = args
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(parse_status)
                    .transpose()
                    .map_err(|e: String| mcp_server::ServerError::Handler(e))?;
                let params = ListParams {
                    status,
                    assignee: args
                        .get("assignee")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    name: args
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    sort_by: args
                        .get("sort_by")
                        .and_then(|v| v.as_str())
                        .unwrap_or("created_at")
                        .to_string(),
                    sort_order: args
                        .get("sort_order")
                        .and_then(|v| v.as_str())
                        .unwrap_or("desc")
                        .to_string(),
                    limit: args
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(20) as u32,
                    page: args
                        .get("page")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1) as u32,
                };
                let list = db.list_tasks(&params).await
                    .map_err(|e| mcp_server::ServerError::Handler(e.to_string()))?;
                ok_text(&list)
            }
            Handler::Update(db) => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let status = args
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(parse_status)
                    .transpose()
                    .map_err(|e: String| mcp_server::ServerError::Handler(e))?;
                let params = UpdateParams {
                    id,
                    name: args
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    description: args
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    assignee: args
                        .get("assignee")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    start_time: args
                        .get("start_time")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    status,
                };
                let task = db.update_task(&params).await
                    .map_err(|e| mcp_server::ServerError::Handler(e.to_string()))?;
                ok_text(&task)
            }
            Handler::Delete(db) => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let deleted = db.delete_task(id).await
                    .map_err(|e| mcp_server::ServerError::Handler(e.to_string()))?;
                ok_text(&serde_json::json!({
                    "id": deleted.id,
                    "name": deleted.name,
                    "deleted": true,
                }))
            }
        }
    }
}

fn ok_text(
    data: &impl serde::Serialize,
) -> Result<CallToolResult, mcp_server::ServerError> {
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| mcp_server::ServerError::Handler(e.to_string()))?;
    Ok(CallToolResult {
        content: vec![ContentBlock::Text(TextContent {
            kind: "text".to_string(),
            text: json,
            annotations: None,
            meta: None,
        })],
        ..Default::default()
    })
}
