# RFC: ACP 模式 Background Review

> 状态：Draft
> 日期：2025-08-19
> 范围：`loom` crate, `cli` crate, `loom-acp` crate

---

## 1. 背景与动机

Background review（后台审查）在每次对话结束后异步执行，提取记忆更新和技能进化。当前实现仅覆盖 CLI 模式（`cli/src/run/agent.rs:336-352`），ACP 模式（IDE 集成场景）完全缺失。

**问题**：
- ACP 是长期驻留进程（IDE 内运行），用户可能持续对话数小时，但没有任何 review 触发
- 记忆和技能进化仅在使用 CLI 时生效，ACP 用户的体验持续退化
- review 核心逻辑全部定义在 `cli` crate，`loom-acp` 无法复用

**目标**：将 background review 的触发能力扩展到 ACP 模式，使用 `tokio::spawn` 在后台线程执行，不阻塞 ACP 的 prompt 响应。

---

## 2. 现状分析

### 2.1 CLI 模式触发链路

```
run_cli_turn()
  → run_agent_with_options()
  → completion_reply() → (reply, stop_reason)
  → if EndTurn && !reply.is_empty():
      build_background_config_from_opts(opts)
      spawn_background_review(config, session_content, session_id)
      → tokio::spawn(async move { run_background_review_workflow(...) })
      → PendingReviewRegistry::push(handle)
```

**关键文件**：
- `cli/src/run/background_review.rs` — config struct、spawn、workflow、registry（全部 cli-private）
- `cli/src/run/agent.rs:336-352` — 触发点
- `cli/src/main.rs` — 进程退出时调用 `wait_for_pending_reviews()`

### 2.2 ACP 模式执行链路

```
AcpAgent::handle_prompt()
  → build RunOptions
  → run_agent_with_options(&opts, &RunCmd::React, on_event).await
  → finish_prompt()
  → return PromptResponse
  // ← 无 review 触发
```

**关键文件**：`loom-acp/src/agent.rs:824`

**返回类型差异**：
- CLI 使用 `completion_reply()` 拆解为 `(reply, reasoning, RunStopReason)`
- ACP 直接匹配 `RunCompletion::Finished(AgentRunResult)`（`AgentRunResult` 包含 `reply: String` 和 `reasoning_content: Option<String>`）

### 2.3 完整依赖图

```
cli/src/run/background_review.rs
  ├── cli/src/run/review_agent_loop.rs
  │   ├── cli/src/run/review_prompts.rs          (纯常量，无外部依赖)
  │   ├── cli/src/run/review_tools.rs
  │   │   ├── cli/src/run/memory.rs              (MemoryStore, MemoryFile)
  │   │   ├── cli/src/run/skill_registry.rs       (SkillRegistry, SkillContent, etc.)
  │   │   ├── cli/src/run/security.rs             (validate_skill_create, validate_skill_path)
  │   │   └── cli/src/run/curator.rs              (Curator, CuratorConfig)
  │   └── loom crate
  │       ├── loom::llm::{ChatOpenAICompat, LlmClient, ModelEntry}
  │       └── loom::message::{AssistantToolCall, Message}
  ├── cli/src/run/memory.rs                       (MemoryStore)
  ├── cli/src/run/skill_registry.rs               (SkillRegistry)
  ├── cli/src/run/observability.rs                (ObservabilityStore)
  ├── cli/src/run/curator.rs                      (Curator)
  ├── cli/src/run/evolution_trigger.rs
  │   ├── cli/src/run/skill_registry.rs
  │   ├── loom::llm::LlmClient
  │   ├── loom::message::Message
  │   └── loom-evolution crate                    (EvolutionConfig, GepaOptimizer)
  ├── cli/src/review_history.rs                   (ReviewHistory, ReviewRecord)
  ├── loom crate
  │   ├── loom::llm::{LlmFactory, ModelEntry}
  │   └── loom::cli_run::{RunOptions, RunCompletion, AgentRunResult}
  └── config crate                                (config::home::loom_home)
```

