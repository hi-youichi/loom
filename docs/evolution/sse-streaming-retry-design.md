# SSE 流式重试与超时：设计文档

- **状态**：Draft v0.1
- **作者**：Loom / LLM runtime
- **适用范围**：`foundation/llm` crate 的 `openai_compat` 子模块
- **关联 issue**：mid-stream SSE 故障 `0 tok / 0ms` 误判为 non-retryable

---

## 1. 背景与问题

### 1.1 现象

`ChatOpenAICompat::invoke_stream`（`foundation/llm/src/client/openai_compat/llm_client.rs:286-535`）在 SSE 流中途发生传输层错误时，**直接 `audit_error` 抛 `LlmError::InvokeFailed`**，不重试、不区分瞬时/永久、不上报 retry 信号。日志形如：

```
OpenAI-compat stream body: error decoding response body
```

### 1.2 根因

- `'sse: loop`（`llm_client.rs:328-336`）对 `res.chunk()` 返回的 `Err(e)` **零防御**：
  - 没有 `is_retryable_reqwest_error` 分类
  - 没有外层重试包络
  - 错误被无脑转成 `LlmError::InvokeFailed`
- `send_with_retry`（`llm_client.rs:122-176`）只重试**握手**（`send_post` 失败 + 可重试 5xx），SSE 主体读取不在它的覆盖范围
- `LlmError`（`foundation/llm/src/error.rs:19-31`）三个变体（`InvokeFailed` / `EmptyResponse` / `Cancelled`）**不携带 retryable 语义**。上游 classifier（Hermes `conversation_loop.py:3022-3122`）拿到不带分类的字符串，**默认打 `non-retryable` 标签**，直奔失败
- 字符串 `"error decoding response body"` 来自 `reqwest` 的 `hyper::Error(IncompleteMessage)` —— 对端在响应头/body 没齐时就关了连接。`loom_http_retry::is_retryable_reqwest_error` 已识别为 transient，但**没有调用**

### 1.3 后果

- **丢消息**：长 prompt 流式输出时偶发小中断，整个 graph 节点失败，user 必须手动重发
- **雪崩**：单次 provider 抖动 → graph 节点失败 → 上游 executor 重试整个节点（如果开启）→ 再次触发 0-token 失败循环
- **可见性差**：audit log 只能看到一行字符串，看不出是 transport 重置 / idle timeout / TLS 失败 / 5xx 任何一种

### 1.4 与行业惯例的差距

跨 LangChain PR #36949、Gloo AI 文档、OpenCode 实时实现、DGX Code "brain layer"、Mengboy 生产指南、AI/TLDR、Claude field notes 七份来源对齐后，Loom 当前缺：

| 实践 | 行业基线 | Loom 当前 |
|---|---|---|
| per-chunk idle timeout | 8-120s（LangChain 120s / Mengboy 8s） | 0（永远等） |
| first_token timeout | 3s（Mengboy） | 无 |
| SSE 流重试 | 2-3 次，0.3-0.5s base，4-8s cap，jitter | **0 次**（bug 根源） |
| 重试前 retryable 分类 | 必备 | 仅 `is_retryable_reqwest_error` 一项 |
| 4xx 不重试 | 必备 | 走 `InvokeFailed` 字符串，无语义 |
| backoff jitter | 20-30% | `retry.rs:32-37` **无 jitter** |
| tool_call 已发不重试 | 强共识 | 会复用 `tool_calls_acc`，会污染 |
| request_id 跨重试复用 | 标准 | 已具备（`llm_client.rs:291`） |
| 错误携带 transient 标记 | 标准 | 无（`InvokeFailed(String)` 一锅端） |

---

## 2. 目标与非目标

### 2.1 目标

1. **G1**：SSE mid-stream 传输层失败时，**有界重试 + 整次重发**（0-token case），最多 3 次，base 500ms，cap 4s，加 30% jitter
2. **G2**：per-chunk idle timeout **8s**；first_token timeout **3s**（layered timeout 三件套）
3. **G3**：`LlmError` 携带 `Retryable { transient: bool }` 语义；上游 classifier 看 `is_retryable()` 即可路由
4. **G4**：tool_call 已部分发出时**禁止整次重发**（safety valve）
5. **G5**：可观测 —— 每次重试 attempt 都打 `tracing::warn!` 带 `attempt / error / request_id / trace_id`
6. **G6**：**完全向后兼容**：默认配置下行为变更对外不可见（无新增 public API 破坏）

### 2.2 非目标（明确不做）

- **N1**：resumable SSE / `Last-Event-ID` 续流 —— OpenAI/Anthropic 均不输出 `id:` 字段，落地成本不匹配收益
- **N2**：`stream_chunk_timeout` 做成 env 全局可配（LangChain 风格）—— 本期硬编码 8s/3s，下期按 provider config 开放
- **N3**：sink 去重 / idempotency key —— 改动面太大（所有 sink 实现），本期接受 partial delta 视觉跳变
- **N4**：circuit breaker / fallback brain（DGX 风格）—— 属于上游 executor 职责，不在 LLM client 层
- **N5**：Anthropic 专属 `content_block_delta` 索引续传 —— 不通用
- **N6**：把 `send_with_retry` 自身的 `COMPAT_RETRY_MAX_RETRIES=20` 调小 —— 握手重试和流重试是独立维度，本期不动

---

## 3. 行业惯例参考

按相关度排序（最贴近 Loom 现状的放前面）：

