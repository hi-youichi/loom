# workflow 内置工具：错误处理修复设计

> 配套审计：`docs/audit/workflow-tool-errors.md`
> 在不破坏现有 tool 协议的前提下，分阶段把 9 个 P0-P3 问题修完，最大化 ROI。

**创建时间**：2026-07-13
**状态**：方案设计，待评审
**前置依赖**：审计报告
**目标版本**：tool-workflow `0.x` → `0.x+1`

---

## 1. 设计目标与约束

### 1.1 目标

1. **正确性**：`RunStatus::Failed` 走 `is_error=true` 路径，CLI 红色 ERROR 面板，LLM 收到 `error` 标签
2. **可观察性**：用户/LLM 能区分"成功 / 失败 / 取消 / 工具参数错误 / 工具崩溃"5 种状态
3. **可测性**：每个 action handler 有完整错误矩阵单测
4. **可演进性**：不影响现有 OK 路径的接口契约（`ToolCallContent::Text` 返回值结构只增字段不删字段）

### 1.2 约束

- 不动 `Tool` trait 签名（`tool.rs` 对外 `spec()` 不变）
- 不动 `ToolSourceError` 的现有 variant（向后兼容，仅新增）
- 不破坏现有 `concurrency` 边界测试（`tool.rs:503-536`）
- 对其他内置 tool（`web_fetcher`、`web_search` 等）的现有行为零影响
- 改动必须可分批提交，便于 review 和 revert

### 1.3 非目标

- 不重构 `act_executor` 整体错误处理架构（仅最小改动）
- 不引入新的 `tool-output` 协议字段
- 不修改 Luft 引擎自身的事件契约

---

## 2. 总体方案

分 4 个阶段，按"修复密度 × 风险"排序：

| 阶段 | 范围 | 风险 | ROI |
|---|---|---|---|
| **Phase 1** | P0-1 + P0-2 + P1-3：重构 `RunDone` 处理器 | 中 | 极高 |
| **Phase 2** | P0-3：JSON 透传 → 专用渲染 + 截断 | 中 | 高 |
| **Phase 3** | P1-1 + P1-2：`is_error` 分级 + 吞错修复 | 中 | 中 |
| **Phase 4** | P2-1 + P2-2 + P3-1：warnings、校验、可读性 | 低 | 中 |

每阶段独立可合并、独立可 revert。Phase 1 是当务之急，Phase 4 可延后到下个迭代。

---

## 3. 详细设计

### 3.1 Phase 1：重构 `RunDone` 处理器（P0-1 + P0-2 + P1-3）

**核心改动**：`tool.rs:347-364` 的 `RunDone` match arm 改为按 `status` 分支显式处理。

**当前代码**（`tool.rs:347-364`）：

```rust
Ok(LuftAgentEvent::RunDone { report, status, total_tokens, .. }) => {
    let text = match &report {
        Value::Null | Value::Bool(_) => {
            let mut obj = json!({ ... });
            if matches!(status, RunStatus::Failed) {  // ← 仅在 Null|Bool 时执行
                obj["error"] = json!("Workflow failed. ...");
            }
            serde_json::to_string_pretty(&obj).unwrap_or_default()
        }
        _ => serde_json::to_string_pretty(&report).unwrap_or_default(),
    };
    return Ok(ToolCallContent::Text(text));  // ← 一律 Ok
}
```

**目标代码**：

```rust
Ok(LuftAgentEvent::RunDone { report, status, total_tokens, .. }) => {
    match status {
        RunStatus::Failed => {
            // 1. 抽取 report 中的 error 详情（不依赖 report 类型）
            let detail = extract_error_detail(&report);
            return Err(ToolSourceError::ToolError(format!(
                "Workflow '{display_name}' failed after {total_tokens} tokens: {detail}. \
                 Use action='run-status' with the latest run_dir to see full events."
            )));
        }
        RunStatus::Cancelled => {
            return Err(ToolSourceError::ToolError(format!(
                "Workflow '{display_name}' was cancelled."
            )));
        }
        RunStatus::Completed | _ => {
            // 2. 成功路径保持现有 Ok(Text) 不变
            let mut obj = match &report {
                Value::Null | Value::Bool(_) => json!({
                    "status": format!("{status:?}"),
                    "workflow": display_name,
                    "tokens": total_tokens,
                }),
                _ => json!({
                    "status": format!("{status:?}"),
                    "workflow": display_name,
                    "tokens": total_tokens,
                    "report": report,
                }),
            };
            // 3. 序列化失败兜底（P1-2 一并修）
            return Ok(ToolCallContent::Text(
                serde_json::to_string_pretty(&obj).unwrap_or_else(|e| {
                    format!("{{\"error\":\"internal: failed to serialize workflow result: {e}\"}}")
                })
            ));
        }
    }
}
```

