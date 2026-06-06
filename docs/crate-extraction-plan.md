# Loom Crate 拆分详细执行计划

> 版本: 0.4.0 | 日期: 2025-07 | 目标: 逐步提取所有模块为独立 crate，最终移除 `loom` 模块

## 1. 当前状态总览

### 1.1 Workspace 现有 crate（35 个）

```
核心基础设施层 (已提取, 全部为 re-export 薄壳):
  loom-llm, loom-graph, loom-pregel, loom-stream, loom-types,
  loom-memory, loom-model-spec, loom-lsp, loom-cache, loom-worktree,
  loom-compress, loom-commands, stream-event, config, model-spec-core

功能层 (已提取):
  loom-tools, loom-background-review, loom-curator, loom-skill

Agent 层 (已提取):
  loom-agent, loom-agent-patterns

应用层 (8 个):
  cli, serve, telegram-bot, loom-acp, task-core, task-cli,
  task-mcp-server, loom-examples

其他:
  loom-workspace (含 gh 子 crate)

待提取:
  loom/src/ 中的活跃代码
```

### 1.2 `loom/src/` 当前模块分类

#### A 类 — 死代码（目录存在但不被编译，可安全删除）

以下目录/文件在 `lib.rs` 中被 `pub use <crate> as <name>` 覆盖，目录内的文件不参与编译：

| 模块 | 文件数 | 行数 | 覆盖方式 | 对应 crate |
|---|---|---|---|---|
| `tools/` | 3 | ~300 | `pub use loom_tools as tools` | `loom-tools` |
| `tool_source/` | 17 | ~2,700 | `pub use loom_tools as tool_source` | `loom-tools` |
| `background_review/` (子文件) | 12 | ~5,000 | `mod.rs` re-export `loom_background_review::*` | `loom-background-review` |
| `compress/` (子文件) | 6 | ~807 | `mod.rs` re-export `loom_compress::*` | `loom-compress` |
| **小计** | **38** | **~8,807** | | |

#### B 类 — 已提取为 re-export 薄壳（只有 mod.rs，可保留）

| 模块 | 行数 | 对应 crate |
|---|---|---|
| `stream/mod.rs` | 11 | `loom-stream` |
| `memory/mod.rs` | 8 | `loom-memory` |
| `model_spec/mod.rs` | 7 | `loom-model-spec` |
| `lsp/mod.rs` | 8 | `loom-lsp` |
| `cache/mod.rs` | 4 | `loom-cache` |
| `worktree/mod.rs` | 4 | `loom-worktree` |
| `command/mod.rs` | 17 | `loom-commands` |
| `background_review/mod.rs` | 11 | `loom-background-review` |
| `compress/mod.rs` | 7 | `loom-compress` |
| `error.rs` | 6 | `loom-llm` |
| `message.rs` | 24 | `loom-llm` (含 1 个 loom 本地辅助函数) |

#### C 类 — 活跃代码（需要提取为新 crate 或合并到已有 crate）

| 模块 | 文件数 | 行数 | 外部依赖 | 提取难度 |
|---|---|---|---|---|
| `stream_display/` | 10 | 3,818 | stream-event, loom-stream, loom-llm, loom-types, state | ⭐⭐ |
| `cli_run/` | 2 | ~500 | helve, skill, llm, react_config, tier | ⭐⭐⭐ |
| `protocol/` | 6 | 1,720 | stream-event, loom-stream, serde | ⭐⭐ |
| `helve/` | 4 | 969 | config, model-spec-core, prompts, loom-types | ⭐⭐ |
| `prompts/` | 3 | 361 | serde_yaml | ⭐ |
| `openai_sse/` | 4 | 836 | stream-event, loom-stream, loom-llm, loom-types | ⭐⭐ |
| `llm/` (factory+registry) | 3 | ~800 | provider, tier, model-spec, async-openai | ⭐⭐⭐ |
| `state/` | 2 | ~900 | loom-types, loom-llm, chrono | ⭐ |
| `config/` | 6 | ~400 | llm, memory, tools | ⭐⭐ |
| `tier/` | 5 | ~430 | model-spec, provider, config, toml | ⭐ |
| `goal_runner/` | 3+1 | ~500 | llm | ⭐ |
| `user_message/` | 2 | ~200 | rusqlite | ⭐ |
| `profile_convert/` | 5 | 329 | cli_run | ⭐ |
| `services/` | 2 | 151 | protocol, model-spec, reqwest | ⭐ |
| `provider/` | 2 | 21 | config, llm | ⭐ |
| `active_operation.rs` | 1 | 144 | tokio-util | ⭐ |
| `export/` | 1 | ~400 | loom-stream, serde | ⭐ |
| `skill.rs` | 1 | 739 | config, loom-tools | ⭐ |
| `react_config.rs` | 1 | 261 | loom-skill, env_config | ⭐ |
| `runner_common.rs` | 1 | 123 | error, graph, memory, stream | ⭐ |
| `title_generator.rs` | 1 | 68 | llm, message, model-spec, tier, provider | ⭐ |
| **小计** | **~57** | **~10,970** | | |

#### D 类 — 根文件（需要归类）

| 文件 | 行数 | 去向 |
|---|---|---|
| `lib.rs` | 760 | facade 胶水 → 最终精简为纯 re-export |
| `traits.rs` | 66 | → `loom-graph` |
| `http_retry.rs` | 1 | → 删除（已是 re-export 注释） |
| `test_util.rs` | ~20 | → 测试辅助或删除 |

---

## 2. 执行策略

### 2.1 总原则

1. **每一步都可独立验证**: 完成每步后运行 `cargo build --workspace && cargo test --workspace`
2. **先删死代码，再提取活代码**: 减少干扰，降低认知负担
3. **依赖倒置**: 子 crate 不依赖 `loom`，`loom` 依赖子 crate
4. **re-export 兼容层**: 每次提取后在 `loom/src/` 保留 `pub use` re-export，确保下游零改动
5. **每步一个 git commit**: 格式 `refactor: extract <module> to <crate-name>`

### 2.2 阶段划分