| 来源 | 关键做法 | URL |
|---|---|---|
| LangChain PR #36949（2026-01） | `stream_chunk_timeout=120s` + `StreamChunkTimeoutError`，**只检测不重试** | https://github.com/langchain-ai/langchain/pull/36949 |
| Gloo AI Completions streaming | `error.retryable` / `error.fault` / `error.code` 三分类；`content_filter` 永不重试 | https://docs.gloo.com/best-practices/completions-streaming-failures |
| OpenCode `14e0b9b` (websocket) | `max_retries=5, initial_delay=0.5, max_delay=8.0`；first event 后失败不重发 partial | https://github.com/anomalyco/opencode/commit/14e0b9b17f886c9157c92e1b98caca5a40d21797 |
| DGX Code "brain layer" | per-request retry + per-chunk idle timeout + circuit breaker + fallback 四件套；"Do not re-send the whole prompt from turn-0" | https://wiki.charleschen.ai/ai/processed/wiki/llm-core/cli/techniques/streaming-retry-and-fallback-brain |
| Mengboy production | 5 层超时：dial 1s / tls 1s / first_token 3s / idle 8s / total 45s；重试 2-3 次 300ms + 20-30% jitter | https://www.mfun.ink/en/2026/03/27/openai-responses-streaming-backpressure-chunk-reassembly-timeout-budget/ |
| AI/TLDR | 3 种失败形态（in-band error / hard disconnect / idle stall）；**"track whether you saw the terminal event"** | https://ai-tldr.dev/learn/llm-apis/streaming-structured-outputs/handle-streaming-errors/ |
| Claude field notes | 5 层防御：state / dedup / tool-arg safety valve / separated backoff / monitoring | https://claudelab.net/en/articles/api-sdk/claude-api-streaming-partial-failure-recovery-field-notes |

Loom 设计综合上述 7 份来源取中位值。

---

## 4. 设计概述

### 4.1 核心思路：三层防护

```
┌────────────────────────────────────────────────────────────────┐
│ Layer 1: send_with_retry  (现有，握手层)                       │
│   - retryable 5xx  +  is_retryable_reqwest_error                │
│   - COMPAT_RETRY_MAX_RETRIES = 20                              │
│   - 1s / 2s / 4s / 8s / 16s                                   │
└────────────────────────────────────────────────────────────────┘
                              ↓ 拿到 res: 200 + body stream
┌────────────────────────────────────────────────────────────────┐
│ Layer 2: per-chunk idle timeout + first_token timeout  (新增) │
│   - first_chunk 前: 3s 内必须出首个 chunk                       │
│   - 之后: 任意两次 chunk 间 ≤ 8s                                │
│   - 超时 = 当作 transport 错误, 走 Layer 3 retry              │
└────────────────────────────────────────────────────────────────┘
                              ↓ 正常流
┌────────────────────────────────────────────────────────────────┐
│ Layer 3: SSE mid-stream retry  (新增, 0-token-only)            │
│   - 条件:  first_chunk_at.is_none()                            │
│           && tool_calls_acc.is_empty()                         │
│           && is_retryable_reqwest_error(&e)                    │
│   - COMPAT_STREAM_RETRY_MAX_RETRIES = 3                        │
│   - 500ms / 1s / 2s / 4s + 30% jitter                          │
│   - 行为: 重新调 send_with_retry, 重置累加器, 整次重发          │
└────────────────────────────────────────────────────────────────┘
```

### 4.2 状态机（invoke_stream 主体）

```
START
  ↓
  send_with_retry  (Layer 1)
  ↓ ok
  reset accumulators
  ↓
  'outer: loop (Layer 3) — max 3 iterations
    │
    ├── 'sse: loop (Layer 2)
    │     │
    │     ├── res.chunk().await
    │     │   ├─ Ok(Some)  → feed parser, send sink, continue
    │     │   ├─ Ok(None)  → break 'outer (normal end)
    │     │   └─ Err(e)
    │     │         ↓
    │     │     check: first_chunk_at.is_some()?
    │     │     ├─ yes  → return audit_error (no retry, partial sent)
    │     │     └─ no   → check: is_retryable_reqwest_error(&e)?
    │     │                ├─ no  → return audit_error
    │     │                └─ yes → check: tool_calls_acc.is_empty()?
    │     │                       ├─ no  → return audit_error (tool safety)
    │     │                       └─ yes → break 'sse, continue 'outer
    │     │
    │     └── 每 chunk 间由 chunk_timeout_future 守护
    │           │
    │           ├── chunk arrived in time → reset timer
    │           └── timer fired → 视同 Err(IdleTimeout)
    │
    ├── outer_attempt < 3 && retry needed
    │     tokio::time::sleep(backoff_with_jitter(outer_attempt))
    │     send_with_retry  → new res
    │     reset accumulators
    │     continue 'outer
    │
    └── outer_attempt == 3
          return audit_error
```

### 4.3 关键不变量

1. **`first_chunk_at.is_some()` ⇒ 永不重发**。已发 token 不能回退
2. **`tool_calls_acc` 非空 ⇒ 永不重发**。tool_call 跨 attempt 复用会污染 JSON
3. **`reasoning_content` 已发 ⇒ 视为已发 token**（同步 sink，但 `first_chunk_at` 已被 thinking 段首字设置）
4. **`res` 在每次 outer attempt 必须新建**。`reqwest::Response` 的 body 是 `Stream`，旧 attempt 的 stream 已断，不能 resume
5. **`request_id` 跨 attempt 不变**。便于服务端日志去重和 trace 关联
6. **`AuditCtx` 跨 attempt 不变**。但每次失败时 `record_error` 一次，便于审计

---

## 5. 详细设计

### 5.1 新增常量（`foundation/llm/src/client/openai_compat/retry.rs`）

