# workflow 内置工具：错误处理审计

**审计对象**：`agent/tool/tool-workflow`（`WorkflowTool`，agent 可调用的内置 `workflow` 工具，封装 Luft 多 Agent 引擎）
**审计日期**：2026-07-13
**结论**：存在 8 个真实缺陷，其中 2 个 P0（错误结果被当成功、原始 JSON 透传用户/LLM），3 个 P1（`is_error` 无差别、吞错、取消语义丢失）
**置信度**：高（行号已逐一回读源文件验证）

---

## 1. 背景

`workflow` 工具是 Loom 内置的 Luft 引擎入口，支持 4 个 action：

| action | 行为 |
|---|---|
| `run` | 同步执行一段 Lua workflow（含 inline `script` 或文件 `workflow`），等待 `RunDone` 事件 |
| `list-workflows` | 扫描 `.luft/workflows/` 列出可用 .lua 文件 |
| `list-runs` | 扫描 `.runs/` 列出历史 run |
| `run-status` | 读取指定 run 的 `checkpoint.json` + `events.jsonl` |

错误传播链路：

```
WorkflowTool::call(args, ctx)
   │
   ├─ Err(ToolSourceError::...)            → is_error=true 路径 [正确]
   └─ Ok(ToolCallContent::Text(json))     → is_error=false 路径 [Bug 多发]
         │
ActExecutor::execute_one (agent/agent-core/src/agent/react/act_executor.rs:171-194)
   │  match result {
   │    Ok(content)  => normalize(..., is_error=false)   ← 错误信息被当成功
   │    Err(e)       => normalize(..., is_error=true)
   │  }
         │
ToolOutputNormalizer::determine_strategy (tool_output_normalizer.rs:282-292)
   │  TRUNCATABLE_TOOLS.contains("workflow") = false  → 永远 Inline，永不截断
         │
ToolResult::is_error  →  StreamEvent::ToolEnd{is_error, result}
         │
ObserveNode::run  →  Message::Tool{ content: "Tool {name} {result|error}:\n{content}" }
         │
CLI/TUI: format_result_preview (tool_preview.rs:713-728)  →  原始 JSON 透传
```

核心问题：`Err` 路径正确（`is_error=true`、ERROR 红色面板、模板化文本），但**所有"看起来像数据但其实是错误"的结果被硬编码成 `Ok(Text)`**，链路中再无回头机会。

---

## 2. 问题清单（按严重程度）

### P0-1 workflow 运行失败被报告成"成功"

**位置**：`agent/tool/tool-workflow/src/tool.rs:347-364`

```rust
Ok(LuftAgentEvent::RunDone { report, status, total_tokens, .. }) => {
    let text = match &report {
        Value::Null | Value::Bool(_) => {
            let mut obj = json!({
                "status": format!("{:?}", status),   // "Failed"
                "workflow": display_name,
                "tokens": total_tokens,
            });
            if matches!(status, luft_core::contract::event::RunStatus::Failed) {
                obj["error"] = json!("Workflow failed. Use action='run-status' with the latest run_dir to see details.");
            }
            serde_json::to_string_pretty(&obj).unwrap_or_default()
        }
        _ => serde_json::to_string_pretty(&report).unwrap_or_default(),
    };
    return Ok(ToolCallContent::Text(text));    // ← 始终 Ok，is_error 永远 false
}
```

**触发条件**：luft 执行过程中任意 `RunStatus::Failed`（Lua 解析错误、Lua 运行时 panic、agent 子任务全部失败、`LoomAgentBackend` 错误等）。

**实际行为**：
- `act_executor.rs:180` `normalize(..., false)` 标记为成功
- `StreamEvent::ToolEnd { is_error: false, ... }`
- `ObserveNode` 标签是 `"result"` 不是 `"error"`
- CLI 走 `format_result_preview` 通用路径（`tool_preview.rs:713-728`），把 JSON 原样打到 `DONE` 行下方
- LLM 收到 `"Tool workflow result:\n{...json...}"`，**不知道任务失败**

**期望行为**：`RunStatus::Failed` 时返回 `Err(ToolSourceError::ToolError("workflow failed: ..."))`，由 `Err` 路径统一处理（红色 ERROR 面板、模板化错误信息）。

