# Loom Reasoning 流：thinking → thought 接入修复设计

> 日期: 2025-08-19
> 状态: 设计完成，待实施
> 范围: `foundation/llm/`、`apps/server/`、loom 端零改动
> 关联文档:
> - `.loom/skills/auto/software-engineering/repository-protocol-analysis/references/loom-reasoning-streaming-protocol.md`（技术参考，本设计的深度诊断底座）
> - `docs/opencode-protocol/audits/loom-vs-opencode-endpoints.md`（端点合规审计，本设计是其延伸）

## 1. 背景

**用户问题**：在 openchamber 前端（TUI/React）看不到 reasoning / thinking / 思考过程的内容。LLM provider 实际输出了 reasoning 字段，前端 SSE 推送里却没有 reasoning part。

### 1.1 期望行为

opencode / openchamber 前端按 `message.part.updated` 事件里的 `type:"reasoning"` part 渲染思考过程：

```jsonc
{
  "type": "message.part.updated",
  "properties": {
    "sessionID": "sess_xxx",
    "messageID": "msg_xxx",
    "part": {
      "id": "prt_xxx",
      "type": "reasoning",
      "text": "<逐步出现的 thinking 内容>",
      "time": { "start": 1234, "end": 5678 }
    }
  }
}
```

### 1.2 现状（loom 当前行为）

LLM provider SSE 流量能进入 loom（`llm_client.rs:387-398` 的 `MessageChunk::thinking` 触发逻辑是 OK 的），但流到前端时 reasoning 内容丢失或 schema 不匹配。**根因不在一条**：是上游字段别名 + v1/v2 schema 错位 + part.id 命名 + 生命周期事件四层差异叠加。

### 1.3 差距总览（loom vs opencode）

| # | 维度 | loom 当前 | opencode 期望 | 影响 |
|---|---|---|---|---|
| 1 | LLM SSE `reasoning_content` 字段别名 | 只识别 `reasoning` / `reason_content` | DeepSeek/GLM-4.5/Stepfun/QwQ/Anthropic-extended 用 `thinking` / `reasoning_text` | ❌ 字段被 serde 静默丢弃 |
| 2 | v1 vs v2 schema `time` 字段名 | `time.start` / `time.end` | v2 用 `time.created` / `time.completed` | ⚠️ v2 客户端 time 字段 undefined |
| 3 | `part.id` 前缀 | 字面量 `"reasoning-0"` / `"text-0"` | v1 schema 强制 `Schema.isStartsWith("prt")` | ⚠️ v1 schema 严格校验下被拒 |
| 4 | opencode v2 reasoning 生命周期事件 | 不发 | `session.next.reasoning.{started,delta,ended}` | ⚠️ v2 增量渲染路径看不到 |
| 5 | `message.part.delta` 增量事件 | 不发（v1 累积快照） | v1 schema 定义，v2 已弃用 | ⚠️ v1 TUI 无打字机效果（终态仍在） |
| 6 | `providerMetadata` 透传 | 不解析、不写出 | v2 schema optional，Anthropic 验签/ replay 必要 | ⚠️ Anthropic extended-thinking 验签丢失 |

外层 envelope（`sessionID` / `messageID` / `partID` camelCase）、事件名（`message.part.updated`）**全部对齐**。

## 2. 设计原则

**只动 loom-server 与 llm-client；opencode / openchamber 零改动**。兼容策略是 loom 同时输出 v1 与 v2 两种 wire 形态，前端任选其一路由消费。