```rust
// --- 现有（不变） ---
pub(crate) const COMPAT_RETRY_MAX_RETRIES: u32 = 20;
pub(crate) const COMPAT_RETRY_INITIAL_BACKOFF: Duration = Duration::from_secs(1);
pub(crate) const COMPAT_RETRY_MAX_BACKOFF: Duration = Duration::from_secs(16);

// --- 新增：SSE mid-stream 重试 ---
/// SSE mid-stream 整次重发上限（不含首次 attempt）。
pub(crate) const COMPAT_STREAM_RETRY_MAX_RETRIES: u32 = 3;
/// SSE 重试初始 backoff。
pub(crate) const COMPAT_STREAM_RETRY_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
/// SSE 重试 backoff 封顶。
pub(crate) const COMPAT_STREAM_RETRY_MAX_BACKOFF: Duration = Duration::from_secs(4);
/// Backoff jitter 比例（±30%）。
pub(crate) const BACKOFF_JITTER_RATIO: f64 = 0.30;

// --- 新增：layered timeouts ---
/// 握手完成到首字节最大等待。
pub(crate) const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(3);
/// 任意两 chunk 间最大静默间隔。
pub(crate) const CHUNK_IDLE_TIMEOUT: Duration = Duration::from_secs(8);
```

### 5.2 Backoff with jitter（`retry.rs:32-37` 改造）

原实现：

```rust
pub(crate) fn backoff_for_attempt(attempt: u32) -> Duration {
    let max_secs = COMPAT_RETRY_MAX_BACKOFF.as_secs_f64();
    let secs = (COMPAT_RETRY_INITIAL_BACKOFF.as_secs_f64() * 2_f64.powi(attempt as i32))
        .min(max_secs);
    Duration::from_secs_f64(secs)
}
```

改造后：

```rust
/// Deterministic base backoff (no jitter) — for tests, logging, and exponential curve.
pub(crate) fn backoff_base_for_attempt(
    attempt: u32,
    initial: Duration,
    cap: Duration,
) -> Duration {
    let max_secs = cap.as_secs_f64();
    let secs = (initial.as_secs_f64() * 2_f64.powi(attempt as i32)).min(max_secs);
    Duration::from_secs_f64(secs)
}

/// Apply ±jitter ratio to a base duration. Uses a non-crypto thread-local RNG.
pub(crate) fn backoff_with_jitter(base: Duration, jitter_ratio: f64) -> Duration {
    use std::cell::Cell;
    thread_local! {
        static RNG: Cell<u64> = Cell::new({
            // Mix current nanos with thread id for variety
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xDEAD_BEEF)
        });
    }
    RNG.with(|rng| {
        let mut state = rng.get();
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        rng.set(state);
        let unit = (state as f64) / (u64::MAX as f64); // 0.0..=1.0
        let delta = base.as_secs_f64() * jitter_ratio * (unit * 2.0 - 1.0); // ±jitter
        let secs = (base.as_secs_f64() + delta).max(0.0);
        Duration::from_secs_f64(secs)
    })
}

/// Backward-compatible alias: existing callers without jitter get the original curve.
pub(crate) fn backoff_for_attempt(attempt: u32) -> Duration {
    backoff_base_for_attempt(
        attempt,
        COMPAT_RETRY_INITIAL_BACKOFF,
        COMPAT_RETRY_MAX_BACKOFF,
    )
}
```

设计点：
- **保留** `backoff_for_attempt` 旧 API 行为不变（**G6 兼容性**）
- 新增 `backoff_base_for_attempt(initial, cap, attempt)` 通用化
- 新增 `backoff_with_jitter(base, ratio)` —— **thread-local xorshift64**，不引入 `rand` crate 依赖
- SSE 重试走 `backoff_with_jitter(backoff_base_for_attempt(..., COMPAT_STREAM_...), BACKOFF_JITTER_RATIO)`

### 5.3 `LlmError` 新增变体（`foundation/llm/src/error.rs:19-31`）

```rust
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum LlmError {
    #[error("LLM invoke failed: {0}")]
    InvokeFailed(String),

    #[error("LLM returned empty response after {retries} retries")]
    EmptyResponse { retries: u32 },

    #[error("LLM call cancelled")]
    Cancelled,

    // --- 新增 ---
    /// LLM 调用因瞬时错误失败（transport 重置 / idle timeout / 5xx / 429），
    /// 已被本地 retry 策略用尽 budget。上游 executor 可根据 `transient` 决定
    /// 是否进入自己的退避重试。
    #[error("LLM invoke failed (transient={transient}): {message}")]
    Retryable { message: String, transient: bool },
}

impl LlmError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::InvokeFailed(_)
                | LlmError::EmptyResponse { .. }
                | LlmError::Retryable { .. }
        )
    }
}
```

配套 `From<LlmError> for GraphError`（`error.rs:47-57`）新增分支：

```rust
impl From<LlmError> for GraphError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::InvokeFailed(msg) => GraphError::ExecutionFailed(msg),
            LlmError::EmptyResponse { retries } => {
                GraphError::ExecutionFailed(format!("LLM returned empty response after {retries} retries"))
            }
            LlmError::Cancelled => GraphError::Cancelled,
            // 新增：保留 transient 标记，message 透传
            LlmError::Retryable { message, transient } => GraphError::ExecutionFailed(
                format!("[transient={transient}] {message}"),
            ),
        }
    }
}
```

`From<reqwest::Error> for LlmError`（`error.rs:64-68`）**保持不变** —— 现有握手层 `?` 调用继续走 `InvokeFailed`。新增的 `Retryable` 变体**只在 `invoke_stream` 内部主动构造**。

### 5.4 `audit_error` 改造（`llm_client.rs:179-183`）

现状：