**根因**：`RunDone` 处理器把"完成事件"和"成功结果"混为一谈，没有按 `status` 分支。

**修复方向**（设计见 `docs/design/workflow-tool-errors.md` §3.1）：
```rust
match status {
    RunStatus::Failed => return Err(ToolSourceError::ToolError(format_workflow_error(&report, display_name))),
    RunStatus::Cancelled => return Err(ToolSourceError::ToolError("Workflow cancelled.".into())),
    _ => /* 现有 Ok(Text) 路径 */
}
```

---

### P0-2 子错误：Failed 时 `error` 字段条件缺失

**位置**：`tool.rs:356-358`（上一条 P0-1 的子分支）

```rust
if matches!(status, RunStatus::Failed) {
    obj["error"] = json!("Workflow failed. Use action='run-status' ...");
}
```

`error` 字段仅在 `report` 为 `Value::Null | Value::Bool(_)` 时写入。当 luft 失败但返回了非空 `report`（例如 `report: { error: "...", partial: [...] }`，是常见业务返回），整个 `match` 走 `_` 分支，**`error` 字段连同"failed"提示一起丢失**，JSON 里只剩 `status: "Failed"` 加原始 report。

**根因**：`if Failed` 块被错误地嵌在 `Value::Null | Value::Bool` 分支内，应当提到外层、无条件追加。

**修复方向**：把 `if Failed { obj["error"] = ... }` 移出 match；或者引入 `WorkflowRunOutcome` 枚举统一处理。

---

### P0-3 原始 JSON 无过滤透传到 LLM 与用户

**位置 1**：`agent/agent-core/src/tool_output_normalizer.rs:279, 290-292`

```rust
const TRUNCATABLE_TOOLS: &[&str] = &["web_fetcher", "web_search"];

fn determine_strategy(...) -> ToolOutputStrategy {
    if !TRUNCATABLE_TOOLS.contains(&tool_name) {
        return ToolOutputStrategy::Inline;       // ← workflow 永远走这里
    }
    // ...
}
```

**位置 2**：`apps/cli/src/display/tool_preview.rs:35-60, 713-728`

```rust
pub fn format_preview(tool_name, args_json, result, compact) -> Option<String> {
    match tool_name {
        "read" | "glob" | "grep" | "ls" => { ... }
        "todo_write" | "todo_read" => {}
        "batch" => { ... }
        "write_file" => return None,
        _ => return None,                          // ← workflow 不在白名单
    }
    // ... 后续分发也不覆盖 workflow
}

pub fn format_result_preview(_tool_name, result, _elapsed) -> String {
    // 直接行过滤后输出，无 JSON 解析/格式化
    let lines: Vec<&str> = result.lines().filter(|l| !l.trim().is_empty()).collect();
    // ...
    output.push_str(line); output.push('\n');
    // ...
}
```

**触发条件**：`workflow` 工具返回任意 JSON（成功报告、失败详情、列表）→ 透传两道：
1. `ObserveNode` 把 JSON 直接注入 LLM 的 `Message::Tool` content
2. CLI 走 `format_result_preview` 兜底，把多行 JSON 逐行打到 `DONE` 行下方

**实际行为**（以 `RunStatus::Failed` 场景为例）：
- LLM 看到：`Tool workflow result:\n{\n  "status": "Failed",\n  "workflow": "x.lua",\n  "tokens": 12345,\n  "error": "..."\n}`
- 用户在 CLI 看到：4-5 行未格式化的 JSON

**期望行为**：workflow 输出是结构化报告，应当走"专用渲染"（按 `status` 分支：Completed 显示摘要、Failed 显示红色错误面板、Cancelled 显示提示），并对大报告做截断。

**根因**：
1. `TRUNCATABLE_TOOLS` 是按工具名硬编码的白名单，workflow 不在内 → 不截断
2. `format_preview` 的工具白名单没覆盖 workflow → 没有专用预览格式
3. `format_result_preview` 兜底函数不过滤 JSON → 任何内容直出

**修复方向**（设计见 `docs/design/workflow-tool-errors.md` §3.2）：
- 把 `workflow` 加入 `TRUNCATABLE_TOOLS`
- 在 `format_preview` 中加 `"workflow"` 分支：根据 `status` 字段渲染
- 提供"按 status 着色"的能力（与现有 `error`/`result` 区分）

