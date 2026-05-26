# CLI 输出用户体验改进方案

## 背景

CLI 输出系统分布在以下核心文件中：

- `cli/src/run/agent.rs` — 流事件处理、stderr 显示回调
- `cli/src/run/display.rs` — 状态格式化与截断
- `cli/src/output.rs` — stdout/file 输出工具（JSON & 文本）
- `cli/src/repl.rs` — 交互式 REPL 循环
- `cli/src/display_limits.rs` — 截断常量

当前输出依赖分散在各事件处理器中的 `eprintln!`，仅有 `verbose` 开/关两种状态。导致普通模式信息太少，verbose 模式 Debug 信息过载。

## 现存问题

### P1：LLM 思考期间无进度反馈

普通模式下，启动信息打印完毕后，终端会完全沉默直到回复开始流式输出。对于涉及工具调用的多轮 agent 运行，可能有 10–30 秒的"假死"感。

当前输出（普通模式）：
```
agent: dev (project) — Code assistant
loaded tools: bash, read, edit, glob, grep
model: claude-sonnet-4 (200K context)

... 15 秒无任何输出 ...

Here is the code you asked for:
```

### P2：stderr 信息扁平且嘈杂

启动信息、工具名称、LLM 统计、状态 dump 全部以相同视觉权重输出。用户无法快速扫描到关键信息。

### P3：思考内容与回复内容混杂

`print_stream_chunk` 将 thinking 发送到 stderr、回复发送到 stdout，但两者之间没有视觉边界。在 verbose 模式下，thinking 内容混在状态 dump 中难以辨认。

### P4：verbose 模式状态 dump 可读性差

`format_react_state_display` 输出的是 Rust Debug 风格的嵌套结构。`ReActState { messages: ..., tool_calls: ..., tool_results: ... }` 格式是为开发者调试设计的，不适合用户阅读。

### P5：不同 agent 的 LLM usage 格式不一致

- ReAct：`\nLLM: 2.35s | prefill: 1200t / 0.85s = 1412 t/s | decode: 800t / 1.50s = 533 t/s`
- DUP/TOT/GOT：`\nLLM: prompt=1200, completion=800`
- 最终汇总：`LLM: 3.50s, 571 tokens/s (prompt: 1200, completion: 800)`

同一个概念用了三种不同格式。

### P6：REPL 过于简陋

- 仅有一个 `>` 提示符，无上下文信息
- 无命令历史（↑↓ 方向键）
- 无颜色区分

---

## 改进方案

### 第一阶段：进度指示器（最高优先级）

**目标：** 展示实时进度，用户永远不必疑惑"是不是卡住了"

**设计：**

1. 添加单行状态栏，使用 `\r`（回车符）原地更新：
   ```
   ⠋ 思考中...
   ⠋ 执行工具: bash (echo hello)
   ⠋ 思考中... (第 2 轮)
   ✓ 完成
   ```
2. Spinner 以 150ms 间隔循环 `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`。
3. 工具调用显示工具名称 + 第一个参数（截断到终端宽度）。
4. 流式回复开始时，清除 spinner，正常输出回复。

**实现：**

- 新增 `cli/src/run/spinner.rs` — 简单的 spinner 结构体，写入 stderr。
- `Spinner::new(label)` 启动动画。
- `Spinner::update(label)` 更新状态文本。
- `Spinner::finish()` 清除当前行。
- spinner 由 `on_event_react` / `on_event_dup` 等函数驱动，基于 `TaskStart` 和 `Updates` 事件。

**依赖：** 无需新增 crate。使用 `std::thread::spawn` + `std::sync::mpsc` 实现定时器，或用 `tokio::time` interval。实现时应检测 stderr 是否为 TTY，非 TTY 时回退到静态 `eprintln!`。

**涉及文件：** `agent.rs`（在事件处理器中集成 spinner）、新增 `spinner.rs`。

---

### 第二阶段：结构化 stderr 面板格式

**目标：** 让 stderr 信息一目了然。

**设计：**

使用带前缀的分类行，格式统一：

```
_AGENT  dev (project) — Code assistant
_TOOLS  bash, read, edit, glob, grep
_MODEL  claude-sonnet-4 (200K context)
```

执行过程中：
```
_CALL   bash: echo "hello world"
_CALL   read: src/main.rs
_DONE   bash: echo "hello world" ✓
_DONE   read: src/main.rs ✓
```

Usage 行（统一格式）：
```
_USAGE  2.35s | 1.2K↓ 800↑ = 2.0K @ 850 t/s
```

**实现：**

- 新增 `cli/src/run/format.rs`，包含辅助函数：
  - `format_panel_line(category, message)` — 生成 `_CATEGORY  message` 格式，前缀带 ANSI 颜色。
  - `format_tool_status(tool_name, args_summary, success)` — 生成工具调用/完成行。
  - `format_usage_line(duration, prompt_tokens, completion_tokens, prefill_duration, decode_duration)` — 统一格式。
- 添加 `--no-color` 参数，或遵循 `NO_COLOR` 环境变量。
- 替换 `agent.rs` 中所有原始 `eprintln!` 调用。

**涉及文件：** `agent.rs`、新增 `format.rs`、`args.rs`（添加 `--no-color`）。

---

### 第三阶段：思考与回复的视觉分离

**目标：** 用户能清晰区分思考过程和最终回答。