**辅助函数** `extract_error_detail`：

```rust
fn extract_error_detail(report: &Value) -> String {
    match report {
        Value::String(s) if !s.is_empty() => s.clone(),
        Value::Object(map) => {
            map.get("error").and_then(|v| v.as_str())
                .or_else(|| map.get("message").and_then(|v| v.as_str()))
                .map(String::from)
                .unwrap_or_else(|| report.to_string())
        }
        Value::Null | Value::Bool(_) => "(no detail provided)".to_string(),
        _ => report.to_string(),
    }
}
```

**对 `Cancelled` 的语义统一**：在 `tool.rs:333-341` 的 `cancelled` 分支同样改为 `Err(ToolSourceError::ToolError("Workflow cancelled.".into()))`，与 `RunStatus::Cancelled` 合并到同一处。

**单测覆盖**（新增 `tool.rs:608+`）：

```rust
#[tokio::test]
async fn run_done_failed_with_string_report_returns_error() {
    // mock LuftBuilder，返回 RunDone { status: Failed, report: Value::String("OOM at agent X") }
    let result = workflow_tool.call(&json!({"script": "..."}), None).await;
    assert!(matches!(result, Err(ToolSourceError::ToolError(msg)) if msg.contains("OOM at agent X")));
}

#[tokio::test]
async fn run_done_failed_with_object_report_includes_error_field() {
    // report = { "error": "agent X panicked", "partial": [...] }
    let result = workflow_tool.call(&json!({"script": "..."}), None).await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("agent X panicked"));
}

#[tokio::test]
async fn run_done_cancelled_returns_error() {
    // status: Cancelled
    let result = workflow_tool.call(&json!({"script": "..."}), None).await;
    assert!(matches!(result, Err(ToolSourceError::ToolError(_))));
}

#[tokio::test]
async fn run_done_completed_with_null_report_returns_ok() {
    // status: Completed, report: Null
    let result = workflow_tool.call(&json!({"script": "..."}), None).await;
    let content = result.unwrap();
    let text = match content { ToolCallContent::Text(t) => t, _ => panic!() };
    assert!(text.contains("Completed"));
}
```

**风险与回滚**：
- 风险：现有依赖"failed 走 Ok 路径"的代码（如观察者）会断。需在 audit 时搜全。
- 回滚：`git revert` 单 commit 即可。

---

### 3.2 Phase 2：JSON 透传修复（P0-3）

**核心改动**：
1. `tool_output_normalizer.rs:279` 把 `"workflow"` 加入 `TRUNCATABLE_TOOLS`
2. `tool_preview.rs:35-60` 在 `format_preview` 加 `"workflow"` 分支
3. 新增 `format_workflow_preview` 函数，按 `status` 着色

**位置 1**：`agent/agent-core/src/tool_output_normalizer.rs:279`

```rust
const TRUNCATABLE_TOOLS: &[&str] = &[
    "web_fetcher",
    "web_search",
    "workflow",  // ← 新增
];
```

**位置 2**：`apps/cli/src/display/tool_preview.rs:25-60` `format_preview`

```rust
pub fn format_preview(
    tool_name: &str,
    args_json: &str,
    result: &str,
    compact: bool,
) -> Option<String> {
    if compact { return None; }
    
    match tool_name {
        "read" | "glob" | "grep" | "ls" => { /* 现有 */ }
        "todo_write" | "todo_read" => {}
        "batch" => { /* 现有 */ }
        "write_file" => return None,
        "workflow" => { /* 现有 None, 现有 code path 也处理 */ }
        _ => return None,
    }
    
    match tool_name {
        // ... 现有分支 ...
        "workflow" => Some(format_workflow_preview(result)),
        _ => None,
    }
}
```

**新增 `format_workflow_preview`**（追加在 `tool_preview.rs:728` 之后）：

```rust
pub fn format_workflow_preview(result: &str) -> String {
    // 尝试解析 status 字段以决定渲染
    let parsed: Option<Value> = serde_json::from_str(result).ok();
    let status = parsed.as_ref()
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");
    
    let color = if status == "Completed" {
        Color::Green
    } else if status == "Failed" {
        Color::Red
    } else if status == "Cancelled" {
        Color::Yellow
    } else {
        Color::Reset
    };
    
    let summary = match status {
        "Completed" => {
            let tokens = parsed.as_ref()
                .and_then(|v| v.get("tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!("Workflow completed ({} tokens).", tokens)
        }
        "Failed" => {
            let err = parsed.as_ref()
                .and_then(|v| v.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("see full output");
            format!("Workflow FAILED: {}", err)
        }
        "Cancelled" => "Workflow cancelled.".to_string(),
        _ => "Workflow status: see full output.".to_string(),
    };
    
    if color_enabled() {
        format!("\x1b[{}m{}\x1b[0m", color_code(color), summary)
    } else {
        summary
    }
}
```