| 不动 | 改动 |
|---|---|
| opencode TUI / openchamber 前端任何代码 | — |
| opencode SDK (`packages/sdk/js/`) | — |
| Loom core (`agent/`、`foundation/` 大部分模块) | — |
| `stream_event` / `agent-core` / `tool_workflow` 等内部 crate 公开 API | — |
| — | `foundation/llm/src/client/openai_compat/stream.rs::StreamDelta` 加 3 个 alias |
| — | `apps/server/src/translator.rs::translate_chunk` 改 part.id 命名、补 v2 `time.*` 字段 |
| — | `apps/server/src/translator.rs` 增 `session.next.reasoning.*` 三段事件 emit（#4） |
| — | `foundation/llm/src/client/openai_compat/stream.rs::StreamDelta` 增 `provider_metadata` 字段解析（#6） |
| — | `apps/server/src/translator.rs` 把 `provider_metadata` 透传到 reasoning part（#6） |

每一条改动都验证后端不破坏现有 v1/v2 TUI 路径，**回归一条失败回滚全部**。

## 3. 修复方案（按优先级）

### 3.1 P0：补 LLM SSE 字段 alias（#1）

`foundation/llm/src/client/openai_compat/stream.rs:25-26`

```rust
#[serde(default,
    alias = "reasoning",
    alias = "reason_content",
    alias = "thinking",                  // Anthropic extended thinking / Stepfun / Qwen QwQ
    alias = "reasoning_text",            // DeepSeek R1 / GLM-4.5 z.ai / OpenRouter
    alias = "reasoning_details",         // 一些反代 / provider-options 层
)]
pub reasoning_content: Option<String>,
```

**影响**：一行改动，零回归风险（只新增识别字段，不影响现有）。立即使 DeepSeek/GLM-4.5/Stepfun/QwQ/Anthropic extended-thinking 在 loom 上线。

### 3.2 P0：改 `part.id` 满足 v1 schema（#3）

`apps/server/src/translator.rs::translate_chunk` 与 `agent_runner.rs::push_part`：

把字面量 `"reasoning-0"` / `"text-0"` 改成符合 `Schema.isStartsWith("prt")` 的 UUID 形式：

```rust
fn make_part_id(prefix: &str) -> String {
    format!("prt_{}", uuid::Uuid::new_v4().simple())
}
```

**稳定性约束**：同一 `assistant_msg_id` 内同一 kind（reasoning / text / tool）必须复用同一 id，否则 TUI state machine 把第二个 part 当成新的丢弃原内容。可以在 `state.parts` 里以 `(msg_id, part_type)` 为键缓存。

**影响**：opencode v1 schema 严格校验通过；opencode v2 不强制 `prt_` 前缀，无回归。

### 3.3 P0：v2 `time` 字段双发（#2）

`apps/server/src/translator.rs::translate_chunk` 写出 `time` 时同时给 v1 命名（`start` / `end`）和 v2 命名（`created` / `completed`），取相同 timestamp：

```rust
"time": {
    "start":    chrono::Utc::now().timestamp_millis(),       // v1
    "created":  chrono::Utc::now().timestamp_millis(),       // v2
},
```

终态补 `time.end` 时同样双发 `time.completed`。两个命名都是 schema optional，**不会冲突**。

**影响**：v1 / v2 客户端都能读到 time，零回归。

### 3.4 P1：发 v2 `session.next.reasoning.*` 生命周期事件（#4）

在 `translate_chunk`（或新增 `translate_reasoning_lifecycle`）里，当遇到 `MessageChunkKind::Thinking` 第一个 chunk 时 emit `session.next.reasoning.started`，中间每个 chunk emit `session.next.reasoning.delta`，assistant message 结束时 emit `session.next.reasoning.ended`。

Schema 字段：

```jsonc
// started
{ "type": "session.next.reasoning.started",
  "properties": {
    "sessionID": "sess_xxx",
    "assistantMessageID": "msg_xxx",
    "reasoningID": "prt_reasoning_xxx",
    "providerMetadata": { /* optional */ }
  }
}
// delta
{ "type": "session.next.reasoning.delta",
  "properties": {
    "sessionID": "...",
    "assistantMessageID": "...",
    "reasoningID": "...",
    "delta": "<chunk content>"
  }
}
// ended
{ "type": "session.next.reasoning.ended",
  "properties": {
    "sessionID": "...",
    "assistantMessageID": "...",
    "reasoningID": "...",
    "text": "<累积全文>",
    "providerMetadata": { /* optional */ }
  }
}
```

