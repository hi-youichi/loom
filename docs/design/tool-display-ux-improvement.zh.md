# CLI & Goal 工具显示用户体验优化方案

> 版本: v2 | 状态: 提案
> 目标: 统一 CLI 和 goal 的工具显示，提供更丰富、更可读的工具执行反馈

## 当前状态分析

### 现有工具显示

**CLI (react/dup/tot/got)**:
```
_CALL    bash: echo hello
_DONE    bash: echo hello ✓
```

**Goal runner**:
```
[tool] bash -> result preview text...
```

### 问题

1. **Goal runner 工具显示未统一**: 仍用 `[tool]` 而非 `_DONE`
2. **缺少执行时间**: 不知道工具跑了多久
3. **缺少执行状态**: 失败的工具没有红色 ✗ 标记
4. **参数显示过于原始**: JSON 参数直接显示，可读性差
5. **缺少工具结果摘要**: 用户不知道工具返回了什么（只有 DONE ✓，没有结果预览）
6. **多工具并行无法区分**: 一次多个 tool_call 时，哪个是哪个不清楚
7. **没有进度指示**: bash 命令运行时没有 spinner 提示

## 优化方案

### Phase 1: 统一工具显示格式

**目标**: Goal runner 也使用 `_CALL`/`_DONE` 面板格式

```
_GOAL     iteration 2 | tool: loom | time: 45s
_CALL     bash: pytest tests/
_DONE     bash: pytest tests/ ✓ 3.2s
_REPLY    All 15 tests passed.
```

关键变化:
- `[tool]` → `_DONE`（与 CLI 统一）
- `_DONE` 后增加执行耗时
- 工具结果摘要（截断到 80 字符）

### Phase 2: 增强工具信息密度

**目标**: 在 `_CALL` 和 `_DONE` 行中展示更有意义的信息

#### 2a. 工具特定参数摘要

不同工具展示不同的摘要信息:

```
_CALL    read: src/main.rs
_DONE    read: src/main.rs ✓ 128 lines

_CALL    edit: src/main.rs (L42-56)
_DONE    edit: src/main.rs ✓ 3 replacements

_CALL    bash: pytest tests/
_DONE    bash: pytest tests/ ✓ 3.2s | 15 passed, 0 failed

_CALL    write: config.yaml
_DONE    write: config.yaml ✓ 245 bytes

_CALL    grep: "TODO" in src/**/*.rs
_DONE    grep: "TODO" in src/**/*.rs ✓ 12 matches

_CALL    web_fetch: https://api.example.com/data
_DONE    web_fetch: https://api.example.com/data ✓ 200 OK | 2.1KB
```

实现方式:
- 每个工具有 `summarize_call()` 和 `summarize_result()` 方法
- 默认实现: `tool_name: first_arg` / `tool_name: first_arg ✓`

#### 2b. 执行耗时

`_DONE` 行显示工具执行耗时:

```
_DONE    bash: npm test ✓ 12.3s
_DONE    read: Cargo.toml ✓ 0.1s
_DONE    bash: cargo build ✓ 45.2s
```

超时阈值:
- < 0.1s: 不显示（瞬时）
- 0.1s - 60s: 显示秒 `3.2s`
- > 60s: 显示分+秒 `2m 15s`

#### 2c. 错误标记

工具执行失败时:

```
_DONE    bash: npm test ✗ 1.5s | exit code 1
_FAIL    bash: npm test | exit code 1 | 3 failed, 12 passed
```

### Phase 3: 流式工具进度

**目标**: 长时间运行的工具实时显示进度

#### 3a. Bash 命令实时输出

```
_CALL    bash: cargo build
_BASH    Compiling loom v0.2.1
_BASH    Compiling cli v0.2.1
_DONE    bash: cargo build ✓ 45.2s
```

- `_BASH` 行: 灰色/dim，实时追加
- 仅在 `verbose` 模式或 `--stream-bash` 时启用
- 默认静默（只显示 `_CALL` → `_DONE`）

#### 3b. 文件操作进度

多文件批量操作时:

```
_CALL    edit: src/main.rs (L42)
_CALL    edit: src/lib.rs (L15)
_CALL    edit: tests/main.rs (L8)
_DONE    edit: 3 files modified ✓ 0.2s
```