**新增 CLI 截断上限**（`tool_output_normalizer.rs`）：

```rust
// 现有 config.inline_limit 沿用；为 workflow 设小一点的硬上限
const WORKFLOW_INLINE_LIMIT: usize = 2000;  // chars
```

**单测**：

```rust
#[test]
fn format_workflow_preview_completed_green() {
    let result = json!({"status": "Completed", "tokens": 1234, "workflow": "x.lua"});
    let preview = format_workflow_preview(&result.to_string());
    assert!(preview.contains("completed"));
    assert!(preview.contains("1234"));
}

#[test]
fn format_workflow_preview_failed_red() {
    let result = json!({"status": "Failed", "error": "agent OOM"});
    let preview = format_workflow_preview(&result.to_string());
    assert!(preview.contains("FAILED"));
    assert!(preview.contains("OOM"));
}

#[test]
fn format_workflow_preview_malformed_falls_back() {
    let preview = format_workflow_preview("not json at all");
    assert!(!preview.is_empty());
}
```

**风险与回滚**：纯增量改动，关闭 `format_workflow_preview` 即可回退。

---

### 3.3 Phase 3：`is_error` 分级 + 吞错修复（P1-1 + P1-2）

#### 3.3.1 P1-1：扩 `ToolSourceError` 加 `severity`

**改动**：`foundation/llm/src/tool.rs:166-177`：

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolSourceError {
    #[error("tool not found: {0}")]
    NotFound(String),
    
    #[error("invalid arguments: {0}")]
    InvalidInput(String),
    
    #[error("tool execution error: {0}")]
    ToolError(String),
    
    /// 内部严重错误（如序列化失败、引擎崩溃）—— UI 应当红色 ERROR + 隐藏细节
    #[error("internal error: {0}")]
    Internal(String),
}

impl ToolSourceError {
    /// 是否应当触发 ERROR 红色面板（true）还是 warning（false）
    pub fn is_user_error(&self) -> bool {
        match self {
            // 工具找不到、参数不对 → 用户级反馈，UI 用 warning
            ToolSourceError::NotFound(_) | ToolSourceError::InvalidInput(_) => false,
            // 工具执行错误、内部错误 → 真正的错误
            ToolSourceError::ToolError(_) | ToolSourceError::Internal(_) => true,
        }
    }
}
```

**对应 act 层**（`act_executor.rs:184-193`）：

```rust
Err(e) => {
    warn!(tool = %tc.name, error = %e, is_user_error = e.is_user_error(), "Tool call failed");
    let is_error = e.is_user_error();
    let error_text = DEFAULT_EXECUTION_ERROR_TEMPLATE
        .replace("{tool_name}", &tc.name)
        .replace("{tool_kwargs}", &args.to_string())
        .replace("{error}", &e.to_string());
    let outcome = self.normalize(tc, &args, &error_text, is_error);  // ← 改
    self.emit_end(tc, &outcome, is_error);
    outcome
}
```

**UI 层对应**（`event_handler.rs:288`）：把 `is_error` 改为更细的 `severity: ErrorSeverity::{Error, Warning}`，Warning 走黄色面板。

> **注意**：这会影响所有内置 tool 的 UI 表现（如 `web_fetcher` 找不到 URL 也变黄色），需要在 changelog 强调。

#### 3.3.2 P1-2：吞错修复

**改动**（4 处统一改）：

```rust
// 旧
serde_json::to_string_pretty(&result).unwrap_or_default()

