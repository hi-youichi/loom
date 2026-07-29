# Context Window Compaction 完整实现方案

> 返回 [README.md](README.md)
>
> 状态：设计完成，尚未实现。

## 10.1 背景

> 开发任务：X7（context window compaction）

[04-protocol-and-id.md](04-protocol-and-id.md) 将 OpenCode `step-finish` 的 compaction check
列为待单独设计项。Loom 已经具备压缩内核，但尚未形成完整的 agent loop、协议和服务端能力：

- `ContextWindowCheck` 可以估算当前上下文并判断是否溢出。
- `PruneNode` 可以裁剪旧工具输出。
- `CompactNode` 可以调用 LLM 生成摘要并替换历史消息。
- `CompactionConfig` 已包含自动压缩、上下文上限和保留窗口配置。
- ReAct graph 目前只在 `observe` 后进入 compression graph。

相关代码：

- `agent/agent-core/src/compress/context_window.rs:13`
- `agent/agent-core/src/compress/prune_node.rs:22`
- `agent/agent-core/src/compress/compact_node.rs:29`
- `agent/agent-core/src/compress/config.rs:6`
- `agent/agent-core/src/agent/react/runner/runner.rs:124`

当前实现存在以下缺口：

1. `START -> think` 不经过压缩，加载旧 checkpoint 并追加新用户消息后可能直接触发 provider context overflow。
2. 无 tool call 的纯文本回答走 `think -> END`，不会进入现有 `observe -> compress` 路径。
3. provider 返回 context overflow 时没有结构化错误和一次性压缩恢复。
4. 压缩过程没有统一的 `started/delta/ended/failed` 流事件。
5. `POST /api/session/:sessionID/compact` 仍返回 501。
6. `/context` 返回全部协议消息，不能反映模型当前实际使用的压缩后上下文。

## 10.2 目标

本方案一次性实现以下能力：

- 每次 LLM 调用前进行 context budget 检查。
- 优先 prune，仍然超限时再调用摘要模型。
- 自动压缩、手动压缩和 provider overflow 恢复共用一套执行器。
- 压缩后立即使用新上下文重试同一个 turn。
- provider overflow 最多自动恢复一次。
- 对外发送完整 compaction 生命周期事件。
- 手动 compact 直接操作 checkpoint，不通过空 prompt 间接触发。
- 模型上下文、协议 `/context` 和持久化状态保持一致。
- 同一 session 的 prompt、compact、abort 操作具有明确的并发和取消语义。

不在本方案中实现：

- 会话标题或普通 summary fork。
- Snapshot/Patch；该能力见 [09-snapshot-patch.md](09-snapshot-patch.md)。
- 通用网络错误重试策略。
- 删除 compaction 之前的完整审计消息。

## 10.3 设计原则

### 10.3.1 Overflow 决策属于 agent-core

translator 是协议投影层，只能将内核事件转换为 OpenCode 事件，不得修改 `ReActState`、设置
`needsCompaction` 或控制 graph 路由。

虽然 OpenCode 的旧实现可以在 `step-finish` 后根据 usage 设置标记，Loom 更适合在每次 `think`
之前运行 context guard：这样可以在请求发送给 provider 之前消除已知的上下文溢出。

`TurnFinish` 只负责：

- 收尾 text/reasoning part。
- 写入 usage、cost 和 finish reason。
- 生成 `step-finish` part。

### 10.3.2 Checkpoint 是模型上下文的事实来源

- `ReActState.messages` 表示下一次 LLM 请求使用的真实上下文。
- server 的 messages/parts 表示完整用户可见历史，不因 compaction 删除。
- `/context` 是压缩后模型上下文的协议投影。
- `/messages` 继续返回完整审计历史。

### 10.3.3 压缩必须是事务性的

- 摘要生成失败或被取消时不得覆盖旧 checkpoint。
- 只有压缩结果持久化成功后才发送 `compaction.ended`。
- 任意失败都必须清理 session operation 和 `time.compacting`。

## 10.4 目标调用链

