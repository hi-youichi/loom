# ACP Tool Description 与 Session Update Content 分析

## 1. 概述

本文档分析 `loom-acp` 模块中两个关键问题：

1. **Tool Description 的使用**：ACP 协议的 `ToolCall.title` 字段当前由 `generate_tool_title()` 根据工具名和输入参数硬编码生成，**未使用** Loom `ToolSpec.description` 字段。
2. **Session Update Content 正确性**：`ToolCallUpdate.content` 的构造方式在不同路径下存在差异，需要逐项检查。

---

## 2. 数据流全景

```
┌──────────────────────────────────────────────────────────────────────┐
│  Loom 内部                                                          │
│                                                                      │
│  ToolTrait.spec() → ToolSpec { name, description, input_schema }    │
│       │                                                              │
│       ├─→ LLM 请求 (openai_compat.rs)                                │
│       │    ToolFunctionRequest { name, description, parameters }     │
│       │    ✅ description 被传给 LLM 用于 function calling            │
│       │                                                              │
│       └─→ StreamEvent::ToolCall { call_id, name, arguments }        │
│            ❌ description 未传递，只有 name                           │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  loom-acp (stream_bridge.rs)                                        │
│                                                                      │
│  StreamEvent::ToolCall                                               │
│    → StreamUpdate::ToolCallStarted { tool_call_id, name, input }    │
│    → create_tool_call()                                              │
│       → generate_tool_title(name, input)  ← 硬编码，未用 description │
│       → name_to_tool_kind(name)           ← 名称匹配                 │
│       → ToolCall::new(id, title)                                     │
│           .kind(kind)                                                │
│           .raw_input(input)                                          │
│           .locations(extract_locations(...))                         │
│    → SessionUpdate::ToolCall(tc)                                     │
│                                                                      │
│  StreamEvent::ToolOutput / ToolEnd                                   │
│    → StreamUpdate::ToolCallUpdated { tool_call_id, status, output } │
│    → ToolCallUpdateFields::new()                                     │
│        .status(Completed/Failed/InProgress)                          │
│        .content(vec![s.clone().into()])                              │
│        .raw_output(parse_text_output_to_raw_value(s))                │
│    → SessionUpdate::ToolCallUpdate(...)                              │
│                                                                      │
│  send_history (Message::Tool)                                        │
│    → ToolCallUpdateFields::new()                                     │
│        .status(Completed)                                            │
│        .content(vec![acp_content])  ← 正确转换 ToolCallContent      │
│        .raw_output(tool_call_content_to_raw_output(content))         │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ACP JSON-RPC 输出                                                   │
│                                                                      │
│  session/update notification:                                        │
│  {                                                                   │
│    "sessionId": "...",                                               │
│    "sessionUpdate": {                                                │
│      "toolCall": {                                                   │
│        "toolCallId": "...",                                          │
│        "title": "Reading src/main.rs",  ← 硬编码生成                 │
│        "kind": "read",                                              │
│        "status": "pending",                                         │
│        "rawInput": { "path": "..." }                                │
│      }                                                               │
│    }                                                                 │
│  }                                                                   │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 3. Tool Description 问题详解

### 3.1 ACP 协议中的 ToolCall 结构

来自 `agent-client-protocol-schema v0.11.4` (`tool_call.rs`):

```rust
pub struct ToolCall {
    pub tool_call_id: ToolCallId,
    pub title: String,                    // ← 人类可读标题
    pub kind: ToolKind,                   // ← read/edit/delete/move/search/execute/think/fetch/switch_mode/other
    pub status: ToolCallStatus,           // ← pending/in_progress/completed/failed
    pub content: Vec<ToolCallContent>,    // ← 工具输出内容
    pub locations: Vec<ToolCallLocation>, // ← 文件位置
    pub raw_input: Option<Value>,         // ← 原始输入参数
    pub raw_output: Option<Value>,        // ← 原始输出
    pub meta: Option<Meta>,
}
```

**注意**：ACP `ToolCall` 没有 `description` 字段。`title` 是唯一的描述性文本字段。

### 3.2 Loom ToolSpec 的 description 字段

来自 `loom/src/tool_source/mod.rs:71`:

```rust
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,    // ← 存在但未传递到 ACP 层
    pub input_schema: Value,
    pub output_hint: Option<ToolOutputHint>,
}
```

工具 description 示例（`loom/src/tools/file/read_file.rs:48`）：

```
"Read file content. Path relative to working folder. Optional offset (0-based)
and limit (default 2000). Output in cat -n style with line numbers."
```

### 3.3 当前 title 生成逻辑

`stream_bridge.rs:531` `generate_tool_title()`:

```rust
pub fn generate_tool_title(name: &str, input: Option<&serde_json::Value>) -> String {
    let kind = name_to_tool_kind(name);
    let target = extract_target_from_input(name, input);

    match kind {
        ToolKind::Execute | ToolKind::Other => {
            target.unwrap_or_else(|| name.to_string())
        }
        _ => {
            let verb = match kind {
                ToolKind::Read => "Reading",
                ToolKind::Edit => "Editing",
                // ...
            };
            match target {
                Some(t) => format!("{} {}", verb, t),
                None => format!("{} {}", verb, name),
            }
        }
    }
}
```

**输出示例**：
| 工具名 | 输入 | 生成的 title |
|--------|------|-------------|
| `read_file` | `{"path": "src/main.rs"}` | `"Reading src/main.rs"` |
| `bash` | `{"command": "cargo build"}` | `"cargo build"` |
| `edit_file` | `{"path": "src/lib.rs"}` | `"Editing src/lib.rs"` |
| `grep` | `{"pattern": "fn main"}` | `"Searching fn main"` |

### 3.4 问题：description 未被使用

**当前流程**：
```
ToolSpec.description → 仅用于 LLM function calling prompt
                    ❌ 未传递到 ACP ToolCall.title