// 新
serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
    tracing::error!(error = %e, "workflow tool: failed to serialize result");
    let err = json!({
        "internal_error": "failed to serialize workflow result",
        "detail": e.to_string(),
    });
    serde_json::to_string_pretty(&err).unwrap_or_else(|_| r#"{"internal_error":"serialization cascade failure"}"#.to_string())
})
```

**或更激进**：直接 `return Err(ToolSourceError::Internal("failed to serialize: {e}"))`，但这会改变返回路径（`is_error=true` 触发），与现有测试假设冲突；建议先用返回 JSON 兜底，下个迭代再统一为 `Err`。

**单测**：

```rust
#[test]
fn unwrap_or_default_returns_error_json_on_serialize_failure() {
    // 用循环引用 Value 触发 to_string_pretty 失败
    // （serde_json 实际不支持循环检测，会直接 panic；改用 mock 或跳过）
    // 改为：在 json_to_lua 模块中注入 mock 失败
}
```

实际工程上 `to_string_pretty` 失败极少发生（serde_json 对合法 `Value` 不会失败），本测试可以**集成到 observability 验证**：在 panic/oom 场景下看 trace 日志是否记录。

---

### 3.4 Phase 4：warnings 字段 + 校验 + 可读性（P2-1 + P2-2 + P3-1）

#### 3.4.1 P2-1：`list_*` 加 `warnings` 字段

**改动**（`tool.rs:127-134, 199-205`）：

```rust
// list-workflows
Ok(ToolCallContent::Text(
    serde_json::to_string_pretty(&json!({
        "workflows": workflows,
        "directory": dir.display().to_string(),
        "count": workflows.len(),
        "warnings": warnings,  // 新增
    })).unwrap_or_default(),
))

// 其中 warnings 在循环中累积：
let mut warnings = Vec::new();
if !dir.exists() {
    warnings.push(format!("workflows directory does not exist: {}", dir.display()));
} else if let Err(e) = std::fs::read_dir(&dir) {
    warnings.push(format!("failed to read workflows directory: {e}"));
}
// 在单个 .lua 读取失败时：
if let Err(e) = std::fs::read_to_string(&path) {
    warnings.push(format!("failed to read {}: {e}", path.display()));
}
```

**对应 `format_workflow_preview`** 扩展：检测 `warnings` 非空时附加 `⚠ {n} warnings` 标记。

#### 3.4.2 P2-2：路径与类型校验

**改动**（`tool.rs:208-216` `run-status`）：

```rust
async fn handle_run_status(&self, run_dir: &str) -> Result<ToolCallContent, ToolSourceError> {
    // 新增：拒绝空串与路径穿越
    if run_dir.is_empty() {
        return Err(ToolSourceError::InvalidInput(
            "run_dir must not be empty".to_string(),
        ));
    }
    let path = std::path::Path::new(run_dir);
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(ToolSourceError::InvalidInput(format!(
            "run_dir must not contain '..': {run_dir}"
        )));
    }
    
    let full = self.runs_dir().join(run_dir);
    if !full.exists() {
        return Err(ToolSourceError::InvalidInput(format!(
            "Run directory '{run_dir}' not found in {}", self.runs_dir().display()
        )));
    }
    // 限制在 runs_dir 之下（防 symlink 逃逸）
    if !full.starts_with(&self.runs_dir()) {
        return Err(ToolSourceError::InvalidInput(format!(
            "run_dir escapes runs directory: {run_dir}"
        )));
    }
    // ... 现有逻辑 ...
}
```

**`script`/`workflow` 类型校验**（`tool.rs:258-259`）：

```rust
let script = match args.get("script") {
    Some(v) if v.is_string() => v.as_str(),
    Some(_) => return Err(ToolSourceError::InvalidInput(
        "'script' must be a string".to_string(),
    )),
    None => None,
};
let workflow = match args.get("workflow") {
    Some(v) if v.is_string() => v.as_str(),
    Some(_) => return Err(ToolSourceError::InvalidInput(
        "'workflow' must be a string".to_string(),
    )),
    None => None,
};
```

#### 3.4.3 P3-1：去重错误前缀

**改动**：`foundation/llm/src/tool.rs:166-177` 去掉 `Display` 前缀：

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolSourceError {
    #[error("{0}")]                            // ← 去前缀
    NotFound(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    ToolError(String),
    #[error("{0}")]
    Internal(String),
}
```

调用方负责加前缀（如 `resolve_workflow` 返回 `"Workflow 'X' not found. Searched: ..."`）。

`DEFAULT_EXECUTION_ERROR_TEMPLATE` 模板（`act_executor.rs:186-189`）加唯一前缀：

```rust
const DEFAULT_EXECUTION_ERROR_TEMPLATE: &str = "Tool '{tool_name}' failed: {error}\n  arguments: {tool_kwargs}";
```

**最终用户可见错误**：
```
Tool 'workflow' failed: Workflow 'X' not found. Searched: .luft/workflows/, /cwd
  arguments: {"workflow":"X"}
```

单层前缀，语义清晰。

---

## 4. 测试矩阵

每个阶段必须新增单测（放在 `tool.rs` 和 `tool_output_normalizer.rs` 内 `#[cfg(test)] mod tests`）：