```
Phase 1 (安全，零风险): Step 0 — 删除死代码
Phase 2 (低风险提取):   Step 1-6 — 提取无交叉依赖的独立模块
Phase 3 (中等风险):     Step 7-11 — 提取有交叉依赖的模块
Phase 4 (高风险):       Step 12-14 — 提取 llm/ 编排层 + 精简 facade
Phase 5 (可选):         Step 15 — 移除 loom crate
```

### 2.3 依赖关系图

```
Step 0 ─── 删除死代码 (零风险，独立)
  │
  ├─ Step 1 ─── openai_sse → loom-stream
  ├─ Step 2 ─── stream_display → loom-stream-display (新)
  ├─ Step 3 ─── protocol → loom-protocol (新)
  │    └─ Step 4 ─── export → loom-protocol
  ├─ Step 5 ─── helve + prompts → loom-helve (新)
  │    └─ Step 6 ─── profile_convert → loom-helve
  ├─ Step 7 ─── user_message → loom-types 或 loom-memory
  ├─ Step 8 ─── tool_output_normalizer → loom-types
  ├─ Step 9 ─── goal_runner types → loom-agent-patterns
  ├─ Step 10 ── react_config + runner_common → loom-agent-patterns
  ├─ Step 11 ── skill.rs → loom-skill
  ├─ Step 12 ── provider + services → loom-tier (新)
  ├─ Step 13 ── llm/factory+registry + title_generator → 应用层或新建 loom-llm-factory
  ├─ Step 14 ── cli_run + active_operation → 应用层
  ├─ Step 15 ── traits.rs → loom-graph
  ├─ Step 16 ── config/summary → loom-helve 或新建
  ├─ Step 17 ── 精简 loom facade
  │
  └─ Step 18 ── (可选) 移除 loom crate
```

**可并行**: Step 1, 2, 3, 7, 8, 9 之间无依赖关系

---

## 3. 详细执行步骤

### Step 0: 删除死代码

**目标**: 删除 `loom/src/` 中不被编译的冗余代码，减少 ~8,807 行
**风险**: ⭐ (零风险，这些文件本来不参与编译)
**预计时间**: 20 min

#### 0.1 删除 `loom/src/tools/` 目录

```bash
# tools/ 目录下有 aggregate_source.rs 和 bash/executor.rs
# lib.rs 第 160 行: `pub use loom_tools as tools;` 覆盖了目录模块声明
# 目录不被编译，直接删除
rm -rf loom/src/tools/
```

#### 0.2 删除 `loom/src/tool_source/` 目录

```bash
# tool_source/ 目录有 17 个文件
# lib.rs 第 159 行: `pub use loom_tools as tool_source;` 覆盖了目录模块声明
rm -rf loom/src/tool_source/
```

#### 0.3 删除 `loom/src/background_review/` 子文件

```bash
# mod.rs 已是 re-export (11 行)，子文件不被编译
rm loom/src/background_review/agent_loop.rs
rm loom/src/background_review/curator.rs
rm loom/src/background_review/curator_backup.rs
rm loom/src/background_review/history.rs
rm loom/src/background_review/memory.rs
rm loom/src/background_review/observability.rs
rm loom/src/background_review/prompts.rs
rm loom/src/background_review/security.rs
rm loom/src/background_review/skill_registry.rs
rm loom/src/background_review/skill_usage.rs
rm loom/src/background_review/tools.rs
rm loom/src/background_review/workflow.rs
# 保留 mod.rs
```

#### 0.4 删除 `loom/src/compress/` 子文件

```bash
# mod.rs 已是 re-export (7 行)，子文件不被编译
rm loom/src/compress/compact_node.rs
rm loom/src/compress/compaction.rs
rm loom/src/compress/config.rs
rm loom/src/compress/context_window.rs
rm loom/src/compress/graph.rs
rm loom/src/compress/prune_node.rs
# 保留 mod.rs
```

#### 0.5 删除 `loom/src/http_retry.rs`

```bash
# 内容只有 1 行注释: "HTTP retry utilities — re-exported from loom-llm."
# lib.rs 第 136 行: `mod http_retry;` — 需要先移除此行
```

**具体操作**:
1. 在 `lib.rs` 中删除 `mod http_retry;` (第 136 行)
2. 删除 `loom/src/http_retry.rs`

#### 0.6 验证

```bash
cargo build -p loom
cargo test -p loom
git add -A && git commit -m "refactor: remove dead code from loom/src (tools, tool_source, background_review, compress, http_retry)"
```

---

### Step 1: 提取 `openai_sse/` → `loom-stream` (补充)

**目标**: 将 `openai_sse/`（4 文件, 836 行）替换为 re-export，实现归入已有 `loom-stream` crate
**风险**: ⭐⭐
**预计时间**: 1h

#### 1.1 当前 `openai_sse/` 的内容

```
openai_sse/
├── mod.rs      (493 行) — StreamToSse, 公开类型, write_sse_line
├── chunk.rs    (98 行)  — ChatCompletionChunk DTO
├── parse.rs    (~150 行) — parse_chat_request, ParsedChatRequest
└── request.rs  (~95 行) — ChatCompletionRequest DTO
```

#### 1.2 外部依赖

```rust
// 来自 crate 内部的引用
use crate::stream::StreamEvent;           // → loom_stream::StreamEvent
use crate::state::ReActState;             // → loom_types::state::ReActState
use crate::message::Message;              // → loom_llm::message::Message
use crate::state::{ToolOutputHint, ...};  // → loom_types 或 loom_llm::tool

// 外部 crate
serde, serde_json, tokio, futures, tokio-stream
```

#### 1.3 执行步骤

1. **将 `openai_sse/` 的 4 个文件复制到 `loom-stream/src/openai_sse/`**
2. **修改 `loom-stream/src/openai_sse/` 中的 import 路径**:
   ```rust
   // 修改前 → 修改后
   crate::stream::StreamEvent     → crate::StreamEvent
   crate::state::ReActState       → loom_types::state::ReActState
   crate::message::Message        → loom_llm::message::Message
   crate::state::ToolOutputHint   → loom_llm::tool::ToolOutputHint
   crate::state::ToolOutputStrategy → loom_llm::tool::ToolOutputStrategy
   ```