### 2.4 各模块外部依赖详情

| 模块 | `loom` crate | `config` crate | `loom-evolution` crate | `cli` 内部 |
|------|-------------|---------------|----------------------|-----------|
| `MemoryStore` | - | `config::home::loom_home` | - | - |
| `SkillRegistry` | - | `config::home::loom_home` | - | - |
| `Curator` | - | - | - | `SkillRegistry` |
| `EvolutionTrigger` | `loom::llm::LlmClient`, `loom::message::Message` | - | `loom_evolution::*` | `SkillRegistry` |
| `ObservabilityStore` | - | `config::home::loom_home` | - | - |
| `ReviewHistory` | - | `config::home::loom_home` | - | - |
| `ReviewToolExecutor` | - | - | - | `MemoryStore`, `SkillRegistry`, `Curator`, `security` |
| `AgentReviewRunner` | `loom::llm::*`, `loom::message::*` | - | - | `ReviewToolExecutor`, `review_prompts` |
| `BackgroundReviewConfig` | `loom::llm::ModelEntry` | - | - | `CuratorConfig`, `EvolutionTriggerConfig` |
| `build_background_config_from_opts` | `loom::cli_run::RunOptions`, `loom::llm::ModelEntry`, `loom::provider::*`, `loom::tier::*` | - | - | - |
| `security.rs` | - | - | - | `SkillContent` |

### 2.5 `loom` crate 现有依赖兼容性

`loom/Cargo.toml` 已包含 review 所需的全部依赖：
- `tokio` ✅
- `serde` / `serde_json` ✅
- `chrono` ✅
- `rusqlite` ✅（SkillRegistry 不需要，但其他模块已用）
- `tracing` ✅
- `async-trait` ✅
- `serde_yaml` — **未包含**，SkillRegistry 需要
- `loom-evolution` — **未包含**，EvolutionTrigger 需要

---

## 3. 方案设计

### 3.1 核心策略

将 review 全套逻辑从 `cli` crate 提取到 `loom/src/background_review/`（新模块），`cli` 和 `loom-acp` 均通过 `loom::background_review` 调用。

### 3.2 拆分方案：两层架构

经过依赖分析，发现有一个关键决策点：**EvolutionTrigger 依赖 `loom-evolution` crate**。

`loom-evolution` 是一个较重的依赖（GEPA 优化器、数据集、回归测试等），将其加入 `loom` crate 会增加核心库体积。

**选择方案**：使用 `loom` crate feature flag 控制 evolution 功能，基础 review 功能无条件可用。

```
loom/Cargo.toml:
  [features]
  lance = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema"]
  review-evolution = ["dep:loom-evolution"]    # 新增

  [dependencies]
  serde_yaml = "0.9"                           # 新增（SkillRegistry 需要）
  loom-evolution = { path = "../loom-evolution", optional = true }  # 新增
```

### 3.3 模块结构

```
loom/src/background_review/
├── mod.rs                  # BackgroundReviewConfig, BackgroundReviewHandle, re-exports
├── spawn.rs                # spawn_background_review(), 输出回调抽象
├── workflow.rs             # run_background_review_workflow(), run_background_review_inner()
├── registry.rs             # PendingReviewRegistry, PENDING_REVIEWS, wait_for_pending_reviews()
├── prompts.rs              # COMBINED_REVIEW_PROMPT, MEMORY_REVIEW_PROMPT, SKILL_REVIEW_PROMPT
├── agent_loop.rs           # AgentReviewRunner, AgentReviewConfig, ReviewMode, AgentReviewResult
├── tools.rs                # ReviewToolExecutor, ReviewAction, review_tool_specs()
├── history.rs              # ReviewHistory, ReviewRecord
├── curator.rs              # Curator, CuratorConfig, CuratorReport, OverlapPair
├── curator_trigger.rs      # run_curator_if_needed()
├── observability.rs        # ObservabilityStore, EvolutionTrackerEntry, EvolutionEvent
└── evolution.rs            # EvolutionTrigger, EvolutionTriggerConfig (feature-gated)
```

