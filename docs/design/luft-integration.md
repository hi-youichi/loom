# Luft 工作流引擎集成方案

> 将 Luft（Lua 多 Agent 编排运行时）直接整合为 Loom 的内置工具，而非通过 MCP 协议通信。

**创建时间**：2025-08-19  
**状态**：方案已确认，待实施

---

## 1. 背景与动机

Luft 是一个 Rust 编写的多 Agent 编排运行时，用 Lua 脚本协调多个 LLM Agent 的执行（顺序、并行、流水线、共识模式）。当前 Luft 通过 MCP Server（stdio JSON-RPC）暴露 `execute_workflow` 等工具供外部调用。

**为什么不用 MCP？**

- 进程间通信开销：每次 tool call 都涉及 stdin/stdout 序列化
- MCP 协议限制：stdio transport 不支持双向流式、不支持取消传播
- 调试困难：跨进程边界的事件追踪复杂

**目标**：Luft 作为 Loom 的 `Tool` 直接嵌入，共享同一个进程内的 LLM provider、工具注册表和事件总线。

---

## 2. 架构概览

```
Loom Agent (parent)
  └─ tool_call: "luft"
      └─ LuftTool::call(args, ctx)
          ├─ let mut builder = LuftBuilder::new();
          │   builder.backend(LoomAgentBackend::from_config(template))
          │           .base_dir(working_folder);
          │   let mut luft = builder.build()?;   // &mut self
          │
          ├─ luft.start_script(lua_source) → RunHandle
          ├─ let event_rx = run_handle.subscribe();   // 拿 receiver（join 前必须先 subscribe）
          │
          ├─ spawn: while recv event_rx → ctx.any_stream_event_sender(json)   // 实时进度
          │
          ├─ Luft Runtime::execute() → mlua VM
          │     → agent({ prompt, schema })
          │         → Scheduler::run_agent(task)
          │             → LoomAgentBackend::run(task, ctx)
          │                 ├─ Agent::from_config(template)
          │                 │     + StructuredOutputTool (if schema)
          │                 │     + BuiltinToolFilter (if allowlist)
          │                 ├─ agent.run(prompt, |ev| event_bridge(ev))
          │                 └─ → AgentResult { output, tokens_used }
          │
          └─ done_rx.recv() → RunDone { report, status, total_tokens }
              └─ → ToolCallContent::Text(report_json)
```

**关键约束**：`RunHandle::join()` 消费 self（`luft/src/builder.rs:378`），与 `cancel()` 的 `&self` 签名不兼容。因此 `LuftTool` **不使用 `join()`**，改用 event subscription 检测 `RunDone` 完成事件（详见 §4.3）。

---

## 3. 三个核心组件

### 3.1 LuftTool（主入口）

**职责**：暴露给 Loom parent agent 的 Tool，接收 Lua 脚本或 workflow 路径，启动 Luft 运行时并等待完成。

**Tool spec**:

```json
{
  "name": "luft",
  "description": "Execute a multi-agent workflow. Supports Lua scripts and pre-defined workflow files.",
  "input_schema": {
    "type": "object",
    "properties": {
      "script": {
        "type": "string",
        "description": "Inline Lua script. Use this for dynamic workflows."
      },
      "workflow": {
        "type": "string",
        "description": "Name or path of a pre-defined .lua workflow file."
      },
      "args": {
        "type": "object",
        "description": "Arguments passed to the workflow.",
        "additionalProperties": true
      }
    }
  }
}
```

**执行逻辑**:

1. 从 `args` 解析 `script` / `workflow` / `args`
2. 如果指定 `workflow`：从 `.luft/workflows/` 或 working folder 查找 `.lua` 文件
3. 构建 `LoomAgentBackend`（注入 parent agent 的 config snapshot）
4. `let mut builder = LuftBuilder::new(); builder.backend(backend).base_dir(working_folder); let mut luft = builder.build()?;`
   - 注意 `build()` 签名是 `&mut self`（`luft/src/builder.rs:117`），不能链式调用