3. **在 `loom-stream/src/lib.rs` 中添加 `pub mod openai_sse;`**
4. **在 `loom-stream/Cargo.toml` 中添加依赖**: `loom-types`, `loom-llm`
5. **将 `loom/src/openai_sse/mod.rs` 替换为 re-export**:
   ```rust
   //! OpenAI SSE adapter — re-exported from loom-stream crate.
   pub use loom_stream::openai_sse::*;
   ```
6. **删除 `loom/src/openai_sse/` 的子文件** (`chunk.rs`, `parse.rs`, `request.rs`)

#### 1.4 验证

```bash
cargo build -p loom-stream && cargo build -p loom
cargo test -p loom-stream && cargo test -p loom
git add -A && git commit -m "refactor: extract openai_sse to loom-stream"
```

---

### Step 2: 提取 `stream_display/` → 新建 `loom-stream-display`

**目标**: 将 `stream_display/`（10 文件, 3,818 行）提取为独立 crate
**风险**: ⭐⭐
**预计时间**: 2h

#### 2.1 当前 `stream_display/` 的内容

```
stream_display/
├── mod.rs            (21 行)   — 模块声明 + re-export
├── event_handler.rs  (529 行)  — 事件回调, CLI 展示
├── format.rs         (~100 行) — 格式化
├── format_subagent.rs (~100 行)— 子代理格式化
├── markdown.rs       (477 行)  — Markdown 渲染
├── panel_format.rs   (~100 行) — 面板格式
├── spinner.rs        (~100 行) — Spinner
├── streaming_markdown.rs (~100 行) — 流式 Markdown
├── tool_preview.rs   (952 行)  — 工具输出预览
└── tool_summary.rs   (655 行)  — 工具摘要
```

#### 2.2 外部依赖

```rust
// 来自 crate 内部
crate::stream::{StreamEvent, MessageChunk, ...}  // → loom_stream
crate::state::ReActState                          // → loom_types::state
crate::llm::{LlmUsage, ToolCall}                  // → loom_llm
crate::message::Message                           // → loom_llm::message
stream_event::{...}                               // → stream_event crate

// 外部 crate
chrono, termsize, console, serde, serde_json
```

#### 2.3 执行步骤

1. **创建目录结构**:
   ```
   loom-stream-display/
   ├── Cargo.toml
   └── src/
       ├── lib.rs
       ├── event_handler.rs
       ├── format.rs
       ├── format_subagent.rs
       ├── markdown.rs
       ├── panel_format.rs
       ├── spinner.rs
       ├── streaming_markdown.rs
       ├── tool_preview.rs
       └── tool_summary.rs
   ```

2. **创建 `Cargo.toml`**:
   ```toml
   [package]
   name = "loom-stream-display"
   version.workspace = true
   edition.workspace = true

   [dependencies]
   stream-event = { path = "../stream-event" }
   loom-stream = { path = "../loom-stream" }
   loom-llm = { path = "../loom-llm" }
   loom-types = { path = "../loom-types" }
   serde = { version = "1.0", features = ["derive"] }
   serde_json = "1.0"
   chrono = { version = "0.4", features = ["serde"] }
   termsize = "0.1"
   console = "0.15"
   ```

3. **复制文件并修改 import 路径**:
   - `crate::stream::*` → `loom_stream::*`
   - `crate::state::ReActState` → `loom_types::state::ReActState`
   - `crate::llm::*` → `loom_llm::*` (对应子模块)
   - `crate::message::*` → `loom_llm::message::*`

4. **在根 `Cargo.toml` workspace members 中添加 `"loom-stream-display"`**

5. **在 `loom/Cargo.toml` 中添加**: `loom-stream-display = { path = "../loom-stream-display" }`

6. **将 `loom/src/stream_display/` 替换为 re-export 薄壳**:
   ```rust
   //! Stream display — re-exported from loom-stream-display crate.
   pub use loom_stream_display::*;
   ```

7. **删除 `loom/src/stream_display/` 子文件**

#### 2.4 验证

```bash
cargo build -p loom-stream-display && cargo build -p loom
cargo test -p loom-stream-display && cargo test -p loom
git add -A && git commit -m "refactor: extract stream_display to loom-stream-display"
```

---

### Step 3: 提取 `protocol/` → 新建 `loom-protocol`

**目标**: 将 `protocol/`（6 文件, 1,720 行）提取为独立 crate
**风险**: ⭐⭐
**预计时间**: 1.5h

#### 3.1 当前 `protocol/` 的内容

```
protocol/
├── mod.rs           (71 行)  — 模块声明 + 文档
├── envelope_state.rs (~100 行)— 信封状态
├── requests.rs      (462 行) — 请求类型
├── responses.rs     (521 行) — 响应类型
├── stream.rs        (630 行) — 流协议转换
└── types.rs         (~100 行) — 共享类型
```

#### 3.2 外部依赖

```rust
stream_event::{StreamEvent}       // → stream_event crate
loom_stream::{MessageChunk, ...}  // → loom_stream
loom_types::state::ReActState     // → loom_types (泛型参数)
serde, serde_json
```

#### 3.3 执行步骤

1. **创建 `loom-protocol/` 目录结构**:
   ```
   loom-protocol/
   ├── Cargo.toml
   └── src/
       ├── lib.rs
       ├── envelope_state.rs
       ├── requests.rs
       ├── responses.rs
       ├── stream.rs
       └── types.rs
   ```

2. **创建 `Cargo.toml`**:
   ```toml
   [package]
   name = "loom-protocol"
   version.workspace = true
   edition.workspace = true

   [dependencies]
   stream-event = { path = "../stream-event" }
   loom-stream = { path = "../loom-stream" }
   loom-types = { path = "../loom-types" }
   serde = { version = "1.0", features = ["derive"] }
   serde_json = "1.0"
   ```

3. **复制文件并修改 import 路径**