```text
START
  -> context_guard
       -> prune tool outputs
       -> check context budget
       -> compact when required
  -> think
       |- tool calls -> act -> observe -> context_guard -> think
       |- provider context overflow -> context_guard -> think (once)
       `- final answer -> END
```

当前 graph：

```text
START -> think
think -> act | END
act -> observe -> compress -> think
```

修改后：

```text
START -> context_guard -> think
think -> act | context_guard | END
act -> observe -> context_guard -> think
```

现有 `CompressionGraphNode` 直接承担 `context_guard` 职责，不新增功能重复的
`PreflightCompactionNode`。建议将 graph node id 从 `compress` 改名为 `context_guard`，内部仍为：

```text
prune -> compact
```

对应修改位置：

- `agent/agent-core/src/agent/react/runner/runner.rs:117`
- `agent/agent-core/src/compress/graph.rs:19`
- `agent/agent-core/src/agent/react/mod.rs:64`

## 10.5 Context budget 判定

### 10.5.1 结构化检查结果

扩展 `agent/agent-core/src/compress/context_window.rs`：

```rust
pub struct ContextWindowReport {
    pub estimated_input_tokens: usize,
    pub reserved_output_tokens: usize,
    pub max_context_tokens: usize,
    pub remaining_tokens: i64,
    pub overflow: bool,
    pub source: TokenEstimateSource,
}

pub enum TokenEstimateSource {
    FullEstimate,
    ProviderUsage,
    ConservativeMaximum,
}
```

保留 `is_overflow()` 作为兼容包装：

```rust
pub fn is_overflow(check: &ContextWindowCheck<'_>) -> bool {
    check_context_window(check).overflow
}
```

### 10.5.2 Token 计算

```text
full_estimate =
    estimate(all messages)
    + estimate(tool schemas)
    + provider request overhead

usage_estimate =
    previous prompt tokens
    + previous completion tokens
    + estimate(messages appended after the previous think)

estimated_input = max(full_estimate, usage_estimate)
reserved_output = max(config.reserve_tokens, model.output_limit)
remaining = max_context_tokens - estimated_input - reserved_output
overflow = remaining <= 0
```

采用两种估算的较大值，避免：

- 只有字符估算时低估 provider 的 tool schema 开销。
- 只使用上一轮 usage 时漏算新追加的 user/tool 消息。
- 模型 output limit 大于固定 reserve 时预留不足。

当 `max_context_tokens == 0` 或模型没有上下文信息时，不自动压缩；手动压缩仍然有效。

### 10.5.3 配置优先级

```text
显式 ReactBuildConfig.compaction_config
  > model-spec context_limit/output_limit
  > CompactionConfig::default()
```

现有解析入口为 `agent/agent-core/src/agent/react/build/runners.rs:48`。需要同时解析 model
`output_limit`，并把 tool schema token 开销注入 `ContextWindowCheck`。

## 10.6 压缩算法

### 10.6.1 两级处理

1. `PruneNode` 裁剪已经完成的旧工具输出。
2. prune 后重新计算 context budget。
3. 如果不再 overflow，则结束 context guard，不调用摘要 LLM。
4. 如果仍然 overflow，或收到 manual/force request，则执行 compact。

`auto=false` 只关闭自动 overflow compact，不影响：

- 手动 compact。
- 明确的 provider overflow 恢复请求。

`prune=false` 时跳过工具输出裁剪。

### 10.6.2 Token-based recent window

现有 `compact_keep_recent` 按消息数量保留，无法约束单条超大消息。新增：

```rust
pub compact_keep_tokens: usize,
```

选择 recent context 时从后向前累计 token，并遵守以下边界：

- leading system/policy 消息始终原样保留。
- assistant tool call 和对应 tool result 作为不可拆分单元。
- 不产生孤立 tool result。
- 单个单元超过预算时进行安全序列化，而不是简单截断 JSON 结构。
- 上一次 compaction summary 必须参与下一次摘要，实现链式压缩。

`compact_keep_recent` 暂时保留用于配置兼容；设置 `compact_keep_tokens` 后以 token 配置为准。

### 10.6.3 输出结果

将 `compaction::compact()` 的返回值从 `Vec<Message>` 改为：

```rust
pub struct CompactionResult {
    pub messages: Vec<Message>,
    pub summary: String,
    pub recent: String,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub pruned_messages: usize,
}
```

输出消息顺序：

```text
[pinned system messages]
[summary of earlier conversation]
[recent messages]
```

压缩完成后必须再次检查：

```text
after_tokens + reserved_output_tokens < max_context_tokens
```

如果仍然超限，返回 `CompactionInsufficient`，禁止继续请求 provider。

对应修改：

- `agent/agent-core/src/compress/compaction.rs:106`
- `agent/agent-core/src/compress/compact_node.rs:39`
- `agent/agent-core/src/compress/prune_node.rs:22`

## 10.7 ReActState 与路由

在 `ReActState` 中新增：

```rust
pub enum CompactionReason {
    Auto,
    Manual,
    ProviderOverflow,
}