同时将基础存储模块也移入 `loom`：
```
loom/src/background_review/
├── memory.rs               # MemoryStore, MemoryFile, MemoryConfig, MemoryError
├── skill_registry.rs       # SkillRegistry, SkillContent, SkillMeta, SkillError, Lifecycle, Source
└── security.rs             # validate_skill_create, validate_skill_path, validate_memory_content
```

### 3.4 输出回调抽象

CLI 和 ACP 对 review 完成后的通知方式不同（CLI 用 `eprintln!`，ACP 用 `tracing::info!`）。通过回调抽象解耦：

```rust
// loom/src/background_review/spawn.rs

pub type ReviewOutputFn = Arc<dyn Fn(&str) + Send + Sync>;

pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
    on_output: Option<ReviewOutputFn>,
) { ... }
```

- **CLI 调用**：传入 `Some(Arc::new(|msg| eprintln!("{}", msg)))`
- **ACP 调用**：传入 `None`（默认仅 `tracing::info!`）

### 3.5 `cli` crate 改造

`cli/src/run/background_review.rs` 精简为 thin wrapper：

```rust
// cli/src/run/background_review.rs (改造后)
pub use loom::background_review::{
    BackgroundReviewConfig, BackgroundReviewHandle,
    build_background_config_from_opts,
    wait_for_pending_reviews,
};

pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
) {
    let on_output = std::sync::Arc::new(|msg: &str| {
        eprintln!("\n📚 {}", msg);
    });
    loom::background_review::spawn_background_review(
        config, session_content, session_id, Some(on_output),
    );
}
```

同时更新 `cli/src/run/mod.rs` 和 `cli/src/run/agent.rs` 中的 import 路径。

其余被移动的文件保留原文件路径但内容替换为 re-export：
```rust
// cli/src/run/memory.rs (改造后)
pub use loom::background_review::memory::*;
```

### 3.6 `loom-acp` crate 触发点

在 `loom-acp/src/agent.rs` 的 `handle_prompt` 返回前，添加 review 触发：

```rust
// loom-acp/src/agent.rs handle_prompt() 中，原 824 行附近

let result = run_agent_with_options(&opts, &RunCmd::React, on_event).await;
self.sessions.finish_prompt(&key, cancellation.generation());

if let Ok(RunCompletion::Finished(ref run_result)) = &result {
    if !run_result.reply.is_empty() {
        let config = loom::background_review::build_background_config_from_opts(&opts);
        if config.enabled {
            let session_id = opts.thread_id.clone()
                .unwrap_or_else(|| format!("acp-{}", args.session_id));
            let user_msg = match &opts.message {
                loom::UserContent::Text(t) => t.clone(),
                _ => String::new(),
            };
            let session_content = format!("User: {}\n\nAssistant: {}", user_msg, run_result.reply);
            loom::background_review::spawn_background_review(
                config, session_content, session_id, None,
            );
        }
    }
}

tokio::time::sleep(std::time::Duration::from_millis(100)).await;
// ... 原有 match result 逻辑不变
```

### 3.7 `loom-acp` 优雅关闭

ACP 是长期驻留进程，在 shutdown 时需等待进行中的 review 完成。

```rust
// loom-acp/src/main.rs (shutdown handler 中添加)
async fn shutdown() {
    let pending = loom::background_review::wait_for_pending_reviews().await;
    if pending > 0 {
        tracing::info!("Waited for {} background review(s) to complete", pending);
    }
}
```

### 3.8 ACP 与 CLI 行为差异