4. **添加 workspace member + loom 依赖**

5. **将 `loom/src/protocol/` 替换为 re-export 薄壳**

#### 3.4 验证

```bash
cargo build -p loom-protocol && cargo build -p loom
cargo test -p loom-protocol && cargo test -p loom
git add -A && git commit -m "refactor: extract protocol to loom-protocol"
```

---

### Step 4: 提取 `export/` → 合并到 `loom-protocol`

**目标**: `export/mod.rs`（~400 行）将 `StreamEvent<S>` 转为 JSON，与 `protocol/stream.rs` 功能类似
**风险**: ⭐
**预计时间**: 30 min
**前置**: Step 3 完成

#### 4.1 执行步骤

1. **将 `export/mod.rs` 内容移到 `loom-protocol/src/export.rs`**
2. **修改 import 路径**: `crate::stream::StreamEvent` → `loom_stream::StreamEvent`
3. **在 `loom-protocol/src/lib.rs` 中添加 `pub mod export;`**
4. **将 `loom/src/export/mod.rs` 替换为 re-export**:
   ```rust
   pub use loom_protocol::export::*;
   ```
5. **删除 `loom/src/export/` 目录**

#### 4.2 验证

```bash
cargo build -p loom-protocol && cargo build -p loom
git add -A && git commit -m "refactor: extract export to loom-protocol"
```

---

### Step 5: 提取 `helve/` + `prompts/` → 新建 `loom-helve`

**目标**: 将产品语义配置（4 文件, 969 行）+ Prompt 加载（3 文件, 361 行）提取为独立 crate
**风险**: ⭐⭐
**预计时间**: 2h

#### 5.1 当前内容

```
helve/
├── mod.rs       (48 行)   — 类型导出
├── config.rs    (~200 行) — HelveConfig
├── env_context.rs (486 行)— 环境上下文 (OsInfo, ShellInfo 等)
└── prompt.rs    (~235 行) — assemble_system_prompt

prompts/
├── mod.rs       (46 行)   — YAML 结构定义
├── load.rs      (~200 行) — 加载/解析 YAML
└── resolve.rs   (~115 行) — AgentPrompts 解析
```

#### 5.2 外部依赖

```rust
config (env_config)    // → env_config crate (package = "config")
model-spec-core        // → model_spec_core
serde, serde_yaml
loom-llm::message      // → Message 类型
loom-types::approval   // → ApprovalPolicy
```

#### 5.3 执行步骤

1. **创建 `loom-helve/` 目录**:
   ```
   loom-helve/
   ├── Cargo.toml
   └── src/
       ├── lib.rs
       ├── config.rs
       ├── env_context.rs
       ├── prompt.rs
       └── prompts/
           ├── mod.rs
           ├── load.rs
           └── resolve.rs
   ```

2. **创建 `Cargo.toml`**:
   ```toml
   [package]
   name = "loom-helve"
   version.workspace = true
   edition.workspace = true

   [dependencies]
   loom-types = { path = "../loom-types" }
   loom-llm = { path = "../loom-llm" }
   env_config = { path = "../config", package = "config" }
   model-spec-core = { path = "../model-spec-core" }
   serde = { version = "1.0", features = ["derive"] }
   serde_yaml = "0.9"
   ```

3. **复制文件并修改 import 路径**

4. **添加 workspace member + loom 依赖**

5. **将 `loom/src/helve/` 和 `loom/src/prompts/` 替换为 re-export 薄壳**

#### 5.4 验证

```bash
cargo build -p loom-helve && cargo build -p loom
cargo test -p loom-helve && cargo test -p loom
git add -A && git commit -m "refactor: extract helve + prompts to loom-helve"
```

---

### Step 6: 提取 `profile_convert/` → 合并到 `loom-helve`

**目标**: `profile_convert/`（5 文件, 329 行）依赖 `cli_run` 中的 `resolve_profile`，合并到 `loom-helve`
**风险**: ⭐
**预计时间**: 30 min
**前置**: Step 5 完成

#### 6.1 当前内容

```
profile_convert/
├── mod.rs        (97 行)  — ExportFormat 枚举 + 入口
├── claude_code.rs (~50 行)— Claude Code 格式导出
├── codex.rs      (55 行)  — Codex 格式导出
├── cursor.rs     (38 行)  — Cursor 格式导出
└── error.rs      (~15 行) — ConvertError
```

#### 6.2 执行步骤

1. **将 `profile_convert/` 文件移到 `loom-helve/src/profile_convert/`**
2. **修改 import**: `crate::cli_run::*` → 需要定义 trait 或引入 profile 类型
3. **将 `loom/src/profile_convert/` 替换为 re-export 薄壳**

#### 6.3 验证

```bash
cargo build -p loom-helve && cargo build -p loom
git add -A && git commit -m "refactor: extract profile_convert to loom-helve"
```

---

### Step 7: 提取 `user_message/` → 合并到 `loom-memory`

**目标**: `user_message/`（2 文件, ~200 行）是消息持久化，与 memory 功能最接近
**风险**: ⭐
**预计时间**: 30 min

#### 7.1 当前内容

```
user_message/
├── mod.rs           (~50 行) — UserMessageStore trait, NoOpUserMessageStore
└── sqlite_store.rs  (~150 行)— SqliteUserMessageStore
```

#### 7.2 外部依赖

```rust
rusqlite, tokio, serde, serde_json
```

#### 7.3 执行步骤

1. **将 `user_message/` 文件移到 `loom-memory/src/user_message/`**
2. **在 `loom-memory/Cargo.toml` 中确保有 `rusqlite` 依赖**
3. **修改 import 路径**
4. **在 `loom-memory/src/lib.rs` 中添加 `pub mod user_message;`**
5. **将 `loom/src/user_message/` 替换为 re-export 薄壳**:
   ```rust
   //! User message store — re-exported from loom-memory.
   pub use loom_memory::user_message::*;
   ```

#### 7.4 验证

```bash
cargo build -p loom-memory && cargo build -p loom
cargo test -p loom-memory
git add -A && git commit -m "refactor: extract user_message to loom-memory"
```