```rust
fn audit_error(&self, ctx: &AuditCtx<'_>, status: u16, err_msg: String) -> LlmError {
    self.record_error(ctx, status, err_msg.clone());
    LlmError::InvokeFailed(err_msg)
}
```

改造后：

```rust
fn audit_error(&self, ctx: &AuditCtx<'_>, status: u16, err_msg: String) -> LlmError {
    self.record_error(ctx, status, err_msg.clone());
    LlmError::InvokeFailed(err_msg)
}

/// SSE mid-stream 专用：审计 + 打 transient 标记
fn audit_stream_error(
    &self,
    ctx: &AuditCtx<'_>,
    err_msg: String,
    transient: bool,
) -> LlmError {
    self.record_error(ctx, 0, err_msg.clone());
    LlmError::Retryable {
        message: err_msg,
        transient,
    }
}
```

### 5.5 `invoke_stream` 主体重构

`llm_client.rs:280-535` 改造要点：

```rust
async fn invoke_stream(
    &self,
    messages: &[crate::message::Message],
    sink: Option<&dyn StreamSink>,
    node_id: &str,
) -> Result<LlmResponse, LlmError> {
    if sink.is_none() {
        return self.invoke(messages).await;
    }

    let trace_id = uuid6().to_string();
    let request_id = uuid6().to_string();
    let sink = sink.expect("sink must be Some when streaming");
    let url = self.chat_completions_url();
    let body = self.build_request(messages, true);
    let tools_count = self.tools.as_ref().map(|t| t.len()).unwrap_or(0);
    let ctx = AuditCtx { /* 同现状 */ };

    debug!(/* 同现状 */);

    // === 拿响应：outer retry 包络 ===
    let mut outer_attempt: u32 = 0;
    let mut res = loop {
        match self
            .send_with_retry(&url, &body, &request_id, "OpenAI-compat stream", &ctx)
            .await
        {
            Ok(r) => break r,
            Err(e) => {
                let transient = matches!(&e, LlmError::Retryable { transient: true, .. })
                    || matches!(&e, LlmError::InvokeFailed(m) if looks_like_transient(m));
                if !transient || outer_attempt >= COMPAT_STREAM_RETRY_MAX_RETRIES {
                    return Err(e);
                }
                outer_attempt += 1;
                let base = backoff_base_for_attempt(
                    outer_attempt,
                    COMPAT_STREAM_RETRY_INITIAL_BACKOFF,
                    COMPAT_STREAM_RETRY_MAX_BACKOFF,
                );
                let delay = backoff_with_jitter(base, BACKOFF_JITTER_RATIO);
                tracing::warn!(
                    trace_id = %trace_id,
                    request_id = %request_id,
                    attempt = outer_attempt,
                    max_retries = COMPAT_STREAM_RETRY_MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "SSE stream handshake retry"
                );
                tokio::time::sleep(delay).await;
            }
        }
    };

    // === 累加器（每次 outer attempt 重置）===
    let mut full_content = String::new();
    let mut full_reasoning_content = String::new();
    let mut sent_any_content = false;
    let mut tool_calls_acc = ToolCallAccumulator::new();
    let mut stream_usage: Option<LlmUsage> = None;
    let mut thinking_parser = self.parse_thinking_tags.then(ThinkingTagParser::new);
    let mut first_chunk_at: Option<std::time::Instant> = None;
    let mut last_chunk_err: Option<reqwest::Error> = None;
    let mut saw_terminal_event = false;

    'outer: loop {
        let mut buf = Vec::<u8>::new();

        'sse: loop {
            // === Layer 2: per-chunk idle timeout + first_token timeout ===
            let timeout = if first_chunk_at.is_none() {
                FIRST_TOKEN_TIMEOUT
            } else {
                CHUNK_IDLE_TIMEOUT
            };
            let chunk_result = tokio::time::timeout(
                timeout,
                res.chunk(),
            )
            .await;

            let bytes = match chunk_result {
                Ok(Ok(Some(bytes))) => {
                    if first_chunk_at.is_none() {
                        first_chunk_at = Some(std::time::Instant::now());
                    }
                    bytes
                }
                Ok(Ok(None)) => {
                    saw_terminal_event = true;
                    break 'sse;
                }
                Ok(Err(e)) => {
                    last_chunk_err = Some(e);
                    break 'sse;
                }
                Err(_elapsed) => {
                    last_chunk_err = Some(/* synthetic IdleTimeout error */);
                    break 'sse;
                }
            };

            // === 原有 SSE 解析逻辑（不动）===
            buf.extend_from_slice(&bytes);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                /* ... 同现状, 但 "data: [DONE]" 时设置 saw_terminal_event = true ... */
                if data == "[DONE]" {
                    saw_terminal_event = true;
                    break 'sse;
                }
                /* ... */
            }
        }

        // === SSE 循环出口处理 ===
        let need_retry = match &last_chunk_err {
            Some(e) => {
                // 1. 已发 token → 不重试
                if first_chunk_at.is_some() {
                    false
                // 2. tool_call 已部分发 → 不重试 (safety valve)
                } else if !tool_calls_acc.is_empty() {
                    tracing::warn!(
                        trace_id = %trace_id,
                        "SSE stream interrupted with partial tool_call; \
                         aborting without retry to avoid argument pollution"
                    );
                    false
                // 3. 错误本身不可重试 → 不重试
                } else if !is_retryable_reqwest_error(last_chunk_err.as_ref().unwrap()) {
                    false
                // 4. 已到重试上限 → 不重试
                } else if outer_attempt >= COMPAT_STREAM_RETRY_MAX_RETRIES {
                    false
                } else {
                    true
                }
            }
            None => false, // 正常结束（saw_terminal_event=true 或不期待 retry）
        };

        if !need_retry {
            // 区分：成功 vs 失败
            if last_chunk_err.is_some() {
                let msg = format!(
                    "OpenAI-compat stream body: {}",
                    last_chunk_err.as_ref().unwrap()
                );
                return Err(self.audit_stream_error(&ctx, msg, true));
            }
            // 正常完成：跳出 outer, 走原有完成逻辑
            break 'outer;
        }

        // === 整次重发 ===
        outer_attempt += 1;
        let base = backoff_base_for_attempt(
            outer_attempt,
            COMPAT_STREAM_RETRY_INITIAL_BACKOFF,
            COMPAT_STREAM_RETRY_MAX_BACKOFF,
        );
        let delay = backoff_with_jitter(base, BACKOFF_JITTER_RATIO);
        tracing::warn!(
            trace_id = %trace_id,
            request_id = %request_id,
            attempt = outer_attempt,
            max_retries = COMPAT_STREAM_RETRY_MAX_RETRIES,
            delay_ms = delay.as_millis() as u64,
            error = %last_chunk_err.as_ref().unwrap(),
            "SSE stream interrupted, resending whole request (0-token case)"
        );
        tokio::time::sleep(delay).await;

        // 重新握手（外层 send_with_retry 已经做过分类, 直接 send_post 即可）
        res = self.send_post(&url, &body, &request_id).await.map_err(|e| {
            self.audit_stream_error(
                &ctx,
                format!("OpenAI-compat stream re-handshake failed: {e}"),
                true,
            )
        })?;

        // 关键：重置所有累加器
        full_content.clear();
        full_reasoning_content.clear();
        sent_any_content = false;
        tool_calls_acc = ToolCallAccumulator::new();
        stream_usage = None;
        thinking_parser = self.parse_thinking_tags.then(ThinkingTagParser::new);
        first_chunk_at = None;
        last_chunk_err = None;
        saw_terminal_event = false;
    }

    // === 原有完成逻辑（不动）===
    /* ... thinking_parser flush, fallback, response build, record_success, Ok ... */
}
```