**设计：**

普通模式：
```
⠋ 思考中...
<thinking 内容以灰色/暗色流式输出>
────────────────────
<回复内容，正常亮度>
```

Verbose 模式：
```
[THINKING]
<thinking 内容，可能多行>
[/THINKING]
[REPLY]
<回复内容>
[/REPLY]
```

**实现：**

- 修改 `print_stream_chunk`：
  - thinking 块：如果是 TTY，用 ANSI 暗色包裹（`\x1b[2m...\x1b[0m`）。非 TTY 时，添加 `[thinking] ` 前缀。
  - 回复块：普通模式原样输出，verbose 模式添加 `[reply] ` 前缀。
- 在 thinking 过渡到 reply 时添加分隔线（在 `EventState` 中追踪状态）。

**涉及文件：** `agent.rs`（修改 `print_stream_chunk` 和 `EventState`）。

---

### 第四阶段：统一 LLM Usage 格式

**目标：** 所有 agent 和上下文使用一致的格式。

**格式：**
```
_USAGE  2.35s | 1.2K↓ 800↑ = 2.0K @ 850 t/s
```

带 prefill/decode 详情（仅 verbose）：
```
_USAGE  2.35s | prefill: 1.2K/0.85s=1.4K t/s | decode: 800/1.50s=533 t/s | total: 2.0K @ 850 t/s
```

**实现：**

- 将共享的 `format_usage_line(...)` 函数提取到 `format.rs`。
- 替换 `on_event_react`、`on_event_dup`、`on_event_tot`、`on_event_got` 中的三种不同 `eprintln!` 模式，以及 `run_agent_wrapper` 中的最终汇总。
- 每个事件处理器使用相同函数，参数中 prefill/decode 为 `Option`。

**涉及文件：** `agent.rs`（四个 `on_event_*` 函数 + 最终汇总）。

---

### 第五阶段：REPL 增强

**目标：** 让交互模式成为真正的聊天界面。

**设计：**

1. 富提示符：`loom (react) > `，显示当前 agent 模式。
2. 颜色编码输出：
   - 用户输入：暗色
   - 助手回复：正常/默认色
   - 工具结果：黄色
   - 错误：红色
3. 支持 ↑↓ 方向键浏览命令历史。
4. 支持 `\` 续行输入多行文本。

**实现：**

- 在 `cli/Cargo.toml` 添加 `rustyline` 依赖。
- 替换 `repl.rs` 中的 `BufReader::new(stdin()).lines()` 循环为 `rustyline::Editor`。
- 根据消息类型用 ANSI 颜色包裹输出。
- 如果 `rustyline` 初始化失败（如非 TTY），回退到当前简单实现。

**依赖：** `rustyline = "14"`（或最新版）。

**涉及文件：** `repl.rs`、`cli/Cargo.toml`。

---

### 第六阶段：输出详细度分级（替代 --verbose）

**目标：** 用三级系统替代 verbose 二元开关。

**级别：**

| 参数 | stderr | 进度 | 状态 dump | Usage 详情 |
|---|---|---|---|---|
| `--quiet` / `-q` | 无 | 无 | 无 | 无 |
| （默认） | 面板格式 | spinner + 工具摘要 | 无 | 仅汇总 |
| `--verbose` / `-v` | 面板格式 | spinner + 工具详情 | 结构化状态 | prefill/decode |

**实现：**

- 添加 `Verbosity` 枚举：`Quiet`、`Normal`、`Verbose`。
- 将 `opts.verbose: bool` 替换为 `opts.verbosity: Verbosity`。
- `--quiet` / `-q` 设为 `Quiet`，`--verbose` / `-v` 设为 `Verbose`，默认为 `Normal`。
- 向后兼容：`-v` 仍然可用，映射到 `Verbose` 级别。

**涉及文件：** `args.rs`、`agent.rs`、`display_limits.rs`，以及所有读取 `opts.verbose` 的文件。

---

## 优先级与工作量

| 阶段 | 影响力 | 工作量 | 风险 |
|---|---|---|---|
| 第一阶段：进度指示器 | ★★★★★ | 1–2 天 | 低 |
| 第二阶段：结构化面板 | ★★★★ | 1 天 | 低 |
| 第三阶段：思考/回复分离 | ★★★ | 0.5 天 | 低 |
| 第四阶段：统一 Usage 格式 | ★★★ | 0.5 天 | 低 |
| 第五阶段：REPL 增强 | ★★★ | 1–2 天 | 中（新依赖） |
| 第六阶段：详细度分级 | ★★ | 1 天 | 中（重构） |

推荐顺序：第一阶段 → 第二阶段 → 第四阶段 → 第三阶段 → 第六阶段 → 第五阶段。

第一至第四阶段可合并为一次 "UX v2" 版本发布。第五、六阶段作为后续迭代。

---

## 兼容性说明

- 所有 ANSI 颜色输出必须检测 `isatty(stderr)` / `isatty(stdout)`，并遵循 `NO_COLOR` 环境变量。
- JSON 模式（`--json`）不受影响 — 所有改动仅适用于文本模式。
- 第六阶段中 `--verbose` 保持向后兼容（仍然可用，映射到 `Verbose` 级别）。
- 流式 chunk 行为（thinking → stderr、reply → stdout）保持不变，确保管道兼容性。