| 行为 | CLI | ACP |
|------|-----|-----|
| 触发条件 | `EndTurn` && `!reply.is_empty()` | `RunCompletion::Finished` && `!reply.is_empty()` |
| 执行方式 | `tokio::spawn`（后台） | 相同 |
| 完成通知 | `eprintln!` 到终端 | 仅 `tracing::info!`（通过 `on_output=None`） |
| 进程退出等待 | `main.rs` 显式调用 `wait_for_pending_reviews()` | shutdown handler 中调用 |
| session_id 格式 | `auto-{timestamp}` 或 `thread_id` | `acp-{session_id}` 或 `thread_id` |
| session 持久化 | `FileSessionStore` 写入 | 由 ACP session 管理器负责 |
| 回复来源 | `completion_reply(result).0` 拆包 | `run_result.reply` 直接访问 |
| curator/evolution | 每次触发 | 相同（已有 interval 保护） |

---

## 4. 详细文件变更清单

### 4.1 新建文件（`loom/src/background_review/`）

| 文件 | 来源 | 行数 | 说明 |
|------|------|------|------|
| `mod.rs` | `cli/run/background_review.rs:24-62` + `cli/run/background_review.rs:296-320` | ~80 | Config struct + build fn + re-exports |
| `spawn.rs` | `cli/run/background_review.rs:117-155` | ~50 | spawn + on_output 回调 |
| `workflow.rs` | `cli/run/background_review.rs:159-290, 296-339` | ~170 | workflow + resolve_model + resolve_session_model |
| `registry.rs` | `cli/run/background_review.rs:64-113, 341-345` | ~70 | PendingReviewRegistry + wait |
| `prompts.rs` | `cli/run/review_prompts.rs` | ~165 | 纯常量，直接搬移 |
| `agent_loop.rs` | `cli/run/review_agent_loop.rs` | ~195 | AgentReviewRunner + build_review_agent_client |
| `tools.rs` | `cli/run/review_tools.rs` | ~430 | ReviewToolExecutor + tool specs |
| `history.rs` | `cli/review_history.rs` | ~60 | ReviewHistory, ReviewRecord |
| `curator.rs` | `cli/run/curator.rs` | ~335 | Curator + tests |
| `curator_trigger.rs` | `cli/run/background_review.rs:347-375` | ~40 | run_curator_if_needed |
| `observability.rs` | `cli/run/observability.rs` | ~183 | ObservabilityStore + all event types |
| `evolution.rs` | `cli/run/evolution_trigger.rs` | ~138 | EvolutionTrigger（feature-gated） |
| `memory.rs` | `cli/run/memory.rs` | ~320 | MemoryStore + tests |
| `skill_registry.rs` | `cli/run/skill_registry.rs` | ~470 | SkillRegistry + tests |
| `security.rs` | `cli/run/security.rs` | ~182 | validation functions |

### 4.2 修改文件

| 文件 | 变更 |
|------|------|
| `loom/src/lib.rs` | 添加 `pub mod background_review;` |
| `loom/Cargo.toml` | 添加 `serde_yaml`，添加 optional `loom-evolution`，添加 feature `review-evolution` |
| `cli/src/run/background_review.rs` | 精简为 re-export + CLI `eprintln!` 包装 |
| `cli/src/run/agent.rs` | 更新 import 路径（`super::background_review::*` → `loom::background_review::*`），或保持不变（re-export 兼容） |
| `cli/src/run/mod.rs` | 各子模块改为 re-export（`pub use loom::background_review::memory::*;`） |
| `cli/src/run/review_prompts.rs` | 改为 re-export |
| `cli/src/run/review_agent_loop.rs` | 改为 re-export |
| `cli/src/run/review_tools.rs` | 改为 re-export |
| `cli/src/run/curator.rs` | 改为 re-export |
| `cli/src/run/evolution_trigger.rs` | 改为 re-export |
| `cli/src/run/observability.rs` | 改为 re-export |
| `cli/src/run/memory.rs` | 改为 re-export |
| `cli/src/run/skill_registry.rs` | 改为 re-export |
| `cli/src/run/security.rs` | 改为 re-export |
| `cli/src/review_history.rs` | 改为 re-export |
| `loom-acp/src/agent.rs` | 添加 review 触发代码块（~15 行） |
| `loom-acp/src/main.rs` | 添加 shutdown handler 中的 `wait_for_pending_reviews()` |

