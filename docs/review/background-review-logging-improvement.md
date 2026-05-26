# Background Review 日志分析与改进方案

## 现状分析

### 当前日志架构

| 层级 | 输出方式 | 位置 | 问题 |
|------|---------|------|------|
| **CLI 层** | `eprintln!("\n📚 {}")` | `cli/src/run/background_review.rs:17` | 硬编码 emoji，无法控制开关，污染 stderr |
| **Workflow 层** | `tracing::info/warn/error` | `workflow.rs` 多处 | 只有纯文本，无结构化字段，难以程序化消费 |
| **Agent Loop 层** | `tracing::info` | `agent_loop.rs:86,128` | 迭代日志过于简略，缺少 LLM 耗时/token/参数信息 |
| **Curator 层** | `tracing::info/warn` | `curator.rs:123,134,162` | 无 dry-run 时的诊断信息 |

### 核心问题

**1. CLI 输出层：eprintln 硬编码**
```rust
// cli/src/run/background_review.rs:16-18
let on_output: ReviewOutputFn = std::sync::Arc::new(|msg: &str| {
    eprintln!("\n📚 {}", msg);
});
```
- 只在有 action > 0 时输出（`workflow.rs:127`），用户无法知道 review 是否执行/跳过/失败
- emoji 在非 TTY 环境下（管道、文件重定向、CI）乱码
- 无法通过 `--quiet` 或配置关闭
- 混入 stderr，与程序错误信息混杂

**2. tracing 日志缺少结构化字段**
```rust
// 当前：纯文本 info!
info!("Background review completed: {} ({} actions, {}ms)", summary, action_count, duration_ms);
info!("Review tool call: {} -> {}", tc.name, if result["success"].as_bool().unwrap_or(false) { "ok" } else { "err" });
```
- summary、action_count、duration_ms 是内联文本，无法被日志系统（如 Loki/Elasticsearch）提取
- 缺少 session_id、model、iteration 等关键上下文
- 工具调用只记录 name + ok/err，不记录参数摘要和返回值

**3. 迭代级别日志缺失**
- 无 LLM 调用耗时
- 无 token 使用量（prompt_tokens / completion_tokens）
- 无每轮 summary（LLM 说了什么）
- max_iterations 命中时无法知道 LLM 最后在做什么

**4. 跳过/失败路径信息不足**
```rust
info!("Skipping background review: session too short ({} chars)", session_content.len());
info!("Skipping background review: no API credentials configured");
```
- 跳过原因只写 tracing（文件），用户在 CLI 上完全看不到
- 失败路径只有 `error!("Background review failed: {} ({}ms)")`, 缺少错误分类

**5. 无异步生命周期追踪**
- spawn 后没有 "started" 事件
- 进行中没有进度指示
- 等待完成时只有 "Waiting for N..." / "All N completed"，无逐个完成通知

---

## 改进方案

### 方案 A：结构化 tracing + 分层输出（推荐）

#### 设计原则
- **结构化**：所有 tracing 日志使用字段，不内联
- **分层**：CLI 输出（用户可见）vs tracing 日志（运维可观测）
- **可控**：CLI 输出受 quiet/verbose 控制；tracing 受 RUST_LOG 控制

#### 1. 引入 ReviewEvent 枚举替代 ReviewOutputFn

```rust
/// Review 事件类型
pub enum ReviewEvent {
    Started { session_id: String },
    Skipped { reason: String, detail: String },
    Progress { iteration: u32, tool: String, success: bool },
    Completed { summary: String, actions: usize, duration_ms: u64 },
    Failed { error: String, duration_ms: u64 },
}

pub type ReviewEventFn = Arc<dyn Fn(ReviewEvent) + Send + Sync>;
```

CLI 层根据事件类型选择性展示：

```rust
let on_event: ReviewEventFn = Arc::new(|event| match event {
    ReviewEvent::Started { .. } => {
        tracing::info!("background review started");
    }
    ReviewEvent::Skipped { reason, detail } => {
        if verbose {
            eprintln!("  Review skipped: {} ({})", reason, detail);
        }
    }
    ReviewEvent::Progress { iteration, tool, success } => {
        tracing::info!(iteration, tool, success, "review progress");
    }
    ReviewEvent::Completed { summary, actions, duration_ms } => {
        if *actions > 0 {
            eprintln!("\n📚 {} ({}ms)", summary, duration_ms);
        }
    }
    ReviewEvent::Failed { error, .. } => {
        eprintln!("\n⚠ Review failed: {}", error);
    }
});
```