设计点逐条对应 §4.3 不变量：

| 不变量 | 实现位置 |
|---|---|
| first_chunk_at.is_some() ⇒ 不重发 | `if first_chunk_at.is_some() { false }` 在 need_retry 判断里 |
| tool_calls_acc 非空 ⇒ 不重发 | `if !tool_calls_acc.is_empty() { false }` 同处 |
| reasoning_content 已发 ⇒ 视为已发 | 现有代码 `send_chunk` thinking 段时 `first_chunk_at` 已被设置（`send_chunk` 内 `if first_chunk_at.is_none() { *first_chunk_at = ts; }`，`llm_client.rs:30-41`） |
| res 每次新建 | 整次重发分支 `res = self.send_post(...)?` |
| request_id 跨 attempt 不变 | `request_id` 在 outer scope，loop 不重声明 |
| AuditCtx 不变 | 同上 |

### 5.6 `is_retryable_reqwest_error` 调用面

`is_retryable_reqwest_error` 来自 `loom_http_retry` crate（`foundation/llm/src/support/http_retry.rs:6` 重导出）。本期在以下位置新增调用：

1. **`llm_client.rs:334` chunk error 分支**（替代 `Err(_elapsed)` synthetic）—— timeout → 当作 transient transport 错误
2. **`llm_client.rs` outer 握手 retry 决策**（§5.5 outer loop）
3. **未来**：可作为 `is_retryable_status_for` 的 fallback（当 classifier 拿不到 base_url 兜底）

### 5.7 Sink 契约（`traits.rs:217`）

**契约不变**。`StreamSink::try_send_message(chunk, node_id)` 继续 fire-and-forget，**不**携带 idempotency key / dedup hint。理由：

- 整次重发只在 0-token 情况触发（`first_chunk_at.is_none()`）→ sink 没有任何外部可见效果
- 用户视觉上看不到"重发"—— 因为 0 token 阶段 UI 还没开始渲染
- 一旦已发 token，禁止重发（safety invariant #1），sink 永远不需要 dedup

**唯一会看到 delta 重复的边界条件**：`first_chunk_at.is_none() && sent_any_content` 不可能同时成立（`sent_any_content` 只在 content delta 路径设置，那条路径上 `first_chunk_at` 也被设置），代码层面已证不可能。

### 5.8 可观测性

每次 SSE retry 触发时打 `tracing::warn!`，字段：

```rust
tracing::warn!(
    trace_id = %trace_id,        // 跨 attempt 不变
    request_id = %request_id,    // 跨 attempt 不变, 用于服务端日志关联
    attempt = outer_attempt,     // 1-based
    max_retries = COMPAT_STREAM_RETRY_MAX_RETRIES,
    delay_ms = delay.as_millis() as u64,
    error = %last_chunk_err.as_ref().unwrap(),  // reqwest::Error Display
    error_source = ?last_chunk_err.as_ref().unwrap().source(),  // hyper::Error 等
    "SSE stream interrupted, resending whole request (0-token case)"
);
```

audit log 同样记录 `record_error(ctx, 0, msg)`，便于事后聚合。

---

## 6. 接口变更总览

### 6.1 Public API

