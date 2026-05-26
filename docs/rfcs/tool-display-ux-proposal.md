# CLI & Goal 工具显示 UX 优化方案

## 当前状态分析

### 现有输出示例（React normal mode）
```
⠋ Thinking...
_CALL  bash: {"command":"grep -r \"TODO\" src/**/*.rs"}
_USAGE  2.35s | 1.2K↓ 800↑ @ 850 t/s

_DONE  bash: {"command":"grep -r \"TODO\" src/**/*.rs"} ✓
_CALL  read: {"path":"src/main.rs"}
_USAGE  1.80s | 800↓ 600↑ @ 778 t/s

_DONE  read: {"path":"src/main.rs"} ✓
```

### 问题清单

| # | 问题 | 严重度 | 影响 |
|---|------|--------|------|
| 1 | **参数是原始 JSON** — `{"command":"grep -r \"TODO\""}` 噪音极大 | 🔴 高 | 用户真正需要的信息被 JSON 语法淹没 |
| 2 | **DONE 行重复完整参数** — 浪费垂直空间 | 🟡 中 | 多工具时输出非常冗长 |
| 3 | **无工具执行耗时** — 不知道工具运行了多久 | 🟡 中 | 无法判断性能瓶颈 |
| 4 | **无工具结果预览** — 不知道工具返回了什么 | 🟡 中 | 需要等待完整输出才能了解进度 |
| 5 | **多工具无编号/进度** — 并行工具调用堆叠在一起 | 🟢 低 | 多工具时难以跟踪 |
| 6 | **CALL 和 DONE 之间无视觉分组** | 🟢 低 | 多轮次时输出混乱 |

---

## 优化方案

### 方案 A：智能参数摘要（推荐实施）

**核心思路**：根据工具名称提取关键参数，而非显示原始 JSON。

#### 改动位置
- `panel_format.rs` → `format_tool_call()` / `format_tool_done()`

#### 改动后的显示
```
之前：
_CALL  bash: {"command":"grep -r \"TODO\" src/**/*.rs"}
_CALL  read: {"path":"src/main.rs","offset":10,"limit":50}
_CALL  edit: {"path":"src/main.rs","oldString":"fn main() {","newString":"fn hello() {"}
_CALL  write_file: {"path":"src/new.rs","content":"fn main() {}\n..."}
_CALL  glob: {"pattern":"**/*.rs"}

之后：
_CALL  bash: grep -r "TODO" src/**/*.rs
_CALL  read: src/main.rs [10:50]
_CALL  edit: src/main.rs: "fn main() {" → "fn hello() {"
_CALL  write_file: src/new.rs (28 bytes)
_CALL  glob: **/*.rs
```

#### 实现方式
```rust
/// 根据工具名称提取关键参数摘要
pub fn format_tool_call(tool_name: &str, args_json: &str) -> String {
    let summary = extract_tool_summary(tool_name, args_json);
    let msg = format!("{}: {}", yellow(tool_name), summary);
    format_panel_line("CALL", &msg)
}

fn extract_tool_summary(tool_name: &str, args_json: &str) -> String {
    // 尝试解析为 JSON，提取关键字段
    let Ok(val) = serde_json::from_str::<serde_json::Value>(args_json) else {
        return truncate_args(args_json, 60);
    };
    
    match tool_name {
        "bash" => val.get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| truncate_args(args_json, 60)),
        "read" => {
            let path = val.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let offset = val.get("offset").and_then(|v| v.as_u64());
            let limit = val.get("limit").and_then(|v| v.as_u64());
            match (offset, limit) {
                (Some(o), Some(l)) => format!("{} [{}:{}]", path, o, o+l),
                (Some(o), None) => format!("{} [{}:]", path, o),
                _ => path.to_string(),
            }
        }
        "edit" | "multiedit" => {
            let path = val.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let old = val.get("oldString").and_then(|v| v.as_str()).unwrap_or("");
            let new = val.get("newString").and_then(|v| v.as_str()).unwrap_or("");
            let old_short = truncate_str(old, 30);
            let new_short = truncate_str(new, 30);
            format!("{}: \"{}\" → \"{}\"", path, old_short, new_short)
        }
        "write_file" => {
            let path = val.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
            format!("{} ({} bytes)", path, content.len())
        }
        "glob" | "grep" => {
            let pattern = val.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            pattern.to_string()
        }
        "websearch" | "web_fetcher" => {
            let query_or_url = val.get("query")
                .or_else(|| val.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            truncate_str(query_or_url, 60).to_string()
        }
        "task_create" | "task_update" | "task_list" => {
            let name = val.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            truncate_str(name, 60).to_string()
        }
        "lsp" => {
            let action = val.get("action").and_then(|v| v.as_str()).unwrap_or("?");
            let file = val.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("{} {}", action, file)
        }
        _ => {
            // 兜底：提取第一个有意义的字段
            extract_first_meaningful_field(&val, 60)
        }
    }
}
```

### 方案 B：精简 DONE 行

**核心思路**：DONE 行不再重复参数，只显示工具名 + 结果状态 + 耗时。

#### 改动后
```
之前：
_DONE  bash: {"command":"grep -r \"TODO\" src/**/*.rs"} ✓
_DONE  read: {"path":"src/main.rs"} ✓

之后：
_DONE  bash ✓ (0.8s, 3 matches)
_DONE  read: src/main.rs ✓ (0.1s)
```