#### 2. 结构化 tracing 字段

```rust
// 替换当前纯文本 info!
info!(
    duration_ms = duration_ms,
    action_count = action_count,
    memory_count = memory_count,
    skill_count = skill_count,
    iterations = result.iterations,
    summary = %summary,
    "background review completed"
);
```

#### 3. Agent loop 结构化迭代日志

```rust
// agent_loop.rs - 每轮迭代
info!(
    iteration = iterations,
    tool_count = response.tool_calls.len(),
    llm_duration_ms = llm_elapsed.as_millis() as u64,
    "review iteration completed"
);

// 每个工具调用
info!(
    iteration = iterations,
    tool = %tc.name,
    success = result["success"].as_bool().unwrap_or(false),
    "review tool executed"
);
```

#### 4. CLI 输出：TTY 检测 + quiet 控制

```rust
let on_output: ReviewOutputFn = Arc::new(move |msg: &str| {
    if !quiet && atty::is(atty::Stream::Stderr) {
        eprintln!("\n📚 {}", msg);
    }
});
```

#### 5. 跳过/失败路径增强

```rust
// 统一跳过日志
info!(
    skip_reason = "session_too_short",
    session_chars = session_content.len(),
    min_chars = config.min_session_chars,
    "background review skipped"
);

// 失败日志增加错误分类
error!(
    duration_ms = duration_ms,
    error = %e,
    error_category = categorize_review_error(&e),  // "llm_call" / "tool_exec" / "io" / "config"
    "background review failed"
);
```

### 方案 B：专用 ReviewLogger trait（轻量替代）

如果不想改 ReviewOutputFn 签名，可以在 loom crate 内部加一个专用 logger：

```rust
// loom/src/background_review/logger.rs
pub trait ReviewLogger: Send + Sync {
    fn started(&self, session_id: &str, model: &str);
    fn skipped(&self, reason: &str, detail: &str);
    fn iteration(&self, iter: u32, tools: &[String], duration_ms: u64);
    fn tool_call(&self, tool: &str, success: bool, detail: Option<&str>);
    fn completed(&self, summary: &str, actions: usize, duration_ms: u64);
    fn failed(&self, error: &str, duration_ms: u64);
}

/// 默认实现：全部走 tracing
pub struct TracingReviewLogger;
impl ReviewLogger for TracingReviewLogger {
    fn started(&self, session_id: &str, model: &str) {
        tracing::info!(session_id, model, "review started");
    }
    fn completed(&self, summary: &str, actions: usize, duration_ms: u64) {
        tracing::info!(summary, actions, duration_ms, "review completed");
    }
    // ...
}

/// CLI 实现：tracing + 有选择性 stderr
pub struct CliReviewLogger { verbose: bool }
impl ReviewLogger for CliReviewLogger {
    fn completed(&self, summary: &str, actions: usize, duration_ms: u64) {
        tracing::info!(summary, actions, duration_ms, "review completed");
        if *actions > 0 {
            eprintln!("\n📚 {} ({}ms)", summary, duration_ms);
        }
    }
    // ...
}
```

---

## 实施优先级

| 优先级 | 改动 | 工作量 | 收益 |
|--------|------|--------|------|
| **P0** | tracing 结构化字段（所有 info!/error!） | 小 | 日志可搜索、可聚合 |
| **P0** | ReviewEvent 枚举替代 ReviewOutputFn | 中 | 完整生命周期可观测 |
| **P1** | CLI 输出：TTY 检测 + quiet 控制 | 小 | 不污染管道/CI |
| **P1** | 跳过/失败事件通知到 CLI | 小 | 用户知道发生了什么 |
| **P2** | Agent loop LLM 耗时/token 记录 | 中 | 性能诊断能力 |
| **P2** | `#[tracing::instrument]` span 包裹 | 小 | 自动关联父子 span |

### 建议实施路径

1. **先做 P0**：所有现有 `info!`/`error!` 改为结构化字段形式
2. **再做 P0+P1**：引入 `ReviewEvent` 枚举，替换 `ReviewOutputFn`，CLI 层按事件类型处理
3. **最后 P2**：LLM 耗时埋点和 span instrumentation