pub struct CompactionRequest {
    pub id: String,
    pub reason: CompactionReason,
}

pub compaction_request: Option<CompactionRequest>,
pub overflow_recovery_attempted: bool,
pub last_compaction: Option<CompactionResultMetadata>,
```

状态规则：

- 正常 preflight overflow 由 `context_guard` 直接识别，reason 为 `Auto`。
- 手动 endpoint 设置 `compaction_request = Manual`。
- provider overflow 设置 `compaction_request = ProviderOverflow`。
- compact 成功后清除 `compaction_request` 和 `force_compact`。
- provider overflow recovery 标志只有在下一次 think 成功后才清除。
- `last_compaction` 只承载持久化和事件投影所需元数据，不保存重复的大型 message 内容。

将 `tools_condition` 扩展为：

```rust
pub enum ToolsConditionResult {
    Compact,
    Tools,
    End,
}
```

路由优先级：

```text
compaction_request -> context_guard
tool_calls not empty -> act
otherwise -> END
```

## 10.8 Provider context overflow 恢复

### 10.8.1 结构化错误

扩展 `foundation/llm/src/error.rs:19`：

```rust
ContextOverflow {
    message: String,
}
```

OpenAI-compatible、Anthropic 和其他 provider adapter 负责把对应 HTTP 状态、error code 和响应体
转换为该变体。不得依赖 agent-core 对错误字符串做模糊匹配。

### 10.8.2 恢复条件

在 `agent/agent-core/src/agent/react/think_node.rs:246` 处理：

```text
ContextOverflow
AND assistant 尚未产生任何 text/reasoning/tool delta
AND overflow_recovery_attempted == false
```

满足时：

1. 不把本次失败写成 assistant 内容。
2. 设置 `compaction_request = ProviderOverflow`。
3. 设置 `overflow_recovery_attempted = true`。
4. 路由到 `context_guard`。
5. compact 后重新执行同一个 think。

以下情况不自动恢复：

- provider 已经发送部分 assistant 内容。
- 同一个 turn 已经执行过一次 overflow recovery。
- compact 后 context budget 仍然不足。
- 压缩摘要模型本身发生 context overflow。

第二次失败返回明确的 `ContextOverflowAfterCompaction`，避免无限循环。

## 10.9 Compaction 事件

### 10.9.1 StreamEvent 变体

在统一的 `StreamEvent<S>` 中加入：

```rust
ContextUpdated {
    used_tokens: usize,
    max_tokens: usize,
    remaining_tokens: i64,
}

CompactionStarted {
    id: String,
    reason: CompactionReason,
    before_tokens: usize,
    max_tokens: usize,
}

CompactionDelta {
    id: String,
    delta: String,
}

CompactionEnded {
    id: String,
    reason: CompactionReason,
    summary: String,
    recent: String,
    before_tokens: usize,
    after_tokens: usize,
}