```

**根因**：`StreamEvent::ToolCall` 只携带 `name` 和 `arguments`，不携带 `description`：

```rust
// stream-event/src/event.rs:154
ToolCall {
    call_id: Option<String>,
    name: String,
    arguments: Value,
    // ❌ 没有 description 字段
}
```

### 3.5 改进方案

**方案 A：在 StreamEvent 中传递 description（推荐）**

1. 在 `ProtocolEvent::ToolCall` 中增加 `description: Option<String>` 字段
2. 在 ActNode 调用工具时，从 `ToolSpec` 获取 description 并填入
3. `stream_bridge.rs` 的 `create_tool_call` 中优先使用 description 作为 title

```
ToolSpec.description
  → ActNode 发出 StreamEvent::ToolCall { ..., description }
  → stream_bridge create_tool_call()
  → ToolCall.title = description or generate_tool_title() as fallback
```

**方案 B：在 stream_bridge 层注入 ToolRegistry 查找**

1. `SessionNotifier` 持有 `Arc<ToolRegistry>` 或 `Arc<dyn ToolSource>` 引用
2. 收到 `StreamEvent::ToolCall` 时，用 name 查询 `ToolSpec.description`
3. 用 description 作为 ToolCall.title

**方案对比**：

| | 方案 A | 方案 B |
|---|--------|--------|
| 侵入性 | 修改 stream-event/loom 两个 crate | 仅修改 loom-acp |
| 延迟 | 无额外查询 | 每次工具调用需查 registry |
| 准确性 | 由调用方保证 | 依赖 registry 状态一致 |
| 适用场景 | description 需要在所有消费者中使用 | 仅 ACP 需要 |

---

## 4. Session Update Content 检查

### 4.1 三条 Content 构造路径

#### 路径 1：实时流 ToolOutput/ToolEnd（`stream_bridge.rs:254-278`）

```rust
StreamUpdate::ToolCallUpdated { tool_call_id, status, output } => {
    let status = match status.as_str() {
        "running" => ToolCallStatus::InProgress,
        "success" => ToolCallStatus::Completed,
        "failure" => ToolCallStatus::Failed,
        _ => ToolCallStatus::InProgress,
    };
    let mut fields = ToolCallUpdateFields::new().status(status);
    if let Some(ref s) = output {
        fields = fields
            .content(vec![s.clone().into()])        // ← ⚠️ s 是 String
            .raw_output(parse_text_output_to_raw_value(s));
    }
    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(..., fields))
}
```

**问题分析**：
- `output` 是 `String` 类型（来自 `StreamEvent::ToolOutput.content` 和 `StreamEvent::ToolEnd.result`）
- `s.clone().into()` 将 `String` → `ToolCallContent`，走的是 `From<ContentBlock> for ToolCallContent`
- 即 `String` → `ContentBlock::Text(TextContent::new(s))` → `ToolCallContent::Content(Content { content: Text(...) })`
- **结论：✅ 正确** — 纯文本输出被正确包装为 ACP TextContent

#### 路径 2：历史回放 Message::Tool（`stream_bridge.rs:430-468`）

```rust
Message::Tool { tool_call_id, content } => {
    let acp_content = match content {
        ToolCallContent::Text(t) => {
            ToolCallContent::from(ContentBlock::Text(TextContent::new(t.clone())))
        }
        ToolCallContent::Diff { path, old_text, new_text } => {
            ToolCallContent::Diff(Diff::new(path.clone(), new_text.clone())
                .old_text(old_text.clone()))
        }
        ToolCallContent::Terminal { terminal_id } => {
            ToolCallContent::Terminal(Terminal::new(TerminalId::new(terminal_id.clone())))
        }
    };
    let fields = ToolCallUpdateFields::new()
        .status(ToolCallStatus::Completed)
        .content(vec![acp_content])
        .raw_output(tool_call_content_to_raw_output(content));
}
```

**问题分析**：
- `content` 是 Loom 的 `loom::tool_source::ToolCallContent` 枚举
- 逐一匹配 Text / Diff / Terminal → 转换为 ACP 的 `ToolCallContent`
- **结论：✅ 正确** — 三种类型都被正确映射

#### 路径 3：ToolCallChunk（`stream_bridge.rs:279-307`）

```rust
StreamUpdate::ToolCallChunk { tool_call_id, name, arguments_delta } => {
    if let Some(tool_name) = name {
        let tc = create_tool_call(
            tool_call_id,
            tool_name,
            parse_arguments_delta(&arguments_delta).as_ref(),
            None
        );
        SessionUpdate::ToolCall(tc)   // ← 新的 ToolCall，没有 content
    } else {
        return None;   // ← 后续 chunk 被忽略
    }
}
```

**问题分析**：
- 第一个 chunk 创建新的 `ToolCall`（status: Pending），无 content — 合理
- 后续 chunk 被忽略（ACP 不支持增量更新）
- **结论：✅ 正确但有限制** — 流式工具参数只发第一个 chunk，完整参数在后续 `ToolCall` 事件中发送

### 4.2 raw_output 正确性

#### 实时流路径

```rust
fn parse_text_output_to_raw_value(output: &str) -> serde_json::Value {
    serde_json::json!(output)   // 简单包装为 JSON string
}
```

**问题**：无论 output 是什么内容，都当作 JSON string。如果 output 本身是 JSON 格式（如 `{"status": "ok"}`），它会被双重转义。

#### 历史回放路径

```rust
fn tool_call_content_to_raw_output(content: &loom::tool_source::ToolCallContent) -> Value {
    match content {
        ToolCallContent::Text(text) => serde_json::json!(text),
        ToolCallContent::Diff { path, old_text, new_text } => serde_json::json!({
            "type": "diff",
            "path": path,
            "old_text": old_text,
            "new_text": new_text,
        }),
        ToolCallContent::Terminal { terminal_id } => serde_json::json!({
            "type": "terminal",
            "terminal_id": terminal_id,
        }),
    }
}
```

**结论：✅ 正确** — 三种类型有结构化的 raw_output 表示

### 4.3 Content 差异总结

| 场景 | content 类型 | raw_output | 是否正确 |
|------|-------------|------------|---------|
| 实时 ToolOutput | `ToolCallContent::Content(Text(...))` | `json!(string)` | ✅ |
| 实时 ToolEnd (success) | `ToolCallContent::Content(Text(...))` | `json!(string)` | ✅ |
| 实时 ToolEnd (failure) | `ToolCallContent::Content(Text(...))` | `json!(string)` | ✅ |
| 历史 Text | `ToolCallContent::Content(Text(...))` | `json!(string)` | ✅ |
| 历史 Diff | `ToolCallContent::Diff(...)` | `json!({"type":"diff",...})` | ✅ |
| 历史 Terminal | `ToolCallContent::Terminal(...)` | `json!({"type":"terminal",...})` | ✅ |
| 实时 Diff | ❌ 不支持 — Loom StreamEvent 只传 String | N/A | ⚠️ |
| 实时 Terminal | ❌ 不支持 | N/A | ⚠️ |

### 4.4 实时流 Diff/Terminal 内容丢失问题

**根因**：`StreamEvent::ToolOutput` 和 `StreamEvent::ToolEnd` 的 content/result 都是 `String`：

```rust
// stream-event/src/event.rs
ToolOutput { call_id, name, content: String }
ToolEnd { call_id, name, result: String, is_error: bool }
```

Loom 内部的 `ToolCallContent::Diff` 和 `ToolCallContent::Terminal` 在序列化为 StreamEvent 时被 `to_string()` 丢失了结构化信息。

**影响**：
- ACP 客户端在**实时流**中收到的 `ToolCallUpdate.content` 永远是 `Text` 类型
- 只有**历史回放**（`send_history`）才能正确恢复 `Diff` 和 `Terminal` 类型

---

## 5. name_to_tool_kind 映射表

`stream_bridge.rs:321` 当前的名称匹配逻辑：

| 工具名模式 | ToolKind | 匹配的 Loom 工具 |
|-----------|----------|-----------------|
| `*read*` | Read | `read_file` |
| `*write*` / `*edit*` | Edit | `write_file`, `edit_file`, `multiedit`, `apply_patch` |
| `*delete*` / `*remove*` | Delete | `delete_file` |
| `*move*` / `*rename*` | Move | `move_file` |
| `*search*` / `*grep*` / `*glob*` | Search | `grep`, `glob`, `exa_codesearch`, `exa_websearch` |
| `*run*` / `*bash*` / `*command*` / `*exec*` / `*shell*` | Execute | `bash`, `powershell` |
| `*think*` / `*reason*` | Think | （无对应内置工具） |
| `*fetch*` | Fetch | `web_fetcher` |
| `*switch_mode*` / `*set_mode*` | SwitchMode | （ACP 内部使用） |
| 其他 | Other | `ls`, `create_dir`, `todo_read`, `todo_write`, `remember`, `recall`, `search_memories`, `list_memories`, `invoke_agent`, `skill`, `lsp`, `batch`, `twitter_search` |

**注意**：`ls` 和 `create_dir` 被归类为 `Other` 而非 `Read`/`Edit`。可以考虑优化。

---

## 6. 改进建议

### 6.1 高优先级：在 ACP ToolCall.title 中使用 ToolSpec description

修改 `stream_bridge.rs` 中的 `create_tool_call`：

```rust
// 方案 A（推荐）：修改 StreamEvent 携带 description
// 1. stream-event: ProtocolEvent::ToolCall 增加 description: Option<String>
// 2. loom ActNode: 从 ToolSpec 获取 description 填入
// 3. stream_bridge: 优先使用 description