### Phase 4: 工具执行时间线

**目标**: 在 turn 结束时显示工具执行摘要

#### 4a. Turn 结束摘要

```
──────────────────────────────────────────
_TOOLS    bash (3) | read (5) | edit (2) | total: 23.1s
_USAGE    2.35s | 1.2K↓ 800↑ @ 850 t/s
```

显示:
- 每种工具的调用次数
- 总工具执行时间
- 与 LLM usage 分行显示

#### 4b. Goal 迭代摘要

Goal 模式下，每次迭代后:

```
_SUMMARY  tools: bash(2) read(4) edit(1) | wall: 23.1s | llm: 2.35s | tokens: 2.0K
```

## 数据结构变更

### ToolCall 扩展

```rust
/// Enhanced tool call display info.
pub struct ToolCallDisplay {
    pub name: String,
    pub call_summary: String,     // e.g., "src/main.rs" or "pytest tests/"
    pub tool_id: Option<String>,  // for matching call → done
}

/// Enhanced tool result display info.
pub struct ToolResultDisplay {
    pub name: String,
    pub call_summary: String,
    pub status: ToolExecStatus,   // Success / Failed / Timeout
    pub duration: Option<Duration>,
    pub result_summary: String,   // e.g., "128 lines" or "15 passed"
}

pub enum ToolExecStatus {
    Success,
    Failed(String),  // exit code or error message
    Timeout,
}
```

### panel_format 新增

```rust
/// `_CALL   tool_name: summary`
pub fn format_tool_call(tool: &str, summary: &str) -> String;

/// `_DONE   tool_name: summary ✓ 3.2s` 或 `_DONE   tool_name: summary ✓ 3.2s | 128 lines`
pub fn format_tool_done(tool: &str, summary: &str, duration: Option<Duration>, extra: Option<&str>) -> String;

/// `_FAIL   tool_name: summary ✗ 1.5s | exit code 1`
pub fn format_tool_fail(tool: &str, summary: &str, duration: Option<Duration>, error: &str) -> String;

/// `_TOOLS  bash (3) | read (5) | edit (2) | total: 23.1s`
pub fn format_tools_summary(tools: &[(String, u32)], total_duration: Duration) -> String;
```

## 实现优先级

| Phase | 内容 | 工作量 | 优先级 |
|-------|------|--------|--------|
| **Phase 1** | 统一 goal runner 工具显示 | 0.5 天 | P0 |
| **Phase 2a** | 工具特定参数摘要 | 1 天 | P1 |
| **Phase 2b** | 执行耗时 | 0.5 天 | P1 |
| **Phase 2c** | 错误标记 | 0.5 天 | P1 |
| **Phase 3a** | Bash 流式输出 | 1 天 | P2 |
| **Phase 3b** | 文件操作进度 | 0.5 天 | P2 |
| **Phase 4a** | Turn 摘要 | 0.5 天 | P2 |
| **Phase 4b** | Goal 迭代摘要 | 0.5 天 | P2 |

## 对比: 当前 vs 优化后

### 当前

```
_GOAL     iteration 1 | tool: loom | time: 0s
Think
_CALL    bash: {"command":"cargo test"}
_DONE    bash: {"command":"cargo test"} ✓
_CALL    read: {"path":"src/main.rs"}
_DONE    read: {"path":"src/main.rs"} ✓

_USAGE    2.35s | 1.2K↓ 800↑ @ 850 t/s
```

### 优化后

```
_GOAL     iteration 1 | tool: loom | time: 0s
_CALL    bash: cargo test
_DONE    bash: cargo test ✓ 12.3s | 37 passed, 0 failed
_CALL    read: src/main.rs
_DONE    read: src/main.rs ✓ 128 lines
──────────────────────────────────────────
_TOOLS    bash (1) | read (1) | total: 12.4s
_USAGE    2.35s | 1.2K↓ 800↑ @ 850 t/s
```

关键改善:
1. **参数可读**: `cargo test` 而非 `{"command":"cargo test"}`
2. **结果预览**: `37 passed, 0 failed` 而非空
3. **执行耗时**: `12.3s` 让用户感知工具花了多久
4. **Turn 摘要**: `_TOOLS` 行一目了然