CompactionFailed {
    id: String,
    reason: CompactionReason,
    error: String,
}
```

不使用 `Custom(Value)`，确保编译器可以检查所有 translator 消费者。

### 10.9.2 事件顺序

正常压缩：

```text
context.updated
compaction.started
compaction.delta * N
compaction.ended
```

prune 已解决 overflow：

```text
context.updated
```

失败：

```text
context.updated
compaction.started
compaction.failed
```

摘要调用使用独立的 `CompactionStreamSink`。摘要 token 不能作为正常 assistant text delta 进入当前
assistant message；它只产生 `CompactionDelta`。

### 10.9.3 Translator 映射

修改 `apps/server/src/translator.rs:159`：

| Loom StreamEvent | OpenCode event |
|---|---|
| `ContextUpdated` | `session.next.context.updated` |
| `CompactionStarted` | `session.next.compaction.started` |
| `CompactionDelta` | `session.next.compaction.delta` |
| `CompactionEnded` | `session.next.compaction.ended` |
| `CompactionFailed` | `session.error` |

translator 不负责调用 compact，也不持有 context budget 配置。

## 10.10 手动 Compaction API

实现当前 stub：`apps/server/src/handlers/session.rs:491`。

```http
POST /api/session/:sessionID/compact
```

### 10.10.1 执行流程

1. 检查 session 是否存在。
2. 获取 session operation guard；如果已有 prompt/compact，返回 409。
3. 将 `session.time.compacting` 设置为当前时间并持久化 session。
4. 使用 `build_react_initial_state_for_resume()` 加载 checkpoint，不追加用户消息。
5. 设置 `compaction_request = Manual`。
6. 只运行 compression graph，不进入 `ThinkNode`。
7. 压缩成功后原子写回 checkpoint。
8. 持久化 `CompactionRecord`。
9. 发送 `compaction.ended` 和 `session.updated`。
10. 清除 `time.compacting` 和 operation guard。
11. 返回 `204 No Content`。

禁止使用空 user prompt 触发 `run_agent()`；否则会污染会话历史，并在 compact 后额外执行一次普通 think。

### 10.10.2 Agent-core API

为 `ReactRunner` 增加：

```rust
pub async fn compact_current_state(
    &self,
    reason: CompactionReason,
    cancellation: Option<RunCancellation>,
) -> Result<CompactionResult, CompactionRunError>;
```

该方法复用 runner 已有的：

- checkpointer。
- runnable config/thread id。
- compression graph。
- compaction LLM。
- event sender。

checkpoint 不存在时返回 `NoContext`，不得创建只有 system prompt 的伪会话。

### 10.10.3 HTTP 状态码

| 状态码 | 条件 |
|---|---|
| 204 | 压缩完成 |
| 404 | session 不存在 |
| 409 | session 正在执行 prompt 或 compact |
| 422 | session 没有 checkpoint，或上下文不足以压缩 |
| 500 | 摘要、事件持久化或 checkpoint 写入失败 |

## 10.11 持久化与 `/context`

### 10.11.1 CompactionRecord

在 server store 中新增：

```rust
pub struct CompactionRecord {
    pub id: String,
    pub session_id: String,
    pub reason: CompactionReason,
    pub summary: String,
    pub recent: String,
    pub covered_message_ids: Vec<String>,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub created_at: i64,
}
```

`covered_message_ids` 是 compaction 开始时已经被 summary/recent checkpoint 覆盖的协议消息，用于稳定地区分
完整历史和当前上下文，不能只依赖时间戳或 Vec 下标。

### 10.11.2 查询语义

`GET /messages`：

```text
返回所有 user/assistant/tool 消息，不删除 compaction 之前的历史。
```

`GET /context`：

```text
返回最新 CompactionRecord 的 checkpoint 投影
+ 不在 covered_message_ids 中的后续消息。
```

没有 compaction record 时，保持返回全部当前消息。

当前错误说明位于 `apps/server/src/handlers/session.rs:536`，实现后删除“loom-server has no compaction”描述。

### 10.11.3 写入顺序

```text
生成摘要
-> 写入压缩后 ReAct checkpoint
-> 写入 CompactionRecord
-> 更新 session
-> 发送 compaction.ended
```

任一步失败时：

- 未提交 checkpoint：保留旧状态。
- checkpoint 已提交但 CompactionRecord 失败：回滚 checkpoint 或将操作标记为可恢复，不能发送 ended。
- 服务启动时校验 checkpoint 的 `last_compaction.id` 与最新 `CompactionRecord.id`，修复中断写入。

优先使用 store 的单事务写入；如果当前 StoreTrait 不支持事务，则新增专用的
`commit_compaction(checkpoint, record, session)` 原子接口。

## 10.12 配置协议

扩展 server `ConfigInfo`：

```json
{
  "compaction": {
    "auto": true,
    "prune": true,
    "maxContextTokens": 128000,
    "reserveTokens": 4096,
    "keepTokens": 20000,
    "pruneKeepTokens": 20000,
    "pruneMinimum": 20000
  }
}
```

字段规则：

| 字段 | 语义 |
|---|---|
| `auto` | 是否允许自动 overflow compaction |
| `prune` | 是否优先裁剪旧工具输出 |
| `maxContextTokens` | 显式覆盖模型 context limit |
| `reserveTokens` | 为下一次模型输出保留的最小 token 数 |
| `keepTokens` | compact 后 recent context 的 token 预算 |
| `pruneKeepTokens` | prune 后保留的工具结果 token 预算 |
| `pruneMinimum` | 低于该值时不执行 prune |

校验规则：

- 所有 token 数必须大于等于 0。
- `reserveTokens < maxContextTokens`。
- `keepTokens + reserveTokens < maxContextTokens`。
- 显式非法配置在加载阶段报错，不静默回退。
- 配置更新只影响后续 operation，不修改正在运行的 compaction。

配置 DTO 转换为 agent-core `CompactionConfig` 后再传入 runner，translator 不读取配置。

## 10.13 并发、取消与错误处理

### 10.13.1 Session operation guard

当前 `begin_run()` 会取消同 session 的旧 run，见 `apps/server/src/state.rs:717`。manual compact 不能复用这个
“替换旧任务”的语义。

新增：

```rust
pub enum SessionOperationKind {
    Prompt,
    Compact,
}