5. `luft.start_script(source)` → `RunHandle`
6. `run_handle.subscribe()` 两次：一个用于事件转发（`forward_rx`），一个用于完成检测（`done_rx`）
7. spawn 一个 task：循环 `forward_rx.recv().await`，将 Luft 事件序列化为 JSON，通过 `ctx.any_stream_event_sender`（`tool-core/src/context.rs:10`）推送给 parent agent
8. 主循环 `loop { select! { done_rx.recv() | cancel_token } }`（详见 §4.3）：
   - 收到 `RunDone { report, .. }` → 提取 `report` JSON，返回
   - parent 取消 → `run_handle.cancel()`，继续 loop 等待 `RunDone(Cancelled)`
9. 返回 `ToolCallContent::Text(serde_json::to_string(report))`

**Cancellation**：通过 `ctx.run_cancellation`（`ToolCallContext`）检测 parent agent 的取消信号。cancel 后不立即返回，而是等待 Luft Scheduler emit `RunDone(Cancelled)` 确保资源清理完成（详见 §4.3）。

**Tool output hint**: `ToolOutputHint::preferred(ToolOutputStrategy::FileRefWithExcerpt)` — 工作流结果可能很长，优先写文件引用。

### 3.2 LoomAgentBackend（Backend 适配）

**职责**：实现 Luft 的 `AgentBackend` trait，每个 Lua `agent()` 调用 → 启动一个完整的 Loom agent。

```rust
pub struct LoomAgentBackend {
    config_template: AgentConfig,
}

#[async_trait]
impl AgentBackend for LoomAgentBackend {
    fn id(&self) -> &'static str { "loom" }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            streaming: true,
            mcp_injection: true,
            structured_output: true,
            models: vec![],  // 继承 template 配置
        }
    }

    async fn run(
        &self,
        task: AgentTask,
        ctx: RunContext,
    ) -> Result<AgentResult, BackendError> {
        // 1. 从 template clone config
        let mut config = self.config_template.clone();

        // 2. 为 structured output 创建共享 slot（必须在构建 tool 前创建）
        let output_slot = Arc::new(Mutex::new(None::<serde_json::Value>));

        // 3. 如果 task.output_schema 存在：
        //    创建 StructuredOutputTool（持有 output_slot.clone()）
        //    注册到 config.extra_tools
        // 4. 如果 task.allowlist 存在：
        //    设置 config.builtin_tool_filter
        // 5. 如果 task.model 存在：
        //    覆盖 config 的 model
        // 6. 构建 Agent（消费 config）
        let agent = Agent::from_config(config).await?;

        // 7. Token 累计 + 事件桥接
        //    agent.run() 要求 on_event: F: FnMut(AgentEvent) + Send + Sync + Clone + 'static
        //    Rust 闭包不自动 impl Clone（即使所有捕获都是 Clone）。
        //    实现时需用 named struct 包装捕获值并 derive(Clone)，或检查
        //    agent.run() 是否可放宽 Clone bound（如改用 Arc<dyn Fn>)。
        let tokens = Arc::new(Mutex::new(TokenUsage::default()));
        let event_sender = ctx.events.clone();
        let agent_id = task.agent_id;
        let slot = output_slot.clone();

        let run = tokio::select! {
            result = agent.run(&task.prompt, {
                let tokens = tokens.clone();
                let event_sender = event_sender.clone();
                move |loom_ev: AgentEvent| {
                    match &loom_ev {
                        AgentEvent::Usage { prompt_tokens, completion_tokens, cached_tokens, .. } => {
                            let mut t = tokens.lock().unwrap();
                            t.input += *prompt_tokens as u32;
                            t.output += *completion_tokens as u32;
                            t.cache_read += *cached_tokens as u32;
                        }
                        _ => {
                            if let Some(delta) = map_loom_event_to_delta(&loom_ev) {
                                let _ = event_sender.send(AgentEvent::AgentProgress {
                                    agent_id,
                                    delta,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }) => result,
            _ = ctx.cancel.cancelled() => {
                return Err(BackendError::Cancelled);
            }
        };

        // 8. 提取结果
        let output = match &run {
            Ok(result) => {
                // 如果 output_slot 有值（structured_output 被调用过），用它
                // 否则用 result.reply 作为 string value
                slot.lock().unwrap()
                    .take()
                    .unwrap_or_else(|| Value::String(result.reply.clone()))
            }
            Err(_) => {
                return Err(BackendError::Execution("agent error".into()));
            }
        };

        let tokens = tokens.lock().unwrap().clone();

        Ok(AgentResult {
            agent_id: task.agent_id,
            status: AgentStatus::Ok,
            output,
            findings: vec![],
            tokens_used: tokens,
            artifacts: vec![],
            logs: LogRef::default(),
        })
    }
}
```