---

### Step 8: 提取 `state/tool_output_normalizer.rs` → 合并到 `loom-types`

**目标**: `tool_output_normalizer.rs`（~550 行）是纯数据处理逻辑，适合放在 `loom-types`
**风险**: ⭐
**预计时间**: 1h

#### 8.1 外部依赖

```rust
chrono       — 时间戳格式化
loom-llm     — ToolOutputHint, ToolOutputStrategy (通过 re-export)
std::path, std::fs — 文件操作
```

#### 8.2 执行步骤

1. **将 `tool_output_normalizer.rs` 复制到 `loom-types/src/tool_output_normalizer.rs`**
2. **在 `loom-types/Cargo.toml` 中添加依赖**: `chrono`, `loom-llm`
3. **修改 import 路径**: 确保 `loom_llm::tool::*` 可用
4. **在 `loom-types/src/lib.rs` 中添加 `pub mod tool_output_normalizer;`**
5. **将 `loom/src/state/tool_output_normalizer.rs` 替换为 re-export**:
   ```rust
   pub use loom_types::tool_output_normalizer::*;
   ```

#### 8.3 验证

```bash
cargo build -p loom-types && cargo build -p loom
cargo test -p loom-types && cargo test -p loom
git add -A && git commit -m "refactor: extract tool_output_normalizer to loom-types"
```

---

### Step 9: 提取 `goal_runner/` (state+message) → 合并到 `loom-agent-patterns`

**目标**: `goal_runner/state.rs` + `goal_runner/message.rs`（~300 行）归入 `loom-agent-patterns`
**风险**: ⭐
**预计时间**: 1h

#### 9.1 当前内容

```
goal_runner/
├── mod.rs       — pub mod + re-exports
├── message.rs   (~100 行) — escape_xml_text, build_continuation_prompt
├── state.rs     (~200 行) — TurnResult, ToolError, GoalMeta, GoalOutcome
├── runner.rs    — 已注释掉 (moved to loom-agent)
├── tool.rs      — 已注释掉 (moved to loom-agent)
└── tests.rs     — 测试
```

#### 9.2 执行步骤

1. **将 `state.rs`, `message.rs` 复制到 `loom-agent-patterns/src/goal/`**
2. **在 `loom-agent-patterns/Cargo.toml` 中确保有 `loom-llm` 依赖**
3. **修改 import 路径**
4. **将 `loom/src/goal_runner/` 替换为 re-export 薄壳**
5. **将 `goal_runner/tests.rs` 移到 `loom-agent-patterns/` 中**
6. **清理已注释掉的 `runner.rs` 和 `tool.rs`**

#### 9.3 验证

```bash
cargo build -p loom-agent-patterns && cargo build -p loom
cargo test -p loom-agent-patterns
git add -A && git commit -m "refactor: extract goal_runner types to loom-agent-patterns"
```

---

### Step 10: 提取 `react_config.rs` + `runner_common.rs` → 合并到 `loom-agent-patterns`

**目标**: React 配置和公共运行器逻辑归入 `loom-agent-patterns`
**风险**: ⭐⭐
**预计时间**: 1.5h
**前置**: Step 11 (skill.rs) 或解耦 SkillRegistry 依赖

#### 10.1 分析

- `react_config.rs`（261 行）— `ReactBuildConfig` 定义
  - 依赖: `env_config::McpServerDef`, `crate::skill::SkillRegistry`
- `runner_common.rs`（123 行）— `run_stream_with_config`, `load_from_checkpoint_or_build`
  - 依赖: `crate::error::AgentError`, `loom_graph::CompiledStateGraph`, `loom_memory::*`, `loom_stream::*`

#### 10.2 执行步骤

1. **将 `react_config.rs` 复制到 `loom-agent-patterns/src/react_config.rs`**
   - `crate::skill::SkillRegistry` → 需要引入 `loom_skill::SkillRegistry` 或定义 trait
2. **将 `runner_common.rs` 复制到 `loom-agent-patterns/src/runner_common.rs`**
   - 修改 import 路径
3. **在 `loom/src/react_config.rs` 替换为 re-export**
4. **在 `loom/src/runner_common.rs` 替换为 re-export**

#### 10.3 验证

```bash
cargo build -p loom-agent-patterns && cargo build -p loom
cargo test -p loom-agent-patterns
git add -A && git commit -m "refactor: extract react_config + runner_common to loom-agent-patterns"
```

---

### Step 11: 提取 `skill.rs` → 合并到 `loom-skill`

**目标**: `skill.rs`（739 行）的 `SkillRegistry` 合并到已有 `loom-skill` crate
**风险**: ⭐
**预计时间**: 1h

#### 11.1 执行步骤

1. **检查 `loom-skill/` 已有的功能，确认是否重叠**
2. **将 `SkillRegistry`, `SkillMetadata`, `SkillEntry`, `SkillSource`, `SkillError` 复制到 `loom-skill/src/registry.rs`**
3. **修改 import 路径**
4. **在 `loom/src/skill.rs` 替换为 re-export**

#### 11.2 验证

```bash
cargo build -p loom-skill && cargo build -p loom
cargo test -p loom-skill
git add -A && git commit -m "refactor: extract SkillRegistry to loom-skill"
```

---

### Step 12: 提取 `tier/` + `provider/` + `services/` → 新建 `loom-tier`

**目标**: 将 Tier 分辨（5 文件）+ Provider 加载（2 文件）+ 模型服务（2 文件）提取为独立 crate
**风险**: ⭐⭐
**预计时间**: 1.5h

#### 12.1 当前内容

```
tier/
├── mod.rs       (8 行)   — 模块声明
├── apply.rs     (79 行)  — resolve_tier_and_build_config
├── plan.rs      (75 行)  — TierPlan
├── resolve.rs   (~200 行)— Tier 解析逻辑
├── resolver.rs  (~67 行) — TierResolver trait
└── plans.toml            — Tier 计划配置

provider/
├── mod.rs       (2 行)   — pub use load::load_provider_configs
└── load.rs      (19 行)  — Provider 配置加载

services/
├── mod.rs       (3 行)   — pub mod models
└── models.rs    (~148 行)— ModelService
```