pub fn try_begin_operation(...) -> Result<SessionOperationGuard, OperationConflict>;
```

规则：

- 同一个 session 同时只能有一个 prompt 或 compact。
- 不同 session 可以并发。
- 新 prompt 不得静默取消正在执行的 manual compact。
- 新 compact 不得静默取消正在执行的 prompt。
- `/abort` 取消当前 operation，无论类型。

### 10.13.2 RAII 清理

`SessionOperationGuard` drop/finish 时必须：

- 从 operation registry 移除当前 generation。
- 清除 `time.compacting`。
- 恢复 session status 为 idle。
- 发送必要的 `session.updated`。

只允许 generation 匹配的旧任务清除自己，沿用 `end_run()` 的防竞态检查，见
`apps/server/src/state.rs:737`。

### 10.13.3 错误可见性

- 对客户端发送稳定、可读的错误类型，不泄露 provider 原始响应或凭据。
- 日志包含 session id、compaction id、reason、before/after tokens 和 duration。
- summary 正文不进入普通 info 日志。
- `CompactionFailed` 后不得继续执行当前 LLM turn。

## 10.14 一次性代码改动清单

| 文件 | 修改 |
|---|---|
| `agent/agent-core/src/compress/config.rs` | 新增 token-based keep 配置和校验 |
| `agent/agent-core/src/compress/context_window.rs` | 增加 `ContextWindowReport`、tool/output budget 计算 |
| `agent/agent-core/src/compress/compaction.rs` | 返回 `CompactionResult`，保留 policy，按 token 选择 recent，支持链式摘要 |
| `agent/agent-core/src/compress/compact_node.rs` | 统一 auto/manual/recovery，发送 compaction 生命周期事件 |
| `agent/agent-core/src/compress/prune_node.rs` | 返回 prune 元数据并在 prune 后重新检查 budget |
| `agent/agent-core/src/compress/graph.rs` | 将 compression graph 作为可复用 context guard，并支持独立 manual invoke |
| `agent/agent-core/src/state.rs` | 新增 compaction request/recovery/result 状态 |
| `agent/agent-core/src/agent/react/mod.rs` | 条件路由增加 `Compact` |
| `agent/agent-core/src/agent/react/runner/runner.rs` | 改为每次 think 前经过 context guard，暴露 manual compact API |
| `agent/agent-core/src/agent/react/think_node.rs` | 处理一次性 provider overflow recovery |
| `foundation/llm/src/error.rs` | 新增结构化 `ContextOverflow` |
| provider adapters | 将 provider context overflow 映射为结构化错误 |
| `foundation/stream-event/src/types/stream_event.rs` | 新增 Context/Compaction 事件变体 |
| `apps/server/src/translator.rs` | 映射 compaction/context 事件 |
| `apps/server/src/state.rs` | 新增 operation registry、CompactionRecord 和配置 DTO |
| server store | 持久化 CompactionRecord，提供原子 commit |
| `apps/server/src/agent_runner.rs` | 注入 compaction event sender 和配置 |
| `apps/server/src/handlers/session.rs` | 实现 manual compact 和正确的 `/context` 投影 |

## 10.15 测试清单

### 10.15.1 Context budget

- 正好低于、等于和超过阈值。
- 无 provider usage 时使用 full estimate。
- tool schema token 被计入。
- model output limit 大于 reserve 时使用 output limit。
- 未知 context limit 时不自动 compact。
- 显式 manual request 不受 `auto=false` 影响。

### 10.15.2 Compaction

- prune 后解除 overflow，不调用摘要 LLM。
- prune 后仍 overflow，执行摘要。
- leading system/policy 消息保持原样。
- tool call/result 不被拆分。
- token-based recent window 不超过预算。
- 重复 compaction 合并上一轮 summary。
- compact 后仍超限返回 `CompactionInsufficient`。
- 摘要失败和取消不修改输入 state。

### 10.15.3 ReAct graph

- 新用户 prompt 第一次 think 前执行 context guard。
- tool loop 的每次 think 前执行 context guard。
- 无 tool call 的正常回答可以直接 END。
- provider overflow 在无输出时 compact 并重试同一 turn。
- provider 已产生 delta 时不自动恢复。
- 第二次 provider overflow 返回错误，不无限循环。
- `auto=false` 时预检查不自动摘要。

### 10.15.4 Stream 与 translator

- `ContextUpdated -> Started -> Delta* -> Ended` 顺序固定。
- 摘要 delta 不写入普通 assistant text part。
- 失败只发送一次 `CompactionFailed/session.error`。
- event payload 符合 OpenCode schema。
- 多 session 并发事件不会串线。

### 10.15.5 Server

- manual endpoint 返回 204/404/409/422/500。
- manual compact 不追加空 user message，也不执行普通 think。
- prompt 与 compact 对同 session 互斥。
- `/abort` 可以取消 manual compact。
- 取消和失败后 `time.compacting` 必定清除。
- `/messages` 保留完整历史。
- `/context` 返回最新 compaction checkpoint 和后续消息。
- 服务重启后 checkpoint、CompactionRecord 和 session 状态一致。

## 10.16 验收标准

实现完成必须同时满足：

1. 任何 ReAct LLM 调用之前都经过 context guard。
2. 已知会超限的请求不会先发送给 provider。
3. provider overflow 最多自动恢复一次。
4. auto、manual、provider recovery 使用同一个压缩执行器。
5. manual compact 不污染消息历史，不产生额外 assistant turn。
6. 压缩失败或取消不会破坏旧 checkpoint。
7. `compaction.ended` 发出时，压缩后 checkpoint 和 CompactionRecord 已持久化。
8. `/messages` 与 `/context` 分别保持完整历史和活动上下文语义。
9. translator 只做事件转换，不参与 overflow 决策。
10. 所有新增单元测试、graph 集成测试、server contract 测试、lint 和 typecheck 通过。