**实现细节**：
- 在 `state.parts` 之外维护 `reasoning_state: HashMap<assistant_msg_id, { reasoning_id, full_text }>` 用于去重 + 累积
- `started` 在 `MessageChunkKind::Thinking` 第一次出现时发一次；后续 chunk 只发 `delta`
- `ended` 由 LLM `finish_reason:"stop"` 或 message end signal 触发；同时把 `providerMetadata`（如果上游带了）一并透传

**同时保留** `message.part.updated` 路径（v1 兼容），两条线路并存，前端按自己 schema 版本消费。

**影响**：v2 客户端可走增量渲染（打字机效果）；持久化路径仍由 `message.part.updated` 累积快照负责。

### 3.5 P2：透传 `providerMetadata`（#6）

`foundation/llm/src/client/openai_compat/stream.rs::StreamDelta`：

```rust
#[serde(default)]
pub provider_metadata: Option<serde_json::Value>,
```

`apps/server/src/translator.rs`：把该字段透传到 reasoning part 的 `providerMetadata`，并在 v2 `reasoning.ended` 事件里透传。

Anthropic extended thinking 在 `response.usage` / `response.content[*].signature` 里携带 `signature` / `redacted_data`，opencode 用这些做后续 reasoning 验签 / replay。**不影响显示**，影响 replay consistency。

**影响**：不影响显示，但让 Anthropic 风格 extended-thinking 的 replay 验签能力恢复。

### 3.6 P3（可选）：v1 `message.part.delta` 增量事件（#5）

loom v1 当前只发 `message.part.updated`（累积快照）。如果 opencode v1 TUI 增量渲染路径依赖 `message.part.delta`，在 `translate_chunk` 增量路径里 emit：

```jsonc
{
  "type": "message.part.delta",
  "properties": {
    "sessionID": "...",
    "messageID": "...",
    "partID": "prt_xxx",
    "field": "text",             // 或 "reasoning.encrypted_content" 等
    "delta": "<新 chunk>"
  }
}
```

**优先级低**：opencode v2 已弃用此事件，v1 TUI 走累积快照路径也能最终显示，仅缺失打字机效果。

## 4. 改动文件清单

| 优先级 | 文件 | 行（参考） | 改动 |
|---|---|---|---|
| P0 #1 | `foundation/llm/src/client/openai_compat/stream.rs` | 25-26 | 加 3 个 alias |
| P0 #3 | `apps/server/src/translator.rs` | 397-458 | time 双字段 |
| P0 #2 | `apps/server/src/translator.rs` + `agent_runner.rs` | 397-458 + 158-198 | part.id 改 `prt_<uuid>` |
| P1 #4 | `apps/server/src/translator.rs` (新增函数) | 新增 | v2 reasoning 三段事件 |
| P2 #6 | `foundation/llm/src/client/openai_compat/stream.rs` | 23-28 | 加 `provider_metadata` 字段 |
| P2 #6 | `apps/server/src/translator.rs` | 397-458 | 透传 `providerMetadata` |
| P3 #5 | `apps/server/src/translator.rs` | 414-436 | 可选 `message.part.delta` emit |

每条改动独立 PR，便于回滚。

## 5. 验证步骤

按顺序执行；任一步失败立即定位：