#### 12.2 外部依赖

```rust
model-spec-core    — ModelTier, ModelSpec
loom-llm           — LlmClient, ChatOpenAI, ProviderConfig
config (env_config)— Provider 配置
loom-types         — ModelConfig
toml, serde, serde_json, reqwest
```

#### 12.3 执行步骤

1. **创建 `loom-tier/` 目录**
2. **创建 `Cargo.toml`**
3. **复制文件并修改 import 路径**
4. **`plans.toml` 使用 `include_str!` 嵌入**
5. **添加 workspace member + loom 依赖**
6. **将 `loom/src/tier/`, `loom/src/provider/`, `loom/src/services/` 替换为 re-export 薄壳**

#### 12.4 验证

```bash
cargo build -p loom-tier && cargo build -p loom
cargo test -p loom-tier && cargo test -p loom
git add -A && git commit -m "refactor: extract tier+provider+services to loom-tier"
```

---

### Step 13: 提取 `llm/` (factory+registry) + `title_generator.rs` → 应用层或新建 `loom-llm-factory`

**目标**: LLM 运行时编排（factory, model_registry, title_generator）移到应用层
**风险**: ⭐⭐⭐ (依赖链复杂)
**预计时间**: 2h
**前置**: Step 12 (provider → loom-tier) 完成

#### 13.1 分析

```
llm/
├── mod.rs           (~47 行) — re-export + 本地模块声明
├── factory.rs       (~60 行) — LlmFactory (依赖 provider, tier)
└── model_registry.rs (~350 行)— ModelRegistry, create_llm_client (依赖 provider, tier, async-openai)

title_generator.rs   (68 行) — generate_title (依赖 llm, message, tier, provider)
```

这些模块依赖 `crate::provider::load_provider_configs()` 和 `crate::tier::*`，是**应用层编排**。

#### 13.2 执行步骤

**方案 A: 移到 `cli` crate**
- 简单直接，但 `serve` 也需要这些类型

**方案 B: 新建 `loom-llm-factory` crate** (推荐)
1. **创建 `loom-llm-factory/` 目录**
2. **将 `factory.rs`, `model_registry.rs` 移入**
3. **将 `title_generator.rs` 移入**
4. **依赖**: `loom-llm`, `loom-tier`, `loom-types`, `model-spec-core`, `async-openai`
5. **在 `loom/src/llm/` 中删除 `mod factory` 和 `mod model_registry`**
6. **将 `loom/src/title_generator.rs` 替换为 re-export**

#### 13.3 验证

```bash
cargo build -p loom-llm-factory && cargo build -p loom
cargo test -p loom-llm-factory && cargo test -p loom
git add -A && git commit -m "refactor: extract llm factory to loom-llm-factory"
```

---

### Step 14: 提取 `cli_run/` + `active_operation.rs` → 应用层

**目标**: CLI 编排逻辑移到 `cli` crate
**风险**: ⭐⭐⭐
**预计时间**: 2h
**前置**: Step 5 (helve), Step 10 (react_config), Step 13 (llm-factory)

#### 14.1 分析

```
cli_run/
├── mod.rs       — re-export + 类型定义 (RunOptions, RunCmd, etc.)
└── profile.rs   — resolve_profile, build_helve_config

active_operation.rs (144 行) — RunCancellation, ActiveOperationKind
```

这些是**应用层编排**，不应留在核心 `loom` crate。

#### 14.2 执行步骤

1. **将 `cli_run/` 移到 `cli/src/cli_run/`**
2. **将 `active_operation.rs` 移到 `loom-types/src/active_operation.rs`**（因为 serve 也使用）
3. **在 `loom/src/cli_run/mod.rs` 替换为 re-export**
4. **在 `loom/src/active_operation.rs` 替换为 re-export**

#### 14.3 验证

```bash
cargo build -p cli && cargo build -p serve && cargo build -p loom
cargo test -p cli && cargo test -p loom
git add -A && git commit -m "refactor: extract cli_run to cli, active_operation to loom-types"
```

---

### Step 15: 移动 `traits.rs` → `loom-graph`

**目标**: `traits.rs`（66 行）定义核心 `Agent` trait
**风险**: ⭐
**预计时间**: 30 min

#### 15.1 执行步骤

1. **将 `Agent` trait 移到 `loom-graph/src/agent.rs`**
2. **确保 `loom-graph/Cargo.toml` 有 `loom-llm` 依赖**（`AgentError` 类型）
3. **在 `loom/src/traits.rs` 替换为 re-export**:
   ```rust
   pub use loom_graph::agent::{Agent, AgentNode};
   ```

#### 15.2 验证

```bash
cargo build -p loom-graph && cargo build -p loom
git add -A && git commit -m "refactor: extract Agent trait to loom-graph"
```

---

### Step 16: 提取 `config/summary/` → 合并到 `loom-helve`

**目标**: `config/summary/`（5 文件, ~400 行）是配置摘要生成
**风险**: ⭐⭐
**预计时间**: 1h

#### 16.1 当前内容

```
config/
├── mod.rs               (12 行) — re-export
└── summary/
    ├── mod.rs            (70 行) — ConfigSection trait, RunConfigSummary
    ├── embedding.rs      (~80 行) — EmbeddingConfigSummary
    ├── llm.rs            (~80 行) — LlmConfigSummary
    ├── memory.rs         (~80 行) — MemoryConfigSummary
    └── tools.rs          (~90 行) — ToolConfigSummary
```

#### 16.2 执行步骤

1. **将 `config/summary/` 移到 `loom-helve/src/config_summary/`**
2. **修改 import 路径**
3. **将 `loom/src/config/` 替换为 re-export 薄壳**

#### 16.3 验证

```bash
cargo build -p loom-helve && cargo build -p loom
git add -A && git commit -m "refactor: extract config/summary to loom-helve"
```

---

### Step 17: 精简 `loom` facade