| 类型/方法 | 变化 | 兼容性 |
|---|---|---|
| `LlmError::InvokeFailed(String)` | 不变 | 完全兼容 |
| `LlmError::EmptyResponse { retries }` | 不变 | 完全兼容 |
| `LlmError::Cancelled` | 不变 | 完全兼容 |
| `LlmError::Retryable { message, transient }` | **新增** | 纯新增，向后兼容 |
| `LlmError::is_retryable()` | 增加 `Retryable` 变体分支 | 行为变更：原 `InvokeFailed`/`EmptyResponse` 仍返回 true；新增 `Retryable` 也返回 true |
| `From<LlmError> for GraphError` | 增加 `Retryable` 分支 | 行为新增，文字格式变化（`[transient=true] xxx`） |
| `ChatOpenAICompat::invoke_stream` | 内部重试逻辑变化 | **对外行为变更**（成功时无感；失败时错误类型从 `InvokeFailed` 变 `Retryable`） |
| `retry::backoff_for_attempt` | 不变 | 完全兼容 |

### 6.2 Private API（crate 内部）

| 项 | 变化 |
|---|---|
| `retry::COMPAT_STREAM_RETRY_MAX_RETRIES` | 新增 |
| `retry::COMPAT_STREAM_RETRY_INITIAL_BACKOFF` | 新增 |
| `retry::COMPAT_STREAM_RETRY_MAX_BACKOFF` | 新增 |
| `retry::BACKOFF_JITTER_RATIO` | 新增 |
| `retry::FIRST_TOKEN_TIMEOUT` | 新增 |
| `retry::CHUNK_IDLE_TIMEOUT` | 新增 |
| `retry::backoff_base_for_attempt(initial, cap, attempt)` | 新增 |
| `retry::backoff_with_jitter(base, ratio)` | 新增 |
| `audit::audit_stream_error(ctx, msg, transient)` | 新增 |

### 6.3 行为变更面

| 场景 | 旧行为 | 新行为 |
|---|---|---|
| SSE 0-token 时 transport 失败 | `InvokeFailed("...error decoding...")` | 3 次重试整次重发；仍失败 → `Retryable { transient: true, message }` |
| SSE 已发 N token 后 transport 失败 | `InvokeFailed` | `Retryable { transient: true, message }`（不重发，但携带标记） |
| first chunk 前等了 3s+ | 无限等 | `Retryable { transient: true }` |
| 两 chunk 间等了 8s+ | 无限等 | `Retryable { transient: true }` |
| tool_call 已发后 transport 失败 | `InvokeFailed` | `Retryable { transient: true }` + audit log warn 标注 safety valve 触发 |
| `send_post` 内部握手重试 20 次 | 不变 | 不变 |
| 正常完成 | 不变 | 不变 |

---

## 7. PR 拆分（按依赖关系排序）

### PR-1: 重试基础与 jitter（最小闭环）

**目标**：实现 §5.1、§5.2、§5.3、§5.4。**不**做超时。

**文件变更**：
- `foundation/llm/src/client/openai_compat/retry.rs` —— 新增 6 个常量 + 2 个函数
- `foundation/llm/src/error.rs` —— 新增 `Retryable` 变体 + `is_retryable()` 分支 + `From<LlmError>` 分支
- `foundation/llm/src/client/openai_compat/audit.rs` —— 新增 `audit_stream_error`
- `foundation/llm/src/client/openai_compat/llm_client.rs` —— invoke_stream 加 outer retry 骨架
- 测试：`retry.rs` 加 6 个常量 + 2 个函数的单元测试；`error.rs` 加 `Retryable` 变体测试

**验收**：
- `cargo test -p loom-llm` 全绿
- 新增 `audit_stream_error` 单元测试
- 新增 SSE retry 集成测试（mock server 在 N 次 chunk 后返 `IncompleteMessage`，断言整次重发 + 终态 `Retryable`）

**风险**：低。`Retryable` 是纯新增变体；`backoff_for_attempt` 行为不变；invoke_stream 改动只在失败路径触发，正常完成无感。

### PR-2: per-chunk idle timeout + first_token timeout

**目标**：实现 §5.5 中的 `tokio::time::timeout` 包装。

**前置**：PR-1 已合入。

**文件变更**：
- `llm_client.rs` invoke_stream 内的 `'sse: loop` 把 `res.chunk().await` 包成 `tokio::time::timeout(timeout, res.chunk()).await`

**验收**：
- 新增集成测试：mock server 在 response head 后 10s 不发 body，断言 8s 后抛 `Retryable { transient: true }`
- 新增集成测试：mock server 在 first chunk 前 sleep 5s，断言 3s 后抛 `Retryable { transient: true }`

**风险**：中。首次引入 `tokio::time::timeout` 与 `res.chunk()` 嵌套，**需注意**：
- `res.chunk()` 本身已 partial，内部已经异步
- timeout 触发时 `res` 仍存活但被丢弃 —— `Drop` 时会关闭底层连接，无泄漏
- 不能用 `select!` 因为 `res.chunk()` 持有 `&mut res`，无法在另一分支同时使用

### PR-3: tool_call safety valve 强化 + 错误分类细化

**目标**：实现 §5.5 中 `tool_calls_acc.is_empty()` 检查 + 把 4xx 错误明确打 `transient=false`。

**前置**：PR-1 已合入。

**文件变更**：
- `llm_client.rs` invoke_stream 的 need_retry 决策增加 tool_call 检查
- `error.rs` 新增辅助函数 `classify_for_retry(err: &LlmError) -> bool`（区分 transient / permanent）
- `llm_client.rs` `send_with_retry` 路径用 classifier 给 4xx 打 `Retryable { transient: false }`

**验收**：
- 新增集成测试：mock server 在 tool_call 已发后 5s 切断，断言 `Retryable` 而非重发
- 新增集成测试：mock server 返 400，断言 `Retryable { transient: false }` 且 `is_retryable() == true`（语义允许上游重试但本地不重试）

**风险**：中。`transient` 字段可能让上游误以为可无限重试；需在 doc-comment 明确语义。