| 阶段 | 测试场景 | 期望 |
|---|---|---|
| **Phase 1** | RunDone Failed + report=String("OOM") | `Err` 含 "OOM" |
| | RunDone Failed + report=Object{error:"..."} | `Err` 含 report.error |
| | RunDone Failed + report=Null | `Err` 含 "(no detail provided)" |
| | RunDone Cancelled | `Err("Workflow 'X' was cancelled.")` |
| | RunDone Completed + report=Null | `Ok(Text)` 含 "Completed" |
| | RunDone Completed + report=Object | `Ok(Text)` 含 "report" 字段 |
| | 序列化失败 | `Ok(Text)` 含 "internal_error" |
| **Phase 2** | `format_workflow_preview` 4 种 status | 颜色 + 文案正确 |
| | `format_workflow_preview` 非 JSON 输入 | 兜底文案 |
| | `TRUNCATABLE_TOOLS` 含 "workflow" | 截断策略生效 |
| **Phase 3** | `ToolSourceError::NotFound` | `is_user_error() = false` |
| | `ToolSourceError::InvalidInput` | `is_user_error() = false` |
| | `ToolSourceError::ToolError` | `is_user_error() = true` |
| | `ToolSourceError::Internal` | `is_user_error() = true` |
| | act_executor Err 分支 (NotFound) | `is_error=false` 传给 normalize |
| **Phase 4** | `run_dir=""` | `Err(InvalidInput)` |
| | `run_dir="../../etc/passwd"` | `Err(InvalidInput)` 包含 ".. 阻止" |
| | `script: 123`（非字符串） | `Err(InvalidInput)` |
| | 错误信息前缀去重 | `to_string()` 不含 "invalid arguments:" |

**集成测试**（`agent/tool/tool-workflow/tests/`）：

- mock LuftBuilder，完整跑一遍 `run` 流程
- 验证 `is_error` 字段在 StreamEvent 中的传递
- 验证 `Message::Tool` content 中的标签（result / error）

---

## 5. 迁移与发布

### 5.1 顺序

1. **PR 1（Phase 1）**：核心错误处理重构，单独 review
2. **PR 2（Phase 2）**：CLI 渲染，可与 PR 1 合并或独立
3. **PR 3（Phase 3）**：`is_error` 分级 + 吞错，影响面大，单独 review + 通知所有 tool 作者
4. **PR 4（Phase 4）**：体验优化，下个版本发

### 5.2 兼容性

- 对**调用方（agent / LLM）**的影响：
  - Failed 路径从 `Ok(Text("..."))` 改为 `Err("...")`，LLM 看到 `error` 标签 → 行为改善，无破坏
  - `Cancelled` 路径同上
  - `list_*` 返回 JSON **新增** `warnings` 字段，**不删** 旧字段 → 向后兼容
- 对**其他 tool**的影响：
  - `ToolSourceError::Internal` 是新增 variant，现有 match 若 exhaustive 会编译失败 → 全局 `grep -rn "match.*ToolSourceError"`，补充 `Internal` 分支
  - `is_user_error()` 是新方法，不影响现有 match

### 5.3 监控

部署后 1 周内观察：

| 指标 | 期望变化 |
|---|---|
| `tool_workflow_failed_is_error_false` 计数 | 应**降为 0**（之前误判为成功） |
| `tool_workflow_cancelled` 计数 | 应**升**（之前被静默） |
| `tool_workflow_warnings` 计数 | 应**升**（之前被吞） |
| LLM 重试同一 workflow tool 的次数 | 应**降**（错误信息更明确） |

---

## 6. 开放问题

1. **`Internal` variant 与 `ToolError` 的边界**：序列化失败算 `ToolError` 还是 `Internal`？建议统一为 `Internal`（系统级 vs 用户级）。
2. **`Cancelled` 算不算错误**：当前设计走 `Err` 路径，但语义上不是错误。是否引入 `Cancelled` 作为独立 variant？倾向于**不分**，简化 act 层。
3. **CLI 黄色 warning 面板是否所有 tool 都用**：建议先在 `workflow` 上验证效果，OK 后再推广。
4. **`warnings` 字段是否要 schema 化**：先做"自由文本数组"，等需求明确再上 schema 校验。
5. **路径校验的边界**：symlink 逃逸是否纳入？建议用 `canonicalize` + `starts_with` 双重校验（性能换安全）。

---

## 7. 关联文档

- 审计报告：`docs/audit/workflow-tool-errors.md`
- Luft 集成设计：`docs/design/luft-integration.md`
- 工具显示 UX 规范：`docs/tool-display-ux.md`（如果存在，否则补）
- `ToolSourceError` 定义：`foundation/llm/src/tool.rs:166-177`
- `ActExecutor`：`agent/agent-core/src/agent/react/act_executor.rs`
