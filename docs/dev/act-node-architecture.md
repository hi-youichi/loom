# ActNode 架构文档

> 文件路径：`loom/src/agent/react/act_node.rs`

## 概述

`ActNode` 是 ReAct（Reasoning + Acting）循环中的**执行节点**，负责接收 LLM 产出的工具调用（`tool_calls`），依次调用 `ToolSource` 执行工具，并将结果（`tool_results`）写回状态。

在 ReAct 图中的位置：

```
ThinkNode → ActNode → ObserveNode → ThinkNode → ...
```

- **输入**：`ReActState.tool_calls`（由 ThinkNode 从 LLM 输出解析得到）
- **输出**：`ReActState.tool_results`（由 ActNode 执行工具后填充，ObserveNode 消费）
- **节点 ID**：`"act"`

## 核心结构

### `ActNode`

```rust
pub struct ActNode {
    tools: Box<dyn ToolSource>,        // 工具来源（MCP server、内置工具等）
    approval_policy: Option<ApprovalPolicy>, // 审批策略
}
```

- `tools`：实现 `ToolSource` trait 的动态分发对象，提供 `list_tools`、`call_tool`、`call_tool_with_context` 等方法。
- `approval_policy`：可选的审批策略，部分危险工具（如文件删除）执行前需要用户确认。

### 依赖关系

```
ActNode
 ├── ToolSource        — 工具调用执行（trait, dyn dispatch）
 ├── ToolCallContext   — 传递给工具的上下文（消息历史、stream writer 等）
 ├── RunContext        — 图运行时上下文（stream、cancel、config）
 ├── normalize_tool_output — 工具输出标准化（截断、存储、预算控制）
 ├── StreamEvent       — 流式事件（ToolStart / ToolEnd / ToolOutput / ToolApproval）
 └── ReActState        — ReAct 循环的全局状态
```

## 执行流程

### `run` 方法（无 stream/cancel）

```rust
async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError>
```

1. 构造 `ToolCallContext`（含消息历史），设置到 `ToolSource`
2. 加载工具输出提示（`load_tool_output_hints`），获取每个工具的 `ToolOutputHint`
3. 遍历 `state.tool_calls`，对每个 `ToolCall`：
   - **审批检查**：若 `needs_approval(tc.name)` 为 true：
     - `approval_result == None` → 返回 `Interrupted`（中断等待用户确认）
     - `approval_result == Some(false)` → 写入 "User rejected" 错误结果
     - `approval_result == Some(true)` → 继续执行
   - **解析参数**：`parse_tool_arguments(&tc.arguments)` → `Value`
   - **调用工具**：`tools.call_tool_with_context(name, args, Some(&ctx))`
   - **成功**：标准化输出 → 构建 `ToolResult`（含 call_id、name）
   - **失败**：用 `DEFAULT_EXECUTION_ERROR_TEMPLATE` 构建错误信息 → 标准化 → 构建 `ToolResult`（is_error=true）
4. 调用 `backfill_tool_result_call_ids` 确保每个 `ToolResult` 都有 `call_id`
5. 清除 `ToolSource` 的 call context
6. 返回新状态（`tool_results` 已填充，`approval_result` 如已消费则置 None）

### `run_with_context` 方法（支持 stream/cancel）

```rust
async fn run_with_context(
    &self, state: ReActState, run_ctx: &RunContext<ReActState>,
) -> Result<(ReActState, Next), AgentError>
```

与 `run` 相同的核心逻辑，额外支持：

| 功能 | 实现方式 |
|------|----------|
| **取消检查** | 每次工具调用前检查 `CancellationToken`；通过 `run_cancellable` 包裹工具调用 |
| **流式事件** | 根据 `StreamMode` 发送 `ToolStart` / `ToolOutput` / `ToolEnd` / `ToolApproval` 事件 |
| **自定义流** | `StreamMode::Custom` 时，通过 `ToolStreamWriter` 允许工具在执行中发送中间进度 |
| **工具输出流** | `StreamMode::Tools` 时，为每个工具创建独立 `ToolStreamWriter`，将输出截断后发送 `ToolOutput` 事件 |

**流模式分支**：

```
if StreamMode::Tools | StreamMode::Debug:
    ├── ToolStart     → stream_tx.send
    ├── ToolOutput    → per_tool_writer (截断至 display_limit)
    └── ToolEnd       → stream_tx.send (含 result + raw_result)
else if StreamMode::Custom:
    └── step_progress → run_ctx.emit_custom
```

### 审批流程