### PR-4: 文档与日志加固（无代码逻辑变化）

**目标**：§5.8 可观测性字段全部上 + `docs/evolution/sse-streaming-retry-design.md`（本文档）落地。

**前置**：PR-1 已合入。

**文件变更**：
- `docs/evolution/sse-streaming-retry-design.md`（本文档）
- `llm_client.rs` retry 分支 `tracing::warn!` 加 `error_source` 字段
- `audit.rs` `record_error` payload 加 `transient: bool` 字段

**验收**：
- `cargo doc -p loom-llm` 无 warning
- audit log 端到端验证：retry 触发时 audit 表里有对应记录

**风险**：零。

---

## 8. 测试方案

### 8.1 单元测试

| 模块 | 测试 | 断言 |
|---|---|---|
| `retry::backoff_base_for_attempt` | 指数曲线 | `attempt=0→500ms, 1→1s, 2→2s, 3→4s, 4→4s` (cap) |
| `retry::backoff_with_jitter` | jitter 范围 | 100 次采样，分布在 ±30% 内 |
| `retry::backoff_for_attempt` | 兼容性 | 旧 API 行为不变（无 jitter） |
| `error::LlmError::Retryable` | Display | `"LLM invoke failed (transient=true): ..."` |
| `error::LlmError::is_retryable` | Retryable 分支 | 返回 true |
| `error::From<LlmError> for GraphError` | Retryable 分支 | 转 `ExecutionFailed` 带 `[transient=true]` 前缀 |

### 8.2 集成测试（mock server via `wiremock`）

| 场景 | mock 行为 | 断言 |
|---|---|---|
| **T1**：正常流 | 200 + 标准 SSE | `Ok(LlmResponse)`，audit success |
| **T2**：SSE 0-token 后 transport 失败 | 200 + 空 body + 立即断 | 3 次 POST，3 次都 `Ok(Some(0字节))` 后断；最终 `Retryable { transient: true }` |
| **T3**：SSE 0-token + retryable 5xx | 第一次 502，第二次 200 + SSE 完整 | `Ok(LlmResponse)`，audit 2 条 |
| **T4**：SSE first_token 超时 | 200 + head 完整 + 5s 不发 body | 3s 后 `Retryable`，`tracing::warn!` 含 `first_token_timeout` |
| **T5**：SSE chunk_idle 超时 | 200 + 1 chunk + 10s 静默 | 8s 后 `Retryable` |
| **T6**：SSE 已发 token 后失败 | 200 + 2 chunk + 断 | 立即 `Retryable`（不重发） |
| **T7**：tool_call safety valve | 200 + tool_call chunk + 断 | 立即 `Retryable` + audit warn |
| **T8**：4xx 不重试 | 400 | 1 次 POST，`Retryable { transient: false }` |
| **T9**：429 retry | 第一次 429，第二次 200 | 走 `send_with_retry` 路径，2 次 POST |
| **T10**：jitter 实际生效 | 跑 100 次 retry | delay 在 350-650ms 范围（base=500ms ±30%） |

### 8.3 端到端测试

`foundation/llm/tests/streaming_retry_e2e.rs`（新增）：

| 场景 | 验证 |
|---|---|
| **E1**：本地 `wiremock` 模拟 5xx + 重试 + 成功 | 真实 `tokio::main`，跑完整 invoke_stream |
| **E2**：tool_call partial → safety valve | 真实 wiremock，断言 audit log 内容 |
| **E3**：first_token 超时 + 重试 | 真实 wiremock，sleep 5s |

### 8.4 性能 / 压力测试

- **P1**：连续 1000 次 `invoke_stream` 正常流 → 监控 P50 / P95 / P99 首字延迟，对比 baseline
- **P2**：连续 100 次带重试的流 → 监控总耗时，确认 jitter 不会因 thundering herd 引发雪崩
- **P3**：mock server 间歇性 502 → 100 次重试，确认 client 不会触发 exponential backoff 失控

---

## 9. 兼容性与风险

### 9.1 兼容性

- **API**：`LlmError` 纯新增变体，match 表达式需要 wildcard 分支兜底（编译器会强制 `match` 检查）
- **行为**：成功路径无感；失败路径错误类型变化（`InvokeFailed` → `Retryable`），下游若 `match LlmError::InvokeFailed(s) => ...` 会编译失败 —— 需提前 grep
- **配置**：无新增 env / config；常量硬编码
- **数据库 / 持久化**：audit log 表 schema 可能需要新增 `transient` 列 —— PR-4 落地

### 9.2 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| `tokio::time::timeout` 与 `res.chunk()` 交互问题（`&mut res` 借用冲突） | 中 | timeout 不生效 | PR-2 实施时先用 `tokio::pin!` + `select!` 验证，再切到 `timeout` |
| `Retryable` 变体让上游误判无限重试 | 中 | 重试雪崩 | doc-comment 明确语义；PR-3 在 classifier 加 `transient` 分类 |
| audit log 表 schema 不兼容 | 低 | 写入失败 | PR-4 加列前先查现有 schema；提供 migration |
| jitter 引入非确定性，CI 偶发 | 中 | test flake | jitter 测试用 `backoff_base_for_attempt` + 单独 `backoff_with_jitter` 测试，不在 retry 集成测试中断言 jitter |
| 0-token 判断不准（thinking 段已发但 content 段没发） | 低 | 重发导致 reasoning 重复 | 现有 `first_chunk_at` 已被任何 chunk 设置触发 → 已发过就视为已发 |
| 超时太短（8s）误杀长 reasoning 模型 | 中 | 完整 reasoning 流被截 | 8s 是单 chunk 间隔，不是 total；thinking 模型单 chunk 也应在 8s 内到；如不放心可提到 15s（LangChain 120s 显然过松，15s 是 Mengboy 8s 的 ~2x） |
| 旧测试 `mock LLM` 适配新 error 变体 | 中 | CI 红 | PR-1 实施前先 grep `match.*InvokeFailed` 列出调用点 |