### 4.3 `cli` crate 中 `loom-evolution` 其他使用点

`loom-evolution` 在 `cli` crate 中还被以下文件使用（非 review 模块），这些文件**不移动**：
- `cli/src/run/session_store.rs:94-103` — `SessionStore` impl for `FileSessionStore`
- `cli/src/subcommands.rs:393-443` — evolution 相关子命令

这些文件继续通过 `cli/Cargo.toml` 的 `loom-evolution` 依赖访问。`loom` crate 中的 `evolution.rs` 通过 feature flag 独立引入。

---

## 5. 实施计划

### Phase 1: 基础存储层移动

将无外部依赖的基础模块移入 `loom/src/background_review/`：

1. `memory.rs` → `loom/src/background_review/memory.rs`
2. `security.rs` → `loom/src/background_review/security.rs`
3. `skill_registry.rs` → `loom/src/background_review/skill_registry.rs`
4. 在 `loom/src/lib.rs` 添加 `pub mod background_review;`
5. 在 `loom/Cargo.toml` 添加 `serde_yaml = "0.9"`
6. 原 `cli` 文件改为 re-export
7. **验证**：`cargo build -p loom -p cli`

### Phase 2: Review 核心逻辑移动

将 review 直接相关模块移入：

1. `prompts.rs` → `loom/src/background_review/prompts.rs`
2. `tools.rs` → `loom/src/background_review/tools.rs`
3. `agent_loop.rs` → `loom/src/background_review/agent_loop.rs`
4. `curator.rs` → `loom/src/background_review/curator.rs`
5. `observability.rs` → `loom/src/background_review/observability.rs`
6. `history.rs` → `loom/src/background_review/history.rs`
7. 原 `cli` 文件改为 re-export
8. **验证**：`cargo build -p loom -p cli`

### Phase 3: Workflow 与 Spawn 移动

将调度层移入：

1. 创建 `loom/src/background_review/mod.rs`（`BackgroundReviewConfig`, `BackgroundReviewHandle`, `build_background_config_from_opts`）
2. 创建 `loom/src/background_review/workflow.rs`（`run_background_review_workflow`, `run_background_review_inner`, `resolve_review_model`, `resolve_session_model`）
3. 创建 `loom/src/background_review/registry.rs`（`PendingReviewRegistry`, `PENDING_REVIEWS`, `wait_for_pending_reviews`）
4. 创建 `loom/src/background_review/spawn.rs`（`spawn_background_review` with `on_output` callback）
5. 创建 `loom/src/background_review/curator_trigger.rs`（`run_curator_if_needed`）
6. `cli/src/run/background_review.rs` 精简为 re-export + CLI 包装
7. **验证**：`cargo build -p loom -p cli`，CLI 功能回归测试

### Phase 4: Evolution 移动（Feature-gated）

1. 在 `loom/Cargo.toml` 添加 `loom-evolution = { path = "../loom-evolution", optional = true }` 和 feature `review-evolution`
2. `evolution.rs` → `loom/src/background_review/evolution.rs`（`#[cfg(feature = "review-evolution")]`）
3. `BackgroundReviewConfig.evolution_enabled` 在无 feature 时编译为 `false`
4. `cli/Cargo.toml` 添加 `loom = { ..., features = ["review-evolution"] }`
5. **验证**：`cargo build -p loom -p cli`

### Phase 5: ACP 集成

1. `loom-acp/src/agent.rs` 添加 review 触发代码块
2. `loom-acp/src/main.rs` 添加 shutdown handler
3. **验证**：`cargo build -p loom-acp`，ACP 集成测试

### Phase 6: 测试与清理

1. 确认所有 `cli` 中的 review 相关测试在 `loom` 中通过
2. 确认 CLI `cargo test -p cli` 全部通过
3. 确认 `cargo test -p loom` 全部通过
4. 移除 `cli` 中的冗余代码（如果 re-export 完整，可删除原文件内容）