### 3.3 StructuredOutputTool（Schema 验证）

**职责**：当 Luft 的 `agent({ schema = {...} })` 注入 `"You MUST call the structured_output tool"` 指令后，子 agent 通过此 tool 提交结构化 JSON。

```rust
pub struct StructuredOutputTool {
    schema: serde_json::Value,
    output_slot: Arc<Mutex<Option<serde_json::Value>>>,
}

#[async_trait]
impl Tool for StructuredOutputTool {
    fn name(&self) -> &str { "structured_output" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "structured_output".into(),
            description: Some(
                "Submit your final structured result. \
                 You MUST call this tool to complete the task.".into()
            ),
            input_schema: self.schema.clone(),
            output_hint: Some(
                ToolOutputHint::preferred(ToolOutputStrategy::Inline)
            ),
        }
    }

    async fn call(
        &self,
        args: Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        // 验证 JSON Schema（使用 jsonschema crate）
        if let Err(errors) = validate_json_schema(&args, &self.schema) {
            return Ok(ToolCallContent::Text(format!(
                "Schema validation failed: {}. Please fix and retry.",
                errors
            )));
        }

        *self.output_slot.lock().unwrap() = Some(args);
        Ok(ToolCallContent::Text("Result submitted.".into()))
    }
}
```

**注意**：验证失败时不返回 `Err`，而是返回文本提示让 agent 重试。这与 Luft 原有的 `SchemaRetry` 事件对齐。

---

## 4. 事件桥接

### 4.1 Loom → Luft（子 agent 进度回传）

子 agent 执行过程中，Loom `AgentEvent` → Luft `ProgressDelta`：

| Loom `AgentEvent` | Luft `ProgressDelta` | 说明 |
|---|---|---|
| `TextChunk(text)` | `Message { text }` | Agent 输出文本片段 |
| `ToolCallStart { name, arguments }` | `ToolCall { name, summary }` | Agent 开始调用工具 |
| `ToolEnd { name, is_error }` | `ToolCall { name, summary: "done" }` | 工具调用完成 |
| `Usage { prompt_tokens, completion_tokens, ... }` | （不转发） | 被 `LoomAgentBackend::run()` 消费用于 token 累计（§10），不转发为 `ProgressDelta` |

其余事件通过 `ctx.events.send(AgentEvent::AgentProgress { ... })` 发送到 Luft 的事件总线。`Usage` 被 `LoomAgentBackend` 内部消费（写入 `Arc<Mutex<TokenUsage>>`），不向外转发。

### 4.2 Luft → Loom（工作流级别事件）

Luft 工作流执行产生的事件回传到 Loom parent agent。

**机制**：`LuftTool::call()` 通过 `ToolCallContext.any_stream_event_sender`（`tool-core/src/context.rs:10`）推送进度。这是一个 `Arc<dyn Fn(serde_json::Value) + Send + Sync>`，可以在 tool 执行期间随时调用。

`LuftTool::call()` 启动一个后台 task 订阅 `run_handle.subscribe()` 的 broadcast receiver，将每个 Luft `AgentEvent` 序列化为 JSON 后调用 `any_stream_event_sender`：

```rust
let event_rx = run_handle.subscribe();
let sender = ctx.any_stream_event_sender.clone();
tokio::spawn(async move {
    while let Ok(ev) = event_rx.recv().await {
        if let Some(ref s) = sender {
            s(serde_json::to_value(&ev).unwrap_or_default());
        }
    }
});
```