**目标**: 所有模块提取完毕后，`loom/src/lib.rs` 只保留 re-export 和胶水代码
**风险**: ⭐⭐
**预计时间**: 1h
**前置**: Step 0-16 全部完成

#### 17.1 精简后的 `loom/src/` 结构

```
loom/src/
├── lib.rs          # 纯 re-export 胶水 (~150 行)
└── (所有子目录/文件已删除或替换为 re-export)
```

#### 17.2 `lib.rs` 最终形态

```rust
//! Loom facade crate — re-exports from all sub-crates.
//!
//! This crate exists for backward compatibility. New code should
//! depend on specific sub-crates directly.

// === 核心基础设施 ===
pub use loom_graph::{channels, managed};
pub use loom_graph as graph;
pub use loom_pregel as pregel;
pub use loom_llm as llm; // (注意: 需要在 llm mod 中只做 re-export)

// === 功能层 ===
pub use loom_memory as memory;
pub use loom_tools as tools;
pub use loom_tools as tool_source;
pub use loom_stream as stream;
pub use loom_lsp as lsp;
pub use loom_cache as cache;
pub use loom_model_spec as model_spec;
pub use loom_worktree as worktree;
pub use loom_commands as command;
pub use loom_compress as compress;
pub use loom_background_review as background_review;
pub use loom_types as types;

// === 新提取的 crate ===
pub use loom_stream_display as stream_display;
pub use loom_protocol as protocol;
pub use loom_helve as helve;
pub use loom_tier as tier;

// === Agent 层 ===
pub use loom_agent_patterns as agent_patterns;
pub use loom_agent as agent;

// === 重要类型根级 re-export ===
pub use loom_graph::{
    generate_dot, generate_text, CompiledStateGraph, Node, Next, RunContext, StateGraph, END, START, ...
};
pub use loom_llm::{ChatOpenAI, LlmClient, Message, ...};
pub use loom_types::state::{ReActState, ...};
// ... 其他根级 re-export
```

#### 17.3 `Cargo.toml` 精简

移除所有不再直接使用的依赖，只保留子 crate 依赖。

```toml
[dependencies]
# 子 crate 依赖（所有实现都在子 crate 中）
loom-llm = { path = "../loom-llm" }
loom-graph = { path = "../loom-graph" }
loom-pregel = { path = "../loom-pregel" }
loom-stream = { path = "../loom-stream" }
loom-types = { path = "../loom-types" }
loom-memory = { path = "../loom-memory" }
loom-model-spec = { path = "../loom-model-spec" }
loom-lsp = { path = "../loom-lsp" }
loom-cache = { path = "../loom-cache" }
loom-worktree = { path = "../loom-worktree" }
loom-compress = { path = "../loom-compress" }
loom-commands = { path = "../loom-commands" }
loom-tools = { path = "../loom-tools" }
loom-background-review = { path = "../loom-background-review" }
loom-curator = { path = "../loom-curator" }
loom-skill = { path = "../loom-skill" }
loom-agent = { path = "../loom-agent" }
loom-agent-patterns = { path = "../loom-agent-patterns" }
# 新提取的 crate
loom-stream-display = { path = "../loom-stream-display" }
loom-protocol = { path = "../loom-protocol" }
loom-helve = { path = "../loom-helve" }
loom-tier = { path = "../loom-tier" }
```