---

## 6. 回归测试策略

### 6.1 必须通过的测试

```bash
cargo build -p loom -p cli -p loom-acp
cargo test -p loom
cargo test -p cli
cargo test -p loom-acp
```

### 6.2 功能回归验证

- CLI `loom run "hello"` 后观察 background review 日志输出
- CLI `loom run` 多轮对话后检查 memory/skills 更新
- ACP 通过 IDE 发送消息后检查 `~/.loom/data/memory/` 和 `~/.loom/data/skills/` 是否有更新

### 6.3 关键测试用例

| 场景 | 验证点 |
|------|--------|
| CLI 短会话（< min_session_chars） | review 跳过，日志显示 "session too short" |
| CLI 正常会话 | review 执行，eprintln 输出 summary |
| ACP 正常会话 | review 执行，仅 tracing 日志 |
| ACP 取消会话（RunCompletion::Cancelled） | review 不触发 |
| 并发 ACP 多会话 | 多个 review task 并行执行 |
| ACP 进程退出 | wait_for_pending_reviews() 等待完成 |

---

## 7. 风险与缓解

| 风险 | 影响 | 概率 | 缓解 |
|------|------|------|------|
| Curator/Evolution 与 CLI 实例并发冲突 | 文件写入冲突 | 低 | Curator 已有 interval 检查（86400s），同一时间只有一个会执行 |
| `MemoryStore::default_path()` 路径依赖 | ACP 进程需路径可写 | 无 | 当前已满足 |
| 大量 ACP 对话并发触发 review | LLM API 限流 / 资源消耗 | 中 | 已有 `min_session_chars` 过滤 + `max_iterations` 限制；可后续添加并发上限 |
| `loom` crate 编译时间增加 | 开发体验 | 低 | `serde_yaml` 是轻量依赖；evolution 通过 feature gate 可选 |
| `cli` crate re-export 链断裂 | 编译错误 | 低 | 逐步移动，每步验证编译 |
| `security.rs` 中的 `Severity` type 暴露到 `loom` | API 污染 | 低 | `Severity` 是 review 专属类型，放在 `background_review` 模块内合理 |

---

## 8. 备选方案

### 8.1 备选 A：不移动，在 ACP 中直接引用 `cli` crate

让 `loom-acp` 依赖 `cli` crate，直接使用其 review 模块。

**问题**：`cli` crate 依赖大量 CLI 专属库（clap, panel_format, spinner 等），会严重污染 ACP 依赖树。**否决**。

### 8.2 备选 B：仅抽取接口 trait，不移动实现

在 `loom` crate 定义 `BackgroundReviewTrigger` trait，`cli` 和 `loom-acp` 各自实现。

**问题**：review 核心逻辑（review agent loop, tool executor, prompts）完全重复。**否决**。

### 8.3 备选 C：将 review 作为独立 crate

创建 `loom-review` crate，`cli` 和 `loom-acp` 都依赖它。

**优点**：依赖隔离最彻底。
**缺点**：多一个 crate 维护；review 模块依赖 `loom` crate 的 `LlmClient`/`Message`/`RunOptions`，创建循环依赖风险（`loom-review` → `loom`，`cli` → `loom` + `loom-review`）。**可接受但非首选**。

---

## 9. 验收标准

- [ ] `loom::background_review` 模块可被 `cli` 和 `loom-acp` 共同使用
- [ ] CLI 模式 review 行为不变（回归测试通过）
- [ ] ACP 模式在 `RunCompletion::Finished` 后自动触发 background review
- [ ] ACP review 不阻塞 prompt 响应（`tokio::spawn` 后立即返回）
- [ ] ACP review 完成后仅输出 tracing 日志，不向 IDE 客户端发送通知
- [ ] `cargo build -p loom -p cli -p loom-acp` 通过
- [ ] `cargo test -p loom -p cli -p loom-acp` 通过
- [ ] Evolution 功能通过 feature flag 可选控制