### 9.3 回退计划

- 所有 PR 独立可回退
- 关键开关：若 PR-2 引入超时太激进，回退 `tokio::time::timeout` 包装即可，`FIRST_TOKEN_TIMEOUT`/`CHUNK_IDLE_TIMEOUT` 设为 `Duration::MAX` 等价于关闭
- PR-1 的 `Retryable` 变体若引起下游 panic，回退 `error.rs` 的 4 行即可（变体定义 + is_retryable + From + 1 测试）

---

## 10. 未来工作

1. **R1**：把 `FIRST_TOKEN_TIMEOUT` / `CHUNK_IDLE_TIMEOUT` 提到 `ModelConfig`（per-provider 配置），对稳定 OpenAI 用 8s/3s，对自建 vLLM 用 30s/15s
2. **R2**：circuit breaker —— 在 `ChatOpenAICompat` 内维护 5xx 滑动窗口，连续 N 次失败熔断 X 秒
3. **R3**：sink 携带 `idempotency_key` 字段，让 UI 端做去重（覆盖 `first_chunk_at` 误判的极端情况）
4. **R4**：把 `is_retryable_status_for` 与 `is_retryable_reqwest_error` 合并为统一的 `classify_error(&LlmError) -> RetryClass` 枚举（`Transient` / `RateLimited` / `Permanent` / `ToolSafetyAbort`）
5. **R5**：monitoring —— 把 retry 总数 / 成功率 / 平均 attempt 数暴露为 Prometheus 指标
6. **R6**：跨 client 统一 —— `AnthropicClient` / `MinimaxClient` 等接入同一套 Layer 2/3

---

## 11. 附录 A：上游根因分类

按出现概率从高到低（基于 LangChain / Mengboy / AI-TLDR / Claude field notes 经验）：

| 根因 | 信号 | Loom 当前表现 | 修复后表现 |
|---|---|---|---|
| **idle timeout**（30s+ 无字节） | `hyper::Error(IncompleteMessage)` 或 read timeout | 0-token 失败 | 重发整次 |
| **HTTP/2 GOAWAY** | 同上 | 0-token 失败 | 重发整次 |
| **TLS / 代理切断** | `connection reset by peer` | 0-token 失败 | 重发整次 |
| **upstream 5xx** | HTTP 502/503/504 | `send_with_retry` 捕获并重试 | 不变 |
| **rate limit 429** | HTTP 429 | `send_with_retry` 捕获并重试 | 不变 |
| **content_filter** | HTTP 200 + `finish_reason="content_filter"` | 当前识别不出 | PR-3 后识别为 `transient: false` |
| **model OOM**（自建 vLLM） | 连接 200 后 60s 不发字节 | 无限等 | 8s idle timeout 触发重试 |
| **provider 端 panic** | 连接中断，TCP RST | 0-token 失败 | 重发整次 |

---

## 12. 附录 B：参考资料

### 12.1 行业参考（已在 §3 列表）

- LangChain PR #36949: https://github.com/langchain-ai/langchain/pull/36949
- Gloo AI streaming best practices: https://docs.gloo.com/best-practices/completions-streaming-failures
- OpenCode commit 14e0b9b: https://github.com/anomalyco/opencode/commit/14e0b9b17f886c9157c92e1b98caca5a40d21797
- DGX Code "brain layer": https://wiki.charleschen.ai/ai/processed/wiki/llm-core/cli/techniques/streaming-retry-and-fallback-brain
- Mengboy production guide: https://www.mfun.ink/en/2026/03/27/openai-responses-streaming-backpressure-chunk-reassembly-timeout-budget/
- AI/TLDR: https://ai-tldr.dev/learn/llm-apis/streaming-structured-outputs/handle-streaming-errors/
- Claude field notes: https://claudelab.net/en/articles/api-sdk/claude-api-streaming-partial-failure-recovery-field-notes

### 12.2 Loom 内部文件

- `foundation/llm/src/client/openai_compat/llm_client.rs:286-535` —— invoke_stream 主体
- `foundation/llm/src/client/openai_compat/llm_client.rs:122-176` —— send_with_retry（握手层）
- `foundation/llm/src/client/openai_compat/llm_client.rs:328-336` —— SSE chunk error 当前路径
- `foundation/llm/src/client/openai_compat/retry.rs:13-37` —— 当前 retry 常量与 backoff
- `foundation/llm/src/client/openai_compat/audit.rs:80+` —— audit record_error
- `foundation/llm/src/error.rs:19-31` —— LlmError 定义
- `foundation/llm/src/error.rs:33-41` —— is_retryable
- `foundation/llm/src/error.rs:47-57` —— From<LlmError> for GraphError
- `foundation/llm/src/support/http_retry.rs:6-10` —— loom_http_retry 重导出
- `foundation/llm/src/traits.rs:217` —— StreamSink 契约
- `foundation/llm/src/traits.rs:261-317` —— LlmClient trait
- `thirdparty/hermes-agent/agent/conversation_loop.py:3022-3122` —— 上游 classifier（待复核）

### 12.3 工具与依赖

- `loom_http_retry` crate —— 提供 `is_retryable_reqwest_error`, `retry_backoff_for_attempt`
- `wiremock` —— 集成测试 mock server
- `tokio::time::timeout` —— per-chunk idle timeout（PR-2 引入）
- `tracing` —— retry 日志（已有）