| Luft `AgentEvent` | JSON payload | 说明 |
|---|---|---|
| `RunStarted` | `{"type":"run_started",...}` | 工作流开始 |
| `PhaseStarted` / `PhaseDone` | `{"type":"phase_started",...}` | 阶段进度 |
| `AgentStarted` / `AgentDone` | `{"type":"agent_started",...}` | 子 agent 生命周期 |
| `ParallelStarted` / `ParallelDone` | `{"type":"parallel_started",...}` | 并行执行进度 |
| `PipelineStarted` / `PipelineDone` | `{"type":"pipeline_started",...}` | 流水线进度 |
| `WorkflowStarted` / `WorkflowDone` | `{"type":"workflow_started",...}` | 嵌套子工作流 |
| `ConvergeStarted` / `ConvergeDone` | `{"type":"converge_started",...}` | 共识迭代 |
| `Log { msg, level }` | `{"type":"log",...}` | 日志消息 |

**设计决策**：通过 `any_stream_event_sender` 推送序列化的 JSON，TUI 层负责解析并渲染进度。不等待工作流完成一次性返回。

### 4.3 Cancellation 双向桥接

```
Loom parent agent 取消
  → ToolCallContext.run_cancellation 触发
    → LuftTool::call() 检测到取消
      → RunHandle.cancel()         // &self，不消费 run_handle
        → Scheduler CancellationToken 触发
          → LoomAgentBackend::run() 的 ctx.cancel 触发
            → tokio::select! 分支命中 → Loom 子 agent 取消
              → Scheduler emit RunDone(Cancelled)
                → LuftTool 通过 event_rx 收到 RunDone → 返回
```

Cancellation 来源：`ToolCallContext.run_cancellation: Option<RunCancellation>`（`tool-core/src/context.rs:15`）。

**所有权问题**：`RunHandle::join()` 消费 self（`luft/src/builder.rs:378`），而 `cancel()` 是 `&self`（`:368`）。无法在 `select!` 中同时使用 `join()` 和 `cancel()`，因为 `join()` 会 move `run_handle` 到 select 分支。

**解决方案**：不使用 `join()`，改用 **event subscription 检测完成**。Luft Scheduler 在工作流结束时 emit `AgentEvent::RunDone`（`luft-core/src/contract/event.rs`），其中包含 `report`、`status` 和 `total_tokens`。通过 `run_handle.subscribe()` 获取的 broadcast receiver 可以接收到这个事件。

```rust
let run_handle = luft.start_script(source)?;

// 两个 subscriber：一个转发事件，一个检测完成
let mut forward_rx = run_handle.subscribe();
let mut done_rx = run_handle.subscribe();

// 转发 Luft 事件到 parent agent
let sender = ctx.and_then(|c| c.any_stream_event_sender.clone());
tokio::spawn(async move {
    while let Ok(ev) = forward_rx.recv().await {
        if let Some(ref s) = sender {
            s(serde_json::to_value(&ev).unwrap_or_default());
        }
    }
});

// 用 CancellationToken 桥接 parent 取消（避免 select! 重复触发）
let cancel_token = tokio_util::sync::CancellationToken::new();
if let Some(ctx) = ctx {
    if let Some(ref rc) = ctx.run_cancellation {
        let ct = cancel_token.clone();
        let rc = rc.clone();
        tokio::spawn(async move {
            rc.cancelled().await;
            ct.cancel();
        });
    }
}

// 主循环
let mut cancelled = false;
loop {
    if cancelled {
        // 已取消：只等待 RunDone，不再 select! cancel_token（避免空转）
        match done_rx.recv().await {
            Ok(AgentEvent::RunDone { report, .. }) => {
                return Ok(ToolCallContent::Text("Workflow cancelled".into()));
            }
            Ok(_) => continue,
            Err(_) => return Err(ToolSourceError::ToolError("event channel closed")),
        }
    }

    tokio::select! {
        // 检测工作流完成
        ev = done_rx.recv() => {
            match ev {
                Ok(AgentEvent::RunDone { report, .. }) => {
                    let text = serde_json::to_string(&report.unwrap_or_default())?;
                    return Ok(ToolCallContent::Text(text));
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(ToolSourceError::ToolError("event channel closed"));
                }
            }
        }
        // Parent 取消（只触发一次）
        _ = cancel_token.cancelled(), if !cancelled => {
            cancelled = true;
            run_handle.cancel();  // &self — 不消费 run_handle
            // 下一轮迭代进入 cancelled 分支，纯等 RunDone
        }
    }
}
```