---

### P1-1 `is_error` 被无差别置为 true

**位置**：`agent/agent-core/src/agent/react/act_executor.rs:184-193`

```rust
Err(e) => {
    warn!(tool = %tc.name, error = %e, "Tool call failed");
    let error_text = DEFAULT_EXECUTION_ERROR_TEMPLATE
        .replace("{tool_name}", &tc.name)
        .replace("{tool_kwargs}", &args.to_string())
        .replace("{error}", &e.to_string());
    let outcome = self.normalize(tc, &args, &error_text, true);  // ← 硬编码 true
    self.emit_end(tc, &outcome, true);
    outcome
}
```

`ToolSourceError` 有 3 个 variant（`NotFound` / `InvalidInput` / `ToolError`），但 `Err` 路径**全部**塞入 `is_error=true` 模板。

**触发条件**：
- workflow 不存在（`InvalidInput`）→ 红色 ERROR
- 参数缺失 `script`/`workflow`（`InvalidInput`）→ 红色 ERROR
- `concurrency` 越界（`InvalidInput`）→ 红色 ERROR
- Lua 解析错误（`ToolError`）→ 红色 ERROR

**实际行为**：用户在 CLI 看到的红色 ERROR 面板和"工具彻底崩溃"无法区分，LLM 也无法区分"参数级反馈"和"执行失败"。

**期望行为**：`InvalidInput` / `NotFound` 是正常的工具级反馈（参数不对、找不到资源），应当用更弱的视觉提示（warning 黄色），让用户和 LLM 知道"工具本身没问题，是你给的不对"。

**根因**：`is_error` 只有 true/false 两态，没有按 `ToolSourceError` variant 区分。

**修复方向**（设计见 `docs/design/workflow-tool-errors.md` §3.3）：在 `ToolSourceError` 上加 `severity` 字段或单独 `is_user_error` 标志，act 层按级别映射到 `is_error` 与 UI 颜色。

---

### P1-2 4 处 `unwrap_or_default()` 静默吞错

**位置**：
- `tool.rs:127-134` `handle_list_workflows` JSON 序列化失败
- `tool.rs:199-205` `handle_list_runs` JSON 序列化失败
- `tool.rs:241-243` `handle_run_status` JSON 序列化失败
- `tool.rs:359, 361` `handle_run` RunDone 报告序列化失败

```rust
Ok(ToolCallContent::Text(
    serde_json::to_string_pretty(&result).unwrap_or_default(),  // ← 失败返回 ""
))
```

**触发条件**：`serde_json` 序列化失败（实际很少发生，但循环引用/递归值理论上可能）。

**实际行为**：返回 `Ok(Text(""))` 或 `Ok(Text("{}"))`，LLM 拿到空观察，下一轮重新执行同一工具 → 浪费 token。

**期望行为**：序列化失败时返回 `Err(ToolSourceError::ToolError(...))`，至少能告诉 LLM "内部序列化失败"。

**根因**：开发者认为"序列化不会失败"做了乐观兜底；实际上应当走 `Err` 路径，至少不静默。

**修复方向**（设计见 `docs/design/workflow-tool-errors.md` §3.4）：统一改为 `unwrap_or_else(|e| format!("{{\"error\":\"internal: serialize failed: {e}\"}}"))` + `is_error=true` 路径。

---

### P1-3 取消（Cancelled）语义丢失

**位置**：`tool.rs:333-341`

```rust
Ok(LuftAgentEvent::RunDone { .. }) => {
    return Ok(ToolCallContent::Text("Workflow cancelled.".to_string()));
}
```

**触发条件**：父 agent 通过 `ToolCallContext::run_cancellation` 取消正在运行的 workflow。

**实际行为**：
- 返回 `Ok`，`is_error=false`
- LLM 收到 `"Workflow cancelled."`，无法区分"取消"和"短小成功输出"
- CLI 不显示任何提示，用户看不到"这个 workflow 是被取消的，不是自然结束"

**期望行为**：取消应当走 `Err` 路径（虽然不是错误，但语义上是"非正常结束"），或新增一个 `ToolSourceError::Cancelled` variant。