```
LLM 返回 tool_calls
    │
    ▼
ActNode 遍历 tool_calls
    │
    ├─ needs_approval(name) == false → 直接执行
    │
    └─ needs_approval(name) == true
         │
         ├─ approval_result == None
         │    └─ 发送 ToolApproval / approval_required_payload
         │    └─ 返回 AgentError::Interrupted(Interrupt { payload })
         │    └─ 调用方（如 Server）收到中断，提示用户确认
         │    └─ 用户确认后，携带 approval_result=Some(true/false) 重新进入
         │
         ├─ approval_result == Some(false)
         │    └─ 写入 "User rejected." 错误结果 → continue
         │
         └─ approval_result == Some(true)
              └─ 继续执行工具
```

> **注意**：当前实现中，如果一批有多个需要审批的工具，只有第一个会触发中断。重新进入后 `approval_result` 对整批工具生效。这是一个已知的设计选择。

## 辅助函数

| 函数 | 用途 |
|------|------|
| `parse_tool_arguments` | 将 `ToolCall.arguments`（JSON 字符串）解析为 `Value`；处理空字符串、无效 JSON、嵌套字符串等情况 |
| `truncate_for_log` | 截断字符串用于日志（按字符截取，追加 `"..."`） |
| `truncate_for_display` | 截断字符串用于 UI 展示（同上，额外处理 `max_chars == 0`） |
| `step_progress_payload` | 构建 `step_progress` 自定义流事件 payload |
| `approval_required_payload` | 构建 `approval_required` 中断 payload |
| `backfill_tool_result_call_ids` | 确保 `ToolResult.call_id` 非空：优先从配对的 `ToolCall.id` 取，否则生成 `call_{uuid6}` |
| `load_tool_output_hints` | 从 `ToolSource.list_tools()` 加载每个工具的 `ToolOutputHint`，用于指导输出标准化 |

## 工具输出标准化

每个工具调用结果都通过 `normalize_tool_output` 进行标准化处理：

```rust
fn normalize_tool_output(
    tool_name: &str,
    args: &Value,
    raw_text: &str,
    is_error: bool,
    output_hint: Option<&ToolOutputHint>,
    config: NormalizationConfig,
) -> NormalizedToolOutput
```

- 根据工具的 `ToolOutputHint` 和 `NormalizationConfig` 决定截断策略
- 维护全局 `used_observation_chars` 预算，跨工具调用累计
- 生成 `ToolResult` 的 `observation_text`（给 LLM）、`display_text`（给 UI）、`raw_content`（原始输出）
- 大输出可持久化到 storage，`ToolResult` 仅保留引用

## 错误处理

| 场景 | 处理方式 |
|------|----------|
| 工具调用失败 | 捕获错误，用 `DEFAULT_EXECUTION_ERROR_TEMPLATE` 格式化后作为 `ToolResult(is_error=true)` 返回给 LLM |
| 审批被拒绝 | 写入 "User rejected." 作为错误结果 |
| 参数解析失败 | `parse_tool_arguments` 记录 warn 日志，回退到空 JSON `{}` |
| call_id 缺失 | `backfill_tool_result_call_ids` 自动生成 fallback id |
| 取消 | 检查 `CancellationToken`，返回 `AgentError::Cancelled` |

## 关键类型

### ToolCall

```rust
pub struct ToolCall {
    pub name: String,        // 工具名称
    pub arguments: String,   // JSON 字符串参数
    pub id: Option<String>,  // 调用 ID（用于匹配 ToolResult）
}
```

### ToolResult

```rust
pub struct ToolResult {
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub content: String,             // 向后兼容
    pub is_error: bool,
    pub raw_content: Option<String>, // 原始输出
    pub observation_text: Option<String>, // 给 LLM 的文本
    pub display_text: Option<String>,    // 给 UI 的文本
    pub storage_ref: Option<ToolStorageRef>, // 大输出的存储引用
    pub strategy: Option<ToolOutputStrategy>,
    pub raw_chars: Option<usize>,
    pub observation_chars: Option<usize>,
    pub truncated: bool,
}
```

### StreamEvent（ActNode 相关变体）

```rust
pub enum StreamEvent<S> {
    ToolStart { call_id, name },          // 工具开始执行
    ToolOutput { call_id, name, content }, // 工具增量输出
    ToolEnd { call_id, name, result, is_error, raw_result }, // 工具执行完成
    ToolApproval { call_id, name, arguments }, // 需要用户审批
    Custom(Value),                         // 自定义事件（step_progress 等）
    // ...其他变体
}
```

## 测试覆盖

当前测试覆盖了辅助函数：

- `truncate_for_log` — 短字符串不变、长字符串截断
- `parse_tool_arguments` — 有效 JSON、空字符串、空白、无效 JSON、嵌套字符串
- `step_progress_payload` — payload 结构验证
- `approval_required_payload` — payload 结构验证
- `act_node_id` — 节点 ID 验证
- `backfill_tool_result_call_ids` — 从 ToolCall.id 填充、生成 fallback id

**未覆盖**：`run`、`run_with_context` 的集成测试（正常调用、审批中断、取消、流事件发射）。