#### 实现
需要事件系统支持在 observe 节点传递工具执行耗时和结果摘要。
当前 `ToolCall` 只有 `name/arguments/id`，需要扩展或从 `ToolResult` 获取信息。

**可选实现路径**：
1. **最小改动**：DONE 行只显示工具名 + ✓，不重复参数
   ```rust
   pub fn format_tool_done(tool_name: &str, _args_summary: &str) -> String {
       format_panel_line("DONE", &format!("{} {}", tool_name, green("✓")))
   }
   ```
2. **完整改动**：在 EventState 中跟踪工具开始时间，在 observe 时计算耗时

### 方案 C：工具结果预览（可选，Phase 2）

在 DONE 行添加简短的结果摘要：
```
_DONE  bash ✓ (0.8s) → 3 matches, 42 lines
_DONE  read: src/main.rs ✓ (0.1s) → 128 lines
_DONE  edit: src/main.rs ✓ (0.0s) → replaced 1 occurrence
_DONE  glob: **/*.rs ✓ (0.2s) → 15 files
```

**需要**：从 ToolResult 提取摘要信息。这需要事件系统暴露更多数据。

### 方案 D：视觉分组（可选，Phase 2）

用空行 + 缩进对工具调用分组：
```
_CALL  bash: grep -r "TODO" src/**/*.rs
_CALL  read: src/main.rs [10:50]

_DONE  bash ✓ (0.8s)
_DONE  read: src/main.rs ✓ (0.1s)
```

或者更紧凑的格式：
```
▶ bash: grep -r "TODO" src/**/*.rs ... ✓ (0.8s)
▶ read: src/main.rs [10:50] ... ✓ (0.1s)
```

---

## 推荐实施计划

### Phase 1：立即可做（无需改事件系统）

| 改动 | 文件 | 效果 |
|------|------|------|
| **1a. 智能参数摘要** | `panel_format.rs` | 去掉 JSON 噪音，关键信息一目了然 |
| **1b. 精简 DONE 行** | `panel_format.rs` | DONE 只显示工具名 + ✓，不重复参数 |

Phase 1 只改 `panel_format.rs` 中的两个格式化函数，零风险，效果最显著。

### Phase 2：需要小幅改动（需扩展事件数据）

| 改动 | 文件 | 效果 |
|------|------|------|
| **2a. 工具耗时** | `event_handler.rs` + 状态追踪 | DONE 行显示耗时 |
| **2b. 结果预览** | 需要从 observe 获取 ToolResult 摘要 | DONE 行显示结果摘要 |

### Phase 3：锦上添花

| 改动 | 文件 | 效果 |
|------|------|------|
| **3a. 编号多工具** | `event_handler.rs` | `CALL [1/3] bash: ...` |
| **3b. 紧凑模式** | 新增 compact display mode | 单行显示整个工具调用 |

---

## 改动前后对比

### Before（当前）
```
⠋ Thinking...
_CALL  bash: {"command":"grep -rn \"format_tool\" loom/src/**/*.rs"}
_USAGE  2.35s | 1.2K↓ 800↑ @ 850 t/s

_DONE  bash: {"command":"grep -rn \"format_tool\" loom/src/**/*.rs"} ✓
_CALL  read: {"path":"loom/src/stream_display/panel_format.rs","offset":80,"limit":30}
_USAGE  1.80s | 800↓ 600↑ @ 778 t/s

_DONE  read: {"path":"loom/src/stream_display/panel_format.rs","offset":80,"limit":30} ✓
_CALL  edit: {"path":"loom/src/stream_display/panel_format.rs","oldString":"fn format_tool_call(tool_name: &str, args_summary: &str) -> String {","newString":"fn format_tool_call(tool_name: &str, args_json: &str) -> String {"}
```

### After（Phase 1）
```
⠋ Thinking...
_CALL  bash: grep -rn "format_tool" loom/src/**/*.rs
_USAGE  2.35s | 1.2K↓ 800↑ @ 850 t/s

_DONE  bash ✓
_CALL  read: panel_format.rs [80:110]
_USAGE  1.80s | 800↓ 600↑ @ 778 t/s

_DONE  read ✓
_CALL  edit: panel_format.rs: "fn format_tool_call(tool_name: &str, args_summary: &str)" → "fn format_tool_call(tool_name: &str, args_json: &str)"
```

### After（Phase 2 — 含耗时）
```
⠋ Thinking...
_CALL  bash: grep -rn "format_tool" loom/src/**/*.rs
_USAGE  2.35s | 1.2K↓ 800↑ @ 850 t/s

_DONE  bash ✓ (0.8s)
_CALL  read: panel_format.rs [80:110]
_USAGE  1.80s | 800↓ 600↑ @ 778 t/s

_DONE  read ✓ (0.1s)
_CALL  edit: panel_format.rs: "format_tool_call(tool_name: &str, args_summary: &str)" → "format_tool_call(tool_name: &str, args_json: &str)"
```

---

## 实施建议

**立即实施 Phase 1**：
1. 修改 `panel_format::format_tool_call()` — 添加 `extract_tool_summary()` 智能解析
2. 修改 `panel_format::format_tool_done()` — 精简为工具名 + ✓
3. 更新对应单元测试
4. CLI 和 Goal 同时受益（共享 `panel_format`）