**根因**：和 P0-1 同根——`RunDone` 处理器把所有"完成事件"当成功。

**修复方向**：和 P0-1 合并到同一处 `match status` 分支。

---

### P2-1 容错把"无"也包成空成功

**位置 1**：`tool.rs:85-135` `handle_list_workflows`
- `dir.exists()` 为 false → 返回 `{"workflows":[],"count":0,"directory":"..."}`（第 89 行）
- `read_dir` 失败 → 静默返回 `{"workflows":[],"count":0,...}`（第 90 行）
- 单个 .lua 文件 `read_to_string` 失败 → 跳过该文件，继续（`tool.rs:106-108`）

**位置 2**：`tool.rs:137-206` `handle_list_runs`
- `read_dir` 失败 → 静默 `{"runs":[],"count":0}`（第 142 行）
- 单个 checkpoint.json 缺失/解析失败 → 默认 `null`（第 150-153 行）

**位置 3**：`tool.rs:226-230` `handle_run_status` `events.jsonl` 缺失 → 静默 `events:[]`

**触发条件**：rundir 路径配错、文件权限问题、events 文件丢失等。

**实际行为**：用户分不清"目录配错"和"真的没工作流"；run 列表显示"0 runs"但实际跑过。

**期望行为**：在返回 JSON 中加 `warnings: []` 字段列出"目录不存在"、"X 个文件读取失败"；或更激进地，遇到配置问题直接 `Err`。

**修复方向**（设计见 `docs/design/workflow-tool-errors.md` §3.5）：在 `list_*` 系列返回结构中加 `warnings`，把"全部为空"和"有问题"区分开。

---

### P2-2 校验盲区

**位置 1**：`tool.rs:459-468` `run_dir` 参数校验（应在 `parse_run_status_args` 附近）

只检查 `run_dir` 是否 `None`，不检查 `""`（空字符串）。空串 `Path::exists()` 永远 false → 走 "not found" 错误（`tool.rs:211-216`），但路径拼接 `<runs_dir>/<empty>` 会产生 `...//` 双斜杠，行为未定义。

**位置 2**：`tool.rs:280-282` 参数校验
- `script` 和 `workflow` 二者皆无 → 已有 `Err` 校验
- 但 `script`/`workflow` 是非字符串类型（如 `script: 123`）→ 当前用 `args.get("script").and_then(|v| v.as_str())` 静默忽略，落到 `(None, None)` 分支报"必须提供其一"，误导用户（"我提供了啊！"）

**位置 3**：`tool.rs:208-216` 路径穿越
`run_dir` 来自 LLM 任意输入，目前未限制必须落在 `.runs/` 之下。`Path::join` 不会阻止 `../etc/passwd` 等穿越输入；虽然后续 read 的是 checkpoint.json/events.jsonl 受限，但错误信息会泄露完整文件系统路径。

**根因**：入参校验在 tool 层偏弱，依赖类型系统兜底（`as_str()` 失败 → `None`），没有显式类型校验和路径规范化。

**修复方向**（设计见 `docs/design/workflow-tool-errors.md` §3.6）：在 schema 层和 tool 层双重校验；`run_dir` 用 `Path::components()` 校验不含 `..`；`script`/`workflow` 显式校验类型。

---

### P3-1 错误信息三层前缀污染

**位置 1**：`foundation/llm/src/tool.rs:166-177` `ToolSourceError::Display`

```rust
#[error("tool not found: {0}")]            NotFound(String),
#[error("invalid arguments: {0}")]         InvalidInput(String),
#[error("tool execution error: {0}")]      ToolError(String),
```

**位置 2**：`agent/agent-core/src/agent/react/act_executor.rs:186-189` 模板替换

```rust
let error_text = DEFAULT_EXECUTION_ERROR_TEMPLATE
    .replace("{tool_name}", &tc.name)
    .replace("{tool_kwargs}", &args.to_string())
    .replace("{error}", &e.to_string());
```

**实际行为**：用户最终看到的错误：
```
Error executing tool 'workflow' with arguments {...}: invalid arguments: Workflow 'X' not found. Searched: .luft/workflows/, /cwd
```
"not found" 反而被埋在两层前缀后面。