pub fn create_tool_call(
    tool_call_id: &str,
    name: &str,
    input: Option<&Value>,
    kind_override: Option<&str>,
    description: Option<&str>,   // ← 新增
) -> ToolCall {
    let id = ToolCallId::new(tool_call_id);
    let title = description
        .map(|d| d.to_string())
        .unwrap_or_else(|| generate_tool_title(name, input));
    // ...
}
```

### 6.2 中优先级：实时流中保留结构化 ToolCallContent

在 `StreamEvent::ToolOutput` 和 `StreamEvent::ToolEnd` 中使用 `ToolCallContent` 替代 `String`：

```rust
// stream-event: 修改 content 字段类型
ToolOutput {
    call_id: Option<String>,
    name: String,
    content: ToolCallContent,   // ← 替代 String
}
ToolEnd {
    call_id: Option<String>,
    name: String,
    result: ToolCallContent,    // ← 替代 String
    is_error: bool,
}
```

### 6.3 低优先级：优化 name_to_tool_kind 映射

```rust
// 增加 ls, create_dir, list_memories 等工具的精确映射
} else if n.contains("ls") || n.contains("list") {
    ToolKind::Read
} else if n.contains("create_dir") || n.contains("mkdir") {
    ToolKind::Edit
}
```

---

## 7. 方案执行后的效果对比

### 7.1 ACP JSON-RPC 输出 Before / After

方案 A 实施后，`ToolCall.title` 将从 `ToolSpec.description` 取值（而非 `generate_tool_title` 硬编码）。

#### Before（当前）

```jsonc
// session/update notification
{
  "sessionId": "session-123",
  "sessionUpdate": {
    "toolCall": {
      "toolCallId": "call-001",
      "title": "Reading src/main.rs",       // ← generate_tool_title 硬编码
      "kind": "read",
      "status": "pending",
      "rawInput": { "path": "src/main.rs" }
    }
  }
}
```

#### After（方案 A 实施后）

```jsonc
{
  "sessionId": "session-123",
  "sessionUpdate": {
    "toolCall": {
      "toolCallId": "call-001",
      "title": "Read file content. Path relative to working folder. Optional offset (0-based) and limit (default 2000). Output in cat -n style with line numbers.",
                                            // ← 来自 ToolSpec.description
      "kind": "read",
      "status": "pending",
      "rawInput": { "path": "src/main.rs" }
    }
  }
}
```

### 7.2 逐工具 Before / After 对照表

| 工具名 | Before (`title`) | After (`title`) | 优劣分析 |
|--------|-----------------|-----------------|---------|
| `read_file` | `"Reading src/main.rs"` | `"Read file content. Path relative to working folder. Optional offset (0-based) and limit (default 2000). Output in cat -n style with line numbers."` | Before 更简洁；After 更具描述性但冗长 |
| `edit_file` | `"Editing src/lib.rs"` | `"Performs exact string replacements in files.\n\nUsage:\n- You must use your read tool at least once..."` | ❌ After 太长，暴露了 LLM prompt 细节 |
| `write_file` | `"Editing output.txt"` | `"Write text content to a file. Creates parent directories if needed. Path is relative to the working folder. Overwrites if file exists."` | After 更清晰 |
| `delete_file` | `"Deleting temp.log"` | `"Delete a file or an empty directory. Path must be under the working folder. For non-empty directories, use recursive option with caution."` | After 更清晰 |
| `move_file` | `"Moving old.rs to new.rs"` | `"Move or rename a file or directory. Both source and target must be under the working folder."` | Before 更具操作性（显示了源→目标） |
| `grep` | `"Searching fn main"` | `"Search file contents under the working folder using a regular expression. Supports full regex syntax..."` | Before 显示了搜索目标，更实用 |
| `glob` | `"Searching **/*.rs"` | `"List files under the working folder that match a glob pattern..."` | Before 显示了 glob 模式，更实用 |
| `ls` | `"ls"` (Other) | `"List files and directories as a tree. Path is relative to the working folder; omit it to list the working folder root."` | After 远优于 `"ls"` |
| `bash` | `"cargo build"` | `"Execute shell commands. Working directory defaults to the working folder."` | ❌ Before 显示了实际命令，远优于 After |
| `apply_patch` | `"Editing src/main.rs"` | `"Apply a multi-file patch. Use *** Begin Patch / *** End Patch..."` | Before 太泛；After 技术性太强 |
| `multiedit` | `"Editing src/app.ts"` | `"Apply multiple find-and-replace edits to a single file in one call. Prefer over edit when making several changes to the same file."` | After 更好 |
| `create_dir` | `"create_dir"` (Other) | `"Create a directory; parent directories are created if needed. Path is relative to the working folder."` | After 远优于裸名 |
| `todo_write` | `"todo_write"` (Other) | `"Write or replace the todo list."` | After 更好 |
| `todo_read` | `"todo_read"` (Other) | `"Read the current todo list."` | After 更好 |
| `web_fetcher` | `"Fetching https://..."` | `"Fetch or send content to a URL. Use this tool to retrieve web pages (GET), call APIs with a body (POST)..."` | Before 显示 URL，更实用 |
| `exa_websearch` | `"Searching AI news 2025"` | `"Search the web using Exa. Use for current events and up-to-date information..."` | Before 显示搜索词，更实用 |
| `powershell` | `"Get-Process"` | `"Executes a PowerShell command on Windows (WMI, Registry, .NET, COM)..."` | Before 显示命令，远优于 After |

### 7.3 结论：description 不适合直接作为 title

对比分析表明：

- **`generate_tool_title` 的优势**：结合了操作动词 + 具体目标（文件路径、命令、搜索词），简洁且具操作性
- **`ToolSpec.description` 的劣势**：
  - 部分工具的 description 是给 LLM 的使用说明（如 `edit_file` 含 Usage 指引），不适合展示给用户
  - 缺少具体操作目标的上下文（如 "Reading src/main.rs" 比 "Read file content..." 更直观）
  - 部分工具（`bash`, `powershell`）的 description 是通用说明，而 title 应展示具体命令

### 7.4 推荐改进：混合策略

```rust
pub fn generate_tool_title(
    name: &str,
    input: Option<&serde_json::Value>,
    description: Option<&str>,   // ← 新增
) -> String {
    let kind = name_to_tool_kind(name);
    let target = extract_target_from_input(name, input);

    match kind {
        // 有具体目标的：保持原有逻辑（动词 + 目标）
        ToolKind::Read | ToolKind::Edit | ToolKind::Delete | ToolKind::Move
        | ToolKind::Search | ToolKind::Fetch => {
            // 保持现有 generate_tool_title 逻辑
            // ...
        }
        // 执行类：展示具体命令
        ToolKind::Execute => {
            target.unwrap_or_else(|| name.to_string())
        }
        // 其他：使用 description 或回退到 name
        ToolKind::Other | ToolKind::Think | ToolKind::SwitchMode => {
            description
                .map(|d| {
                    // 截取第一句话作为 title
                    d.split('.')
                        .next()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| name.to_string())
                })
                .unwrap_or_else(|| name.to_string())
        }
    }
}
```

**效果**：

| 工具 | 改进后 title | 说明 |
|------|-------------|------|
| `read_file` | `"Reading src/main.rs"` | 保持不变 |
| `bash` | `"cargo build"` | 保持不变 |
| `ls` | `"List files and directories as a tree"` | ✅ 使用 description 首句 |
| `create_dir` | `"Create a directory; parent directories are created if needed"` | ✅ 使用 description 首句 |
| `todo_write` | `"Write or replace the todo list"` | ✅ 使用 description 首句 |
| `todo_read` | `"Read the current todo list"` | ✅ 使用 description 首句 |
| `edit_file` | `"Editing src/lib.rs"` | ✅ 保持具体目标，不用长 description |
| `invoke_agent` | `"Delegate work to sub-agents"` | ✅ 使用 description 首句 |

这样 **有具体操作目标的工具保持动词+目标的格式**，**没有具体目标的 Other 类工具使用 description 首句**，兼顾了信息量和简洁性。

---

## 9. 相关文件索引

| 文件 | 作用 |
|------|------|
| `loom-acp/src/stream_bridge.rs` | Loom 事件 → ACP SessionUpdate 转换核心 |
| `loom-acp/src/agent.rs` | ACP Agent 实现，处理 prompt/load_session |
| `loom/src/tool_source/mod.rs:71` | `ToolSpec` 定义（含 description） |
| `loom/src/tools/trait.rs` | `Tool` trait 定义（`spec()` 返回 ToolSpec） |
| `loom/src/llm/openai_compat.rs:94` | `ToolFunctionRequest`（description 传给 LLM） |
| `stream-event/src/event.rs:154` | `ProtocolEvent::ToolCall`（无 description） |
| `web/src/types/toolConfig.ts` | 前端 ToolKind 图标/标签配置 |
| `web/src/types/chat.ts:9` | 前端 `ToolType` 类型定义 |

---

## 10. ACP ToolKind 与 Loom 工具映射参考

ACP 协议定义的 ToolKind（`agent-client-protocol-schema-0.11.4`）：

| ToolKind | 描述 | 对应 Loom 工具 | 当前映射方式 |
|----------|------|---------------|-------------|
| `read` | Reading files or data | `read_file` | `*read*` |
| `edit` | Modifying files or content | `write_file`, `edit_file`, `multiedit`, `apply_patch` | `*write*` / `*edit*` |
| `delete` | Removing files or data | `delete_file` | `*delete*` / `*remove*` |
| `move` | Moving or renaming files | `move_file` | `*move*` / `*rename*` |
| `search` | Searching for information | `grep`, `glob`, `exa_*`, `search_memories` | `*search*` / `*grep*` / `*glob*` |
| `execute` | Running commands or code | `bash`, `powershell` | `*run*` / `*bash*` / `*command*` / `*exec*` / `*shell*` |
| `think` | Internal reasoning or planning | （无） | `*think*` / `*reason*` |
| `fetch` | Retrieving external data | `web_fetcher` | `*fetch*` |
| `switch_mode` | Switching session mode | ACP 内部 | `*switch_mode*` / `*set_mode*` |
| `other` | Other tool types (default) | `ls`, `create_dir`, `todo_*`, `remember`, `recall`, `invoke_agent`, `skill`, `lsp`, `batch` | 默认 |
