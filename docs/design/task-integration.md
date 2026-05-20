---
sidebar_position: 7
title: "Task 集成方案"
description: "为 Task 模块增加 Loom Native Tool 和 MCP Server 双模式集成"
---

# Task 集成方案

将 Task 管理模块集成到 Loom 智能体生态，同时支持 **Native Tool**（方案 A）和 **MCP Server**（方案 B）两种接入方式，实现零业务逻辑重复。

## 设计目标

- **双模式接入**：Loom 智能体通过 Native Tool 直接调用，外部 MCP 客户端通过 MCP 协议调用
- **单一核心**：业务逻辑集中在 `task-core` crate，两个方案共享，零重复
- **自动注册**：当 `working_folder` 存在时，Loom 自动注册 Task tools
- **标准协议**：MCP Server 遵循 MCP 协议规范，任何 MCP 客户端均可接入

## 架构概览

```
graphweave/
├── task-core/          # lib: Task, TaskStatus, TaskDb (Mutex<Connection>)
├── task-cli/           # bin: CLI 入口，依赖 task-core
├── task-mcp-server/    # bin: MCP Server (stdio)，依赖 task-core + mcp_server
├── loom/               # 方案 A 的 native tools 在 loom/src/tools/task/
└── Cargo.toml          # workspace members 加入上面三个
```

```
                    ┌──────────────────────┐
                    │     task-core (lib)    │
                    │  Task TaskStatus TaskDb │
                    └─────┬──────────┬──────┘
                          │          │
                   ┌──────▼───┐  ┌───▼────────────┐
                   │ task-cli  │  │ task-mcp-server │
                   │ (binary)  │  │  (binary,stdio) │
                   └──────────┘  └───┬─────────────┘
                                     │ MCP protocol
              ┌──────────────────────┘
              │
   ┌──────────▼──────────────────────┐
   │  方案 A: Loom Native Tools      │
   │  loom/src/tools/task/*.rs       │
   │  直接依赖 task-core             │
   └─────────────────────────────────┘
```

## Crate 拆分

### 现状

当前 `task/` 是独立的 CLI crate，包含 `args.rs`、`db.rs`、`models.rs`、`main.rs`，业务逻辑和 CLI 参数绑定耦合（`CreateArgs` 使用 clap derive）。

### 拆分方案

| Crate | 类型 | 职责 | 依赖 |
|-------|------|------|------|
| `task-core` | lib | `Task`、`TaskStatus`、`TaskDb`、`CreateParams`、`ListParams`、`UpdateParams` | rusqlite, uuid, chrono, serde |
| `task-cli` | bin | CLI 入口，clap 参数解析，委托 task-core | task-core, clap |
| `task-mcp-server` | bin | MCP Server，stdio JSON-RPC，委托 task-core | task-core, mcp_server, mcp_core, tokio |

### task-core 关键设计

```rust
// task-core/src/db.rs
pub struct TaskDb {
    conn: Mutex<Connection>,  // Mutex 包装，支持 Arc<TaskDb> 在 async 上下文共享
}

impl TaskDb {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error>;
    pub fn create_task(&self, params: &CreateParams) -> Result<Task, Box<dyn Error>>;
    pub fn show_task(&self, id_prefix: &str) -> Result<Task, ShowError>;
    pub fn list_tasks(&self, params: &ListParams) -> Result<TaskList, Box<dyn Error>>;
    pub fn update_task(&self, params: &UpdateParams) -> Result<Task, Box<dyn Error>>;
    pub fn delete_task(&self, id_prefix: &str) -> Result<Task, ShowError>;
}

// task-core/src/models.rs — 与现有完全一致
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub assignee: String,
    pub start_time: String,
    pub created_at: String,
    pub status: TaskStatus,
}

// task-core/src/params.rs — 纯数据 struct，无 clap 依赖
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

pub struct UpdateParams {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub assignee: Option<String>,
    pub start_time: Option<String>,
    pub status: Option<TaskStatus>,
}
```

### workspace Cargo.toml 变更

```toml
[workspace]
members = [
    # ... 现有 members ...
    "task-core",
    "task-cli",
    "task-mcp-server",
]
```

## 方案 A：Loom Native Tools

### 文件结构