**设计要点**：
- `cancelled` flag 确保取消逻辑只执行一次：cancel 后切换到纯 `done_rx.recv().await` 等待 `RunDone(Cancelled)`，避免 `cancel_token.cancelled()` 在后续迭代中持续立即返回导致 select! 空转
- `run_handle.cancel()` 是 `&self`，不消费 `run_handle`，可在 cancel 分支中安全调用
- 不调用 `join()`，Luft 内部 spawned task 会自行完成并清理；`RunHandle` drop 时不会 abort 任务
- **Luft API 改进建议**：理想情况下 `RunHandle` 应提供 `cancel_token() -> &CancellationToken` 访问器，或实现 `Clone`，这样可以直接将 Luft 的 CancellationToken 与 Loom 的 RunCancellation 做双向 link（`select! { a.cancelled() => b.cancel(); b.cancelled() => a.cancel() }`），消除对 polling 的需要

---

## 5. AgentConfig 模板策略

`LoomAgentBackend` 从 parent agent 的 config snapshot 构建：

| 配置项 | 来源 | 说明 |
|--------|------|------|
| Model | Parent config + `task.model` 覆盖 | 保持模型一致性 |
| Working folder | Parent config + `task.workdir` 覆盖 | 默认继承 |
| System prompt | Parent config | 继承 parent 的 system prompt |
| Builtin tools | Parent registry + `task.allowlist` 过滤 | 可限制子 agent 权限 |
| Extra tools | `+ StructuredOutputTool`（如果 schema 存在） | 动态注入 |
| Checkpointer | 通过 `build_checkpointer` 默认构建，可返回 None | 子 agent 不做 checkpoint（`agent-core/src/agent/react/build/runners.rs:30`） |
| Depth | Parent depth + 1 | 递归深度保护，上限 3 层，防止 LLM 生成递归 Lua 脚本（`tool-core/src/context.rs:14`） |
| Compression | Parent config | 继承上下文压缩策略 |

**构造方式**：

```rust
impl LoomAgentBackend {
    pub fn from_agent(agent: &Agent) -> Self {
        Self {
            config_template: agent.config_snapshot(),
        }
    }
}
```

---

## 6. Luft 运行时配置集成

`LuftTool` 在构建 `LuftBuilder` 时需要配置 `concurrency` / `exec_limits` / `base_dir`。这些参数从 Loom 的 agent config 中读取，支持用户自定义：

```toml
# ~/.config/loom/config.toml 或 .loom/config.toml
[luft]
concurrency = 4              # 最大并行 agent 数（默认 4）
base_dir = ".luft/runs"     # Luft run artifacts 存储路径（默认 .luft/runs）
max_rounds = 10             # Lua 脚本最大执行轮次
timeout_secs = 600          # 单个 agent 最大执行时间
```

**优先级**：Loom config `[luft]` section > 默认值。`LoomAgentBackend` 的 `config_template` 继承 parent agent 配置，但 model 可被 `AgentTask.model` 覆盖。

**默认值**：
- `concurrency`: 4（防止首次使用耗尽 LLM rate limit）
- `base_dir`: `{working_folder}/.luft/runs`
- `max_rounds`: 10
- `timeout_secs`: 600（10 分钟）

---

## 7. Workflow 发现机制

`LuftTool` 接收 `workflow` 参数时，按以下优先级查找 `.lua` 文件：