**根因**：`Display` 前缀与模板前缀重复。

**修复方向**：`ToolSourceError::Display` 只输出"语义文本"（如 "Workflow 'X' not found"），不在 variant 上加 "invalid arguments: "；模板层统一加 `Error executing tool '{name}': {error}`。

---

## 3. 正常路径（无误，已审计）

`Err(ToolSourceError::...)` 路径在以下场景正确工作（`is_error=true`，CLI 红色 ERROR 面板）：

| 错误场景 | 位置 | 行为 |
|---|---|---|
| `action='run'` depth ≥ 3 | `tool.rs:252-256` | `ToolError("Workflow nesting depth exceeded (max 3).")` |
| workflow 文件 `read_to_string` 失败 | `tool.rs:273-275` | `ToolError("Failed to read workflow: {e}")` |
| `script` + `workflow` 都缺 | `tool.rs:280-282` | `InvalidInput("Either 'script' or 'workflow' must be provided.")` |
| `LuftBuilder::build()` 失败 | `tool.rs:298-300` | `ToolError("Workflow engine build failed: {e}")` |
| `luft.start_script()` 失败 | `tool.rs:302-305` | `ToolError("Failed to start workflow: {e}")` |
| `done_rx.recv()` 意外关闭 | `tool.rs:366-369` | `ToolError("Workflow event channel closed unexpectedly.")` |
| `action='run-status'` run_dir 不存在 | `tool.rs:211-216` | `InvalidInput("Run directory 'X' not found in ...")` |
| checkpoint.json 读取/解析失败 | `tool.rs:218-224` | `ToolError("Failed to read checkpoint: {e}")` / `Invalid checkpoint JSON: {e}` |
| `action='list-runs'` concurrency 越界 | `tool.rs:31-44` | `InvalidInput(...)`（已有完整单测） |

---

## 4. 测试覆盖空白

| 模块 | 现有测试 | 缺失 |
|---|---|---|
| `handle_run` | 无 | RunStatus::Failed 路径、RunStatus::Cancelled 路径、Lua parse error、`start_script` 失败、`done_rx` 意外关闭 |
| `handle_list_workflows` | 无 | 目录不存在、read_dir 失败、单个 .lua 读取失败、序列化失败 |
| `handle_list_runs` | 无 | 目录不存在、read_dir 失败、checkpoint 缺失/解析失败 |
| `handle_run_status` | 无 | run_dir=""、路径穿越、events.jsonl 缺失 |
| `extract_user_args` | `tool.rs:498-536` | 边界：非 dict 类型的 args |
| `inject_args_globals` | `tool.rs:540-607` | 覆盖良好 |
| `parse_concurrency` | `tool.rs:498-536` | 覆盖良好（边界 + 类型） |
| `resolve_workflow` | `workflow_resolver.rs:55-92` | 4 个路径全覆盖 |

**结论**：核心 4 个 action handler 完全无测，错误矩阵无单测覆盖。

---

## 5. 修复优先级与影响面

| 优先级 | 问题 | 影响面 | 修复成本 |
|---|---|---|---|
| **P0-1** | RunStatus::Failed 当成功 | CLI 误显示 + LLM 误判 | 中（重构 RunDone handler） |
| **P0-2** | Failed 时 error 字段缺失 | LLM 收不到错误信息 | 低（移出 match 分支） |
| **P0-3** | 原始 JSON 透传 | LLM + CLI 双输 | 中（加专用渲染 + 截断） |
| **P1-1** | is_error 无差别 | UI/UX 一致性 | 中（扩 ToolSourceError） |
| **P1-2** | 静默吞错 | 4 处，资源浪费 | 低（统一改 Error 兜底） |
| **P1-3** | Cancelled 语义丢失 | 用户/LLM 混淆 | 低（合并到 P0-1） |
| **P2-1** | 容错成空成功 | 用户分不清状态 | 中（加 warnings 字段） |
| **P2-2** | 校验盲区 | 错误信息误导 + 路径穿越 | 中（加 schema/路径校验） |
| **P3-1** | 前缀污染 | 错误可读性 | 低（去重前缀） |

完整修复设计与分阶段实施见 `docs/design/workflow-tool-errors.md`。