#### 17.4 验证

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
git add -A && git commit -m "refactor: slim down loom to pure facade"
```

---

### Step 18: (可选) 移除 `loom` crate

**目标**: 如果所有下游 crate 已改为直接依赖子 crate，可以移除 `loom` crate
**风险**: ⭐⭐⭐⭐ (工作量大，下游可能有几百个 import)
**预计时间**: 4-8h

#### 18.1 前提条件

1. `cli/Cargo.toml` 改为依赖 `loom-agent`, `loom-stream-display`, `loom-helve` 等
2. `serve/Cargo.toml` 改为依赖 `loom-protocol`, `loom-agent`, `loom-tier` 等
3. `telegram-bot/Cargo.toml` 改为依赖 `loom-agent`, `loom-stream` 等
4. `loom-acp/Cargo.toml` 改为依赖 `loom-protocol`, `loom-agent` 等

#### 18.2 执行步骤

1. **逐个修改下游 crate 的 `Cargo.toml`**: 将 `loom` 依赖替换为具体子 crate
2. **逐个修改下游 crate 的 import**: `use loom::xxx` → `use loom_xxx::xxx`
3. **移除 `loom/` 目录**
4. **从 workspace members 中移除 `"loom"`**

#### 18.3 替代方案（推荐）

**保留 `loom` 作为 facade**（零代码，纯 re-export），让下游继续使用 `use loom::*`。这是 Rust 生态中常见的做法（如 `tokio` crate 本身就是 facade）。

---

## 4. 新建 crate 汇总

| Crate 名 | 来源模块 | 行数 | 类型 |
|---|---|---|---|
| `loom-stream-display` | `stream_display/` | 3,818 | 新建 |
| `loom-protocol` | `protocol/` + `export/` | 2,120 | 新建 |
| `loom-helve` | `helve/` + `prompts/` + `profile_convert/` + `config/summary/` | 2,059 | 新建 |
| `loom-tier` | `tier/` + `provider/` + `services/` | 601 | 新建 |
| `loom-llm-factory` | `llm/factory+registry` + `title_generator` | ~480 | 新建 (可选) |

合并到已有 crate 的模块：

| 目标 crate | 来源模块 | 行数 |
|---|---|---|
| `loom-stream` | `openai_sse/` | 836 |
| `loom-types` | `tool_output_normalizer`, `active_operation` | ~694 |
| `loom-agent-patterns` | `goal_runner/`, `react_config`, `runner_common` | ~684 |
| `loom-skill` | `skill.rs` | 739 |
| `loom-memory` | `user_message/` | ~200 |
| `loom-graph` | `traits.rs` | 66 |

---

## 5. 风险控制

### 5.1 每步必做验证

```bash
# 编译检查
cargo build --workspace
# 测试检查
cargo test --workspace
# Lint 检查
cargo clippy --workspace -- -D warnings
```

### 5.2 回滚策略

每步完成后提交 git commit。如出问题可直接 `git revert`。

### 5.3 已知风险点

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| `cli_run/` 依赖很多内部模块 | Step 14 可能需要重新设计接口 | 先提取简单的，最后处理 cli_run |
| `react_config.rs` 依赖 `SkillRegistry` | Step 10 需要先完成 Step 11 | 调整顺序或定义 trait 解耦 |
| `stream_display/` 依赖终端 I/O | 测试可能需要 mock | 确保 termsize 等依赖正确传递 |
| `loom-agent` 反向依赖 `loom` | Step 18 无法完全移除 loom | 保留 facade |
| `llm/factory` 依赖 `provider` + `tier` | Step 13 需要 Step 12 先完成 | 顺序执行 |
| `lib.rs` 有 ~760 行测试代码 | 需要跟随 `cli_run` 一起移动 | Step 14 一起处理 |
| feature flag `lance` 需要透传 | 每个涉及的 crate 都要处理 | 在 Cargo.toml 中添加 `[features]` |

---

## 6. 工作量估算

### 按阶段

| 阶段 | Steps | 预计时间 | 风险 |
|---|---|---|---|
| Phase 1: 删除死代码 | 0 | 20 min | ⭐ |
| Phase 2: 独立模块提取 | 1, 2, 3, 4, 7, 8, 9 | ~7h | ⭐⭐ |
| Phase 3: 有依赖的模块 | 5, 6, 10, 11, 12 | ~5.5h | ⭐⭐ |
| Phase 4: 编排层 | 13, 14, 15, 16, 17 | ~5h | ⭐⭐⭐ |
| Phase 5: (可选) 移除 loom | 18 | 4-8h | ⭐⭐⭐⭐ |
| **总计** | | **~22h** | |

### 按 Step

| Step | 任务 | 预计时间 | 风险 |
|---|---|---|---|
| 0 | 删除死代码 | 20 min | ⭐ |
| 1 | openai_sse → loom-stream | 1h | ⭐⭐ |
| 2 | stream_display → loom-stream-display | 2h | ⭐⭐ |
| 3 | protocol → loom-protocol | 1.5h | ⭐⭐ |
| 4 | export → loom-protocol | 30 min | ⭐ |
| 5 | helve + prompts → loom-helve | 2h | ⭐⭐ |
| 6 | profile_convert → loom-helve | 30 min | ⭐ |
| 7 | user_message → loom-memory | 30 min | ⭐ |
| 8 | tool_output_normalizer → loom-types | 1h | ⭐ |
| 9 | goal_runner types → loom-agent-patterns | 1h | ⭐ |
| 10 | react_config + runner_common → loom-agent-patterns | 1.5h | ⭐⭐ |
| 11 | skill.rs → loom-skill | 1h | ⭐ |
| 12 | tier + provider + services → loom-tier | 1.5h | ⭐⭐ |
| 13 | llm/factory + title_generator → loom-llm-factory | 2h | ⭐⭐⭐ |
| 14 | cli_run + active_operation → 应用层 | 2h | ⭐⭐⭐ |
| 15 | traits.rs → loom-graph | 30 min | ⭐ |
| 16 | config/summary → loom-helve | 1h | ⭐⭐ |
| 17 | 精简 loom facade | 1h | ⭐⭐ |
| 18 | (可选) 移除 loom crate | 4-8h | ⭐⭐⭐⭐ |
| **总计** | | **~24h** | |

**建议**: Phase 1-2 为第一阶段（~7h），Phase 3 为第二阶段（~5.5h），Phase 4 为第三阶段（~5h），Phase 5 为可选第四阶段。

---

## 7. 文档更新清单

完成所有 Step 后需要更新的文档：

| 文档 | 更新内容 |
|---|---|
| `docs/crate-extraction-plan.md` | 标记为完成 |
| `loom/README.md` 或 `loom/src/lib.rs` doc comment | 更新模块列表和依赖关系 |
| `Cargo.toml` (根) workspace members | 添加新 crate |
| 各子 crate 的 `Cargo.toml` | 确保依赖正确 |
| `cli/Cargo.toml` | 更新依赖 |
| `serve/Cargo.toml` | 更新依赖 |
| `telegram-bot/Cargo.toml` | 更新依赖 |
| `loom-acp/Cargo.toml` | 更新依赖 |

---

## 附录 A: 每步提取的通用模板

以下是每个 Step 的标准操作流程：

### A.1 提取到已有 crate

```
1. 在目标 crate 中创建对应的 module 目录/文件
2. 复制源文件到目标位置
3. 修改 import 路径:
   - crate::xxx → loom_xxx::xxx
   - super::yyy → crate::yyy (如果还在同一 crate)
4. 在目标 crate 的 lib.rs 中添加 pub mod xxx;
5. 在目标 crate 的 Cargo.toml 中添加缺失的依赖
6. 在 loom/src/xxx/mod.rs 中替换为 re-export:
   pub use loom_xxx::yyy::*;
7. 删除 loom/src/xxx/ 下的子文件
8. cargo build -p loom_xxx && cargo build -p loom
9. cargo test -p loom_xxx && cargo test -p loom
10. git add -A && git commit -m "refactor: extract xxx to loom-xxx"
```

### A.2 提取到新 crate

```
1. 创建新目录 loom-xxx/
2. 创建 Cargo.toml (version.workspace, edition.workspace)
3. 复制源文件到 src/
4. 创建 src/lib.rs (pub mod ...)
5. 修改 import 路径
6. 在根 Cargo.toml 的 workspace.members 中添加 "loom-xxx"
7. 在 loom/Cargo.toml 中添加 loom-xxx = { path = "../loom-xxx" }
8. 将 loom/src/xxx/ 替换为 re-export 薄壳
9. cargo build -p loom-xxx && cargo build -p loom
10. cargo test -p loom-xxx && cargo test -p loom
11. git add -A && git commit -m "refactor: extract xxx to loom-xxx"
```