1. `.luft/workflows/{name}.lua` — 项目级工作流目录
2. `~/.config/luft/workflows/{name}.lua` — 用户级工作流目录（跨项目共享）
3. `{name}` 作为相对路径 — 直接文件路径
4. `{name}` 作为绝对路径

如果都不存在，返回 `ToolCallContent::Text` 错误提示。

---

## 8. Crate 结构

### 8.1 新 crate: `tool-luft`

```
agent/tool/tool-luft/
  ├── Cargo.toml
  ├── src/
  │   ├── lib.rs                  # register_luft_tool(registry, config_template)
  │   ├── tool.rs                 # LuftTool (impl Tool)
  │   ├── backend.rs              # LoomAgentBackend (impl AgentBackend)
  │   ├── structured_output.rs    # StructuredOutputTool (impl Tool)
  │   ├── event_bridge.rs         # Luft ↔ Loom 事件映射
  │   └── workflow_resolver.rs    # workflow 路径解析
  └── tests/
      ├── integration.rs          # 端到端：Lua script → Loom agent → result
      ├── backend.rs              # LoomAgentBackend 单元测试
      └── structured_output.rs    # Schema 验证测试
```

### 8.2 Cargo.toml 依赖

Luft crate 已发布到 crates.io（luft, luft-core, luft-storage, luft-runtime, luft-adapters, luft-planner, luft-service, luft-mcp, luft-cli，均为 0.3.0）。Loom **不**使用 path/git 依赖 luft，全部走 crates.io，避免脆弱的相对路径和 monorepo 假设：

```toml
# 在 tool-workflow/Cargo.toml 中直接引用 crates.io 版本
[dependencies]
luft = "0.3"
luft-core = "0.3"
agent-core = { path = "../../agent-core" }   # loom 内部 crate 仍用 path
tool-core = { path = "../tool-core" }
loom-llm = { path = "../../../foundation/llm" }
async-trait = { workspace = true }
tokio = { workspace = true }
serde_json = { workspace = true }
jsonschema = "0.22"     # JSON Schema 验证
tracing = { workspace = true }
```

### 8.3 注册入口

在 `tool-basic/src/lib.rs` 中添加：

```rust
pub async fn register_luft_tool(
    registry: &ToolRegistryLocked,
    config_template: AgentConfig,
) {
    registry.register_async(Box::new(
        tool_luft::LuftTool::new(config_template)
    )).await;
}
```

---

## 9. 使用示例

### 9.1 Loom agent 动态生成 Lua 脚本

```
User: 分析 src/ 目录的安全问题，并给出严重级别分类

Loom Agent:
  → tool_call: luft
    args: {
      "script": "
        local results = parallel({'src/auth', 'src/api', 'src/db'}, function(dir)
          return agent({
            prompt = 'Analyze security issues in ' .. dir,
            schema = {
              type = 'object',
              properties = {
                issues = { type = 'array', items = { type = 'string' } },
                severity = { type = 'string', enum = {'low','medium','high'} }
              }
            }
          })
        end)
        -- parallel 返回 result table 数组: { status, ok, output, tokens, findings }
        local merged = {}
        for i, r in ipairs(results) do
          merged[i] = r.output
        end
        report(merged)
      "
    }
```

**注意**：`parallel()` 返回 result table 数组（`luft-runtime/src/sdk/agent/task.rs:80-103`），每个元素是 `{ status, ok, output, tokens, findings }`。需要通过 `.output` 访问实际结果。

### 9.2 执行已有 workflow

```
User: 运行代码审查工作流

Loom Agent:
  → tool_call: luft
    args: {
      "workflow": "code-review",
      "args": { "target": "src/auth/", "depth": "deep" }
    }
```

对应 `.luft/workflows/code-review.lua`:
```lua
function main(args)
    local target = args.target or 'src/'
    local depth = args.depth or 'standard'

    pipeline({'lint', 'security', 'quality'}, function(stage)
        return agent({
            prompt = string.format("Run %s analysis on %s (%s mode)", stage, target, depth),
            schema = {
                type = 'object',
                properties = {
                    score = { type = 'number' },
                    findings = { type = 'array', items = { type = 'string' } }
                }
            }
        })
    end)
end
```