```
loom/src/tools/task/
├── mod.rs           # pub use + register_task_tools()
├── create.rs        # TaskCreateTool
├── show.rs          # TaskShowTool
├── list.rs          # TaskListTool
├── update.rs        # TaskUpdateTool
├── delete.rs        # TaskDeleteTool
```

### 注册入口

```rust
// loom/src/tools/task/mod.rs
pub async fn register_task_tools(
    aggregate: &AggregateToolSource,
    db: Arc<TaskDb>,
) {
    aggregate.register_async(Box::new(TaskCreateTool::new(db.clone()))).await;
    aggregate.register_async(Box::new(TaskShowTool::new(db.clone()))).await;
    aggregate.register_async(Box::new(TaskListTool::new(db.clone()))).await;
    aggregate.register_async(Box::new(TaskUpdateTool::new(db.clone()))).await;
    aggregate.register_async(Box::new(TaskDeleteTool::new(db))).await;
}
```

### Tool 实现示例

```rust
// loom/src/tools/task/create.rs
pub struct TaskCreateTool {
    db: Arc<TaskDb>,
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str { "task_create" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_create".into(),
            description: Some("创建一个新任务".into()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name":        { "type": "string", "description": "任务名称" },
                    "description": { "type": "string", "description": "任务描述" },
                    "assignee":    { "type": "string", "description": "负责人" },
                    "start_time":  { "type": "string", "description": "开始时间 (ISO 8601)" },
                    "status":      { "type": "string", "enum": ["pending","in_progress","completed","cancelled"] }
                },
                "required": ["name"]
            }),
            output_hint: None,
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let params = parse_create_params(args)?;
        let task = self.db.create_task(&params)
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        let json = serde_json::to_string(&task)
            .map_err(|e| ToolSourceError::ToolError(e.to_string()))?;
        Ok(ToolCallContent::text(json))
    }
}
```

### 自动注册

在 `build_tool_source` 中，当 `working_folder` 存在时自动注册：

```rust
// loom/src/agent/react/build/tool_source.rs
if let Some(ref wf) = config.working_folder {
    let db_path = wf.join("tasks.db");
    if let Ok(db) = TaskDb::open(&db_path) {
        let db = Arc::new(db);
        crate::tools::task::register_task_tools(&aggregate, db).await;
    }
}
```

无需新增配置字段，`working_folder` 存在即自动注册 Task tools。

### 暴露的 Tools

| Tool 名称 | 描述 | 必需参数 |
|-----------|------|---------|
| `task_create` | 创建新任务 | `name` |
| `task_show` | 按 ID 查看任务 | `id` |
| `task_list` | 列出任务（支持过滤/分页） | 无 |
| `task_update` | 更新任务 | `id` + 至少一个可选字段 |
| `task_delete` | 删除任务 | `id` |

## 方案 B：MCP Server

### 技术选型

使用 `mcp_server` crate（来自 `mcm-rust` 仓库），核心 API：

- `McpServer::new(server_info, options)` — 创建服务器实例
- `McpServer::register_tool(tool, handler)` — 注册工具和处理器
- `ToolHandler` trait — `async fn call(arguments, context) -> Result<CallToolResult, ServerError>`
- `mcp_core::stdio` — JSON-RPC 消息的序列化/反序列化（`ReadBuffer`、`deserialize_message`、`serialize_message`）

### stdio 传输

`mcm-rust` 没有现成的 server-side stdio transport（只有 client-side 和 HTTP handler），需要自实现一个轻量的 stdio 事件循环：

```rust
// task-mcp-server/src/stdio_transport.rs
pub async fn run_stdio(server: Arc<Server>) -> Result<(), Box<dyn Error>> {
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let msg = deserialize_message(&line)?;
        match msg {
            JsonRpcMessage::Request(req) => {
                let result = server.handle_request(req).await?;
                let response = JsonRpcMessage::Result(result);
                let mut out = serialize_message(&response)?;
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
            }
            JsonRpcMessage::Notification(notif) => {
                let _ = server.handle_notification(notif).await;
            }
            _ => {}
        }
    }
    Ok(())
}
```

### Tool 注册