1. **单元测试（`StreamDelta` 解析）**

   ```rust
   #[test]
   fn parses_thinking_field() {
       let json = r#"{"choices":[{"delta":{"thinking":"step 1"}}]}"#;
       let chunk: StreamChunk = serde_json::from_str(json).unwrap();
       assert_eq!(chunk.choices.unwrap()[0].delta.reasoning_content.as_deref(), Some("step 1"));
   }

   #[test]
   fn parses_reasoning_text_field() {
       let json = r#"{"choices":[{"delta":{"reasoning_text":"step 2"}}]}"#;
       let chunk: StreamChunk = serde_json::from_str(json).unwrap();
       assert_eq!(chunk.choices.unwrap()[0].delta.reasoning_content.as_deref(), Some("step 2"));
   }
   ```

   对应改 `apps/server/src/translator.rs` 的现有测试 `translate_chunk_*`，覆盖 reasoning case。

2. **手工抓包**：启动 loom-server，跑一次带 reasoning 的 prompt，在 `foundation/llm/src/client/openai_compat/llm_client.rs:331-342` 的 `'sse: loop` 处临时 `dbg!(data)`，确认上游真实字段名。

3. **curl 抓 loom 出栈**：

   ```bash
   curl -N http://127.0.0.1:18081/api/session/<id>/event | grep reasoning
   ```

   期望看到 `message.part.updated` payload 的 part.id 是 `prt_xxx`，time 同时含 `start`/`end`/`created`/`completed`。

4. **v2 schema 解码测试**：拿一份 loom 当前 SSE payload，在 opencode v2 schema `.loom/opencode-ref/schema/session-message.ts:147` 下 `Schema.decodeUnknown`，不抛错即为通过。

5. **Anthropic extended-thinking 端到端**：开 `LOOM_PROVIDER=anthropic` 且 `INTERLEAVED_THINKING=true`，跑 prompt，抓 `providerMetadata.signature` 字段确认透传成功。

6. **回归现有 v1/v2 SSE 通路**：

   ```bash
   cargo test -p loom-server
   ./scripts/check-protocol.ps1
   ```

   跑完无新增失败。

## 6. 风险评估

| 风险 | 等级 | 缓解 |
|---|---|---|
| #1 alias 加错导致 serde 退到 `None` 反而丢 reasoning | 低 | 加单元测试覆盖每个别名 |
| #2 `prt_<uuid>` 生成引入新 bug，破坏现有 TUI 状态机 | 中 | 同一 message+type 复用同一 id 的缓存；先在 dev 跑 regression |
| #3 `time` 双字段同时存在触发某些客户端 strict decoder 失败 | 低 | 两个字段都是 schema optional，客户端应忽略多余字段 |
| #4 v2 三段事件 emit 与 v1 累积快照并发导致前端 dedup 异常 | 中 | 仔细读 opencode `message-updater.ts:343-373` 的 accumulate 逻辑，确保 `started→delta` 序列唯一标识 |
| #5 provider 字段 `provider_metadata` 在某些 OpenAI 兼容端是 `null` | 低 | serde `Option<Value>`，null 解析为 None |
| #6 同 part.id 复用逻辑与 `push_part` 现有去重冲突 | 中 | 复用走 `state.parts` 现有 key（`msg_id + part_type`），不破坏现有结构 |

## 7. 回滚策略

- 每条 P0/P1 PR 独立合并
- 任意一条引入回归 → 单 PR revert；loom-server 在 v1/v2 路径间始终至少有累积快照 (`message.part.updated`) 兜底
- P0 #1 一行 alias 改动风险极低，可作为 fast-rollout 第一步

## 8. 后续跟踪

- 完成 P0 #1/#2/#3 后，重新跑 `scripts/check-protocol.ps1` 与 `docs/opencode-protocol/audits/loom-vs-opencode-endpoints.md` 中关联的 M2/M4 检查项
- 当 openchamber 前端确认消费 `session.next.reasoning.*`，可在 `docs/opencode-protocol/audits/loom-vs-opencode-endpoints.md` 中把 reasoning 维度从"潜在 bug 区"移出
- 跟踪 v1 `message.part.delta` 实际消费者；如果只有 v1 TUI 增量渲染路径依赖且 v2 已弃用，可把 P3 #5 推迟或砍掉