### 9.3 共识模式

`converge(items, options)` 接收两个参数（`luft-runtime/src/converge.rs:414`）：
- `items`：要验证的 JSON-serializable 值数组
- `options`：`{ adversarial, vote_threshold, max_rounds, producers_per_item, ... }`

```lua
function main()
    -- 先用多个 agent 生成候选方案
    local candidates = parallel({'rust', 'go', 'python'}, function(lang)
        return agent({
            prompt = 'Argue why ' .. lang .. ' is best for a new microservice',
            schema = {
                type = 'object',
                properties = {
                    language = { type = 'string' },
                    arguments = { type = 'array', items = { type = 'string' } },
                }
            }
        }).output
    end)

    -- 对抗式共识验证
    local result = converge(candidates, {
        adversarial = true,
        max_rounds = 3,
    })
    -- result = { surviving, rounds, converged, findings }
    report(result.surviving)
end
```

---

## 10. Token 用量追踪

`LoomAgentBackend::run()` 在执行过程中通过 `Arc<Mutex<TokenUsage>>` 累计子 agent 的 token（闭包需要 `Clone + Send + Sync`，不能直接捕获 `&mut`）：

```rust
let tokens = Arc::new(Mutex::new(TokenUsage::default()));

agent.run(&task.prompt, {
    let tokens = tokens.clone();
    move |ev: AgentEvent| {
        if let AgentEvent::Usage { prompt_tokens, completion_tokens, cached_tokens, .. } = ev {
            let mut t = tokens.lock().unwrap();
            t.input += prompt_tokens as u32;
            t.output += completion_tokens as u32;
            // Loom cached_tokens → Luft cache_read
            t.cache_read += cached_tokens as u32;
        }
        // ... 事件桥接
    }
}).await?;

let tokens = tokens.lock().unwrap().clone();

// 填入 AgentResult
AgentResult {
    tokens_used: tokens,
    ..
}
```

**Token 字段映射**：

| Loom `AgentEvent::Usage` | Luft `TokenUsage` | 说明 |
|---|---|---|
| `prompt_tokens` | `input` | 输入 token |
| `completion_tokens` | `output` | 输出 token |
| `cached_tokens` | `cache_read` | 缓存命中 token |
| （无） | `cache_write` | Loom 不区分 cache_write，默认 0 |

Luft Scheduler 会在 `RunDone` 事件中汇总所有 agent 的 token 总量。

---

## 11. 错误处理

| 场景 | 行为 |
|------|------|
| Lua 脚本语法错误 | `LuftTool::call()` 返回 `ToolCallContent::Text` 错误描述 |
| 子 agent 执行失败 | `LoomAgentBackend::run()` 返回 `AgentStatus::Error`，Luft Scheduler 决定是否重试 |
| Schema 验证失败 | `StructuredOutputTool` 返回文本提示，子 agent 自行重试 |
| 超时 | Scheduler 层 `tokio::time::timeout`，返回 `AgentStatus::TimedOut` |
| 用户取消 | Cancellation 传播：`LuftTool` 检测 `ToolCallContext.run_cancellation` → `RunHandle.cancel()` → `LoomAgentBackend` 的 `ctx.cancel` → 返回 `BackendError::Cancelled`。`LuftTool` 返回 `ToolSourceError::ToolError("cancelled")`（`ToolSourceError` 无 `Cancelled` 变体，`foundation/llm/src/tool.rs:170-181`） |
| Workflow 文件不存在 | `LuftTool::call()` 返回错误描述 |

---

## 12. 安全考虑