```rust
// task-mcp-server/src/main.rs
fn main() {
    let db_path = resolve_db_path();  // --db-path CLI arg
    let db = Arc::new(TaskDb::open(&db_path).unwrap());

    let mut server = McpServer::new(
        Implementation {
            name: "task".into(),
            title: Some("Task Management".into()),
            version: "0.1.0".into(),
        },
        ServerOptions::default(),
    );

    server.register_tool(
        make_tool("task_create", "Create a new task", CREATE_SCHEMA),
        TaskCreateHandler { db: db.clone() },
    ).unwrap();

    // ... 同理注册 show/list/update/delete

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_stdio(Arc::new(server.server().clone()))).unwrap();
}
```

### ToolHandler 实现

```rust
struct TaskCreateHandler {
    db: Arc<TaskDb>,
}

#[async_trait]
impl ToolHandler for TaskCreateHandler {
    async fn call(
        &self,
        arguments: Option<Value>,
        _context: RequestContext,
    ) -> Result<CallToolResult, ServerError> {
        let args = arguments.unwrap_or_default();
        let params = parse_create_params(args)
            .map_err(|e| ServerError::Handler(e.to_string()))?;
        let task = self.db.create_task(&params)
            .map_err(|e| ServerError::Handler(e.to_string()))?;
        Ok(CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                type_: "text".into(),
                text: serde_json::to_string(&task).unwrap(),
            })],
            ..Default::default()
        })
    }
}
```

### 使用方式

在 `.loom/mcp.json` 中配置：

```json
{
  "mcpServers": {
    "task": {
      "command": "task-mcp-server",
      "args": ["--db-path", ".loom/tasks.db"]
    }
  }
}
```

Loom 启动时通过 `McpToolSource::new` spawn 子进程，自动 `tools/list` 获取 task tools。

## 方案对比

| 维度 | 方案 A (Native Tool) | 方案 B (MCP Server) |
|------|---------------------|---------------------|
| **延迟** | 直接函数调用，零开销 | 进程间 JSON-RPC，有序列化开销 |
| **部署** | 编译进 loom，无需额外进程 | 独立二进制，需配置 mcp.json |
| **复用性** | 仅 Loom 内部 | 任何 MCP 客户端都能用 |
| **并发** | `Mutex<Connection>` 共享状态 | 独立进程，天然隔离 |
| **开发量** | 5 个 Tool 实现 + 注册逻辑 | 5 个 ToolHandler + stdio transport |
| **适用场景** | Loom agent 内部使用 | 外部系统集成、多客户端共享 |

## 实施计划

### Phase 1：拆分 task-core

1. 从 `task/` 提取 `task-core`（lib）：`models.rs`、`db.rs`（加 `Mutex`）、`params.rs`（去 clap）
2. 创建 `task-cli`（bin）：保留 `main.rs`、`args.rs`（依赖 clap），委托 `task-core`
3. 更新 workspace `Cargo.toml`
4. 验证 `task-cli` 功能与现有一致

### Phase 2：方案 A — Loom Native Tools

1. `loom/Cargo.toml` 增加 `task-core` 依赖
2. 实现 `loom/src/tools/task/` 下 5 个 Tool
3. 在 `build_tool_source` 中注册
4. 编写单元测试
5. 集成测试：验证 agent 可以通过 tool 管理 task

### Phase 3：方案 B — MCP Server

1. 创建 `task-mcp-server` crate
2. 实现 stdio transport
3. 注册 5 个 ToolHandler
4. 集成测试：通过 `McpToolSource` 连接并调用
5. 文档更新：使用方式、mcp.json 配置示例

## 风险与缓解

| 风险 | 缓解措施 |
|------|---------|
| `mcp_server` crate 使用 `edition 2024` | 可 fork 并降级 edition，或升级 workspace edition |
| SQLite 并发写入冲突 | `Mutex` 保证同一时间只有一个写事务 |
| `task-core` API 变更影响 CLI 和 MCP | 统一参数类型，CLI/MCP 只做参数转换 |
| stdio transport 边界情况（大消息、编码） | 使用 `ReadBuffer` 逐行解析，复用 `mcp_core` 的序列化 |

## 相关概念

- [工具系统](../tools/tool-system.md) — Tool trait、ToolSource、AggregateToolSource
- [MCP 集成](../tools/mcp.md) — McpToolSource、McpToolAdapter
- [ReAct 运行模式](../core/react.md) — 工具在智能体循环中的调用流程