- **Lua 沙箱**：Luft 的 mlua VM 已移除 `io.* / os.* / package.* / require`，LLM 生成的脚本无法逃逸
- **工具权限**：`task.allowlist` 可以限制子 agent 的工具集（如禁止 `bash`）
- **Schema 验证**：`StructuredOutputTool` 使用 `jsonschema` crate 做严格验证，防止注入
- **工作目录隔离**：子 agent 继承 parent 的 working folder，但 Luft 的 `base_dir` 可以独立配置
- **递归深度保护**：`ToolCallContext.depth`（`tool-core/src/context.rs:14`）记录调用深度。`LuftTool` 应检查 depth，当 `depth >= 3` 时拒绝执行（防止 LLM 生成递归 Lua 脚本导致无限嵌套）。子 agent 的 config template 设置 `depth + 1`
- **并发限制**：Luft `LuftBuilder::concurrency()` 控制最大并行 agent 数，防止资源耗尽

---

## 13. 实施路线

| 阶段 | 内容 | 预估时间 |
|------|------|----------|
| 一 | 创建 `tool-luft` crate，实现 `LoomAgentBackend` 基本流程（不带 schema） | 3h |
| 二 | 实现 `StructuredOutputTool` + schema 验证 | 2h |
| 三 | 实现 `LuftTool` + workflow 解析 + 事件桥接 | 3h |
| 四 | 集成测试（mock backend + 真实 Lua 脚本） | 2h |
| 五 | Cancellation 传播 + token 追踪 | 1h |

**总计**：约 11h

### 验收标准

- [ ] `cargo build --workspace` 通过
- [ ] `cargo test -p tool-luft` 通过
- [ ] Loom agent 能通过 `luft` tool 执行内联 Lua 脚本
- [ ] Loom agent 能通过 `luft` tool 执行已有 workflow 文件
- [ ] Structured output schema 验证工作正常
- [ ] 事件实时桥接到 parent agent TUI
- [ ] Cancellation 正确传播
- [ ] Token 用量正确汇总

---

## 附录 A：Luft SDK 原语参考

| 原语 | 签名 | 返回值 | 说明 |
|------|------|--------|------|
| `agent(opts)` | `opts.prompt, opts.schema, opts.model` | `{ status, ok, output, tokens, findings }` | 单 agent 执行 |
| `parallel(items, map_fn)` | items 数组 + map 函数 | result table 数组 | barrier fan-out，等所有完成 |
| `pipeline(items, stages)` | items 数组 + stage 函数 | result table 数组 | streaming，不 barrier |
| `converge(items, options)` | items 数组 + `{ adversarial, max_rounds, ... }` | `{ surviving, rounds, converged, findings }` | 多轮共识验证 |
| `workflow(path, args?)` | 路径 + 可选参数 | 子工作流 report 值 | 嵌套子工作流 |
| `phase_begin(name)` / `phase_end(span)` | 阶段名 | span handle | 结构化进度 |
| `report(value)` | JSON-serializable 值 | — | 最终输出，必须调用 |
| `log(msg, level?)` | 消息 + 可选级别 | — | 日志 |
| `budget(time_ms?, rounds?)` | 时间/轮数限制 | — | 运行时限制 hint |

## 附录 B：关键类型对照

| Luft | Loom | 说明 |
|------|------|------|
| `AgentTask` | — | Luft 的 agent 任务描述 |
| `AgentResult` | `AgentRunResult` | 执行结果 |
| `AgentEvent` | `AgentEvent` (loom) | 事件类型 |
| `RunContext` | — | Luft 运行上下文（cancel + events） |
| `AgentBackend` | `Tool` | 核心 trait 对接点 |
| `output_schema: Value` | `input_schema: Value` (ToolSpec) | JSON Schema |
| `CancellationToken` | `RunCancellation` | 取消机制 |
| `LogRef` | — | Luft 日志引用（`LogRef::default()` 构造，无 `none()` 方法） |
| `RunOutcome` | — | `{ run_id, run_dir_name, result: Result<Value, ScriptError> }`（`luft/src/builder.rs:393-400`） |
| `ToolSourceError` | — | Loom 工具错误（无 `Cancelled` 变体，用 `ToolError` 代替） |
| `any_stream_event_sender` | — | `ToolCallContext` 的进度回调（`tool-core/src/context.rs:10`） |
| `depth: u32` | — | `ToolCallContext` 的调用深度（`tool-core/src/context.rs:14`） |
