# Loom v2 SSE 逐类型修正矩阵

状态：实现完成，待真实 Provider 的全类型互操作回归。更新日期：2026-07-24。目标 OpenCode revision：`b8142c7aa`。

本文是 [Loom v2 SSE 修正方案](LOOM-V2-SSE-REMEDIATION.md) 的逐类型附录，覆盖 `packages/schema/src/session-event.ts` 的全部 32 个 `session.next.*` event。所有 event 的外层均为：

```json
{ "id": "evt_*", "type": "session.next.*", "data": { "timestamp": 0, "sessionID": "sess_*" } }
```

标记说明：**V1/V2** 是 durable version；**live** 表示不得进入 session durable replay log；“无来源”意味着当前 Loom 不能可靠生成该事件，不能伪造。

## 实施状态（2026-07-24）

`apps/server/src/handlers/session.rs` 与 `translator.rs` 已包含本矩阵全部 **32** 个 `session.next.*` publisher：28 个 durable、4 个 live-only。生产 durable rows 写入 `$LOOM_HOME/server/v2-events/<sessionID>.jsonl`，并由 `/api/session/:id/event?after=<seq>` 回放；live-only rows仅由 `/api/event` 输出。下方“当前来源/状态”和“精确修正动作”保留为实施前的差距追踪，不能再按字面理解为当前未实现。

## 1. 通用、prompt 与 shell（9）

| v2 type | data 额外必填字段 | durable | Loom 当前来源/状态 | 精确修正动作 |
| --- | --- | --- | --- | --- |
| `session.next.agent.switched` | `messageID`, `agent` | V1 | 无 event；session PATCH 可变 agent，但没有切换生命周期。 | 在成功更新 agent 后发布；记录触发该切换的 message ID。 |
| `session.next.model.switched` | `messageID`, `model` | V1 | `/api/session/:id/model` 未完成。 | 先实现真实 model switch；成功后发布标准 `Model.Ref`。 |
| `session.next.moved` | `location`, `subdirectory?` | V1 | 无来源。 | 仅在真实 session/location move 实现后发布。 |
| `session.next.prompted` | `messageID`, `prompt`, `delivery` | V1 | `prompt_v2()` 创建 user message，但未发 typed event。 | 在 admission 前由 handler 发布，使用原始 prompt/files/agents 与 delivery。 |
| `session.next.prompt.admitted` | `messageID`, `prompt`, `delivery` | V1 | 有异步 admission，但只有 legacy message event。 | 持久写入 user message 成功后发布；失败时不发布 admitted。 |
| `session.next.context.updated` | `messageID`, `text` | V1 | 无 context 更新 event。 | 仅在 context epoch/compaction 真实实现后发布。 |
| `session.next.synthetic` | `messageID`, `text` | V1 | 无 synthetic message 能力。 | 不实现前不发布。 |
| `session.next.shell.started` | `messageID`, `callID`, `command` | V1 | `run_shell` 能执行命令，但无 call lifecycle。 | 创建稳定 call ID，在执行前发布。 |
| `session.next.shell.ended` | `callID`, `output` | V1 | 同上。 | 命令结束后发布；失败须定义为 shell output/error 的目标语义，不能复用 `session.error`。 |

## 2. Step（3）

| v2 type | data 额外必填字段 | durable | Loom 当前来源/状态 | 精确修正动作 |
| --- | --- | --- | --- | --- |
| `session.next.step.started` | `assistantMessageID`, `agent`, `model`, `snapshot?` | V1 | `TurnStart` 仅生成 legacy `step-start` part。 | 从 `V2RunContext` 读取 assistant message/agent/model，发布 typed event；没有有效 model 时不得标记支持。 |
| `session.next.step.ended` | `assistantMessageID`, `finish`, `cost`, `tokens`, `snapshot?`, `files?` | V2 | `TurnFinish` 生成 legacy `step-finish` part；token shape 接近；使用 `reason`、无 cost/snapshot/files。 | 显式 `reason → finish` 映射，接入 provider cost，发布 durable V2 settlement。 |
| `session.next.step.failed` | `assistantMessageID`, `error: UnknownError` | V2 | `ProviderError` 只发 legacy `session.error`。 | 错误分类为 `UnknownError`，结束活动 block 后发布 V2 settlement failed。 |

## 3. Text（3）

| v2 type | data 额外必填字段 | durable | Loom 当前来源/状态 | 精确修正动作 |
| --- | --- | --- | --- | --- |
| `session.next.text.started` | `assistantMessageID`, `textID` | V1 | `TextBlockStart` 创建空 legacy text part。 | 分配/复用稳定 text ID，写入 tracker 后发布 durable started。 |
| `session.next.text.delta` | `assistantMessageID`, `textID`, `delta` | live | `TextDelta` 被累积为 `message.part.updated`；生产不发 delta。 | 保留原始 chunk，在写累计文本前发布 live delta；绝不写 durable log。 |
| `session.next.text.ended` | `assistantMessageID`, `textID`, `text` | V1 | `TextBlockEnd` 只更新 part time。 | 从 tracker 取完整 text，发布 durable ended；重连依赖它恢复文本。 |

## 4. Reasoning（3）

| v2 type | data 额外必填字段 | durable | Loom 当前来源/状态 | 精确修正动作 |
| --- | --- | --- | --- | --- |
| `session.next.reasoning.started` | `assistantMessageID`, `reasoningID`, `providerMetadata?` | V1 | `ReasoningBlockStart { id, metadata }` 生成 legacy part。 | 用输入 `id` 作为 reasoning ID；将可兼容 metadata 映射为 providerMetadata，无法映射则省略。 |
| `session.next.reasoning.delta` | `assistantMessageID`, `reasoningID`, `delta` | live | 累计 reasoning part。 | 发布原始 chunk；不进入 durable log。 |
| `session.next.reasoning.ended` | `assistantMessageID`, `reasoningID`, `text`, `providerMetadata?` | V1 | End 只盖 part 的时间。 | tracker 保存完整 text/metadata 后发布 durable ended。 |

## 5. Tool（7）

| v2 type | data 额外必填字段 | durable | Loom 当前来源/状态 | 精确修正动作 |
| --- | --- | --- | --- | --- |
| `session.next.tool.input.started` | `assistantMessageID`, `callID`, `name` | V1 | `ToolCall` 已有 name/call ID，但没有 input streaming 分段。 | 在解析调用开始发布；call ID 缺失时先在上游保证稳定 ID。 |
| `session.next.tool.input.delta` | `assistantMessageID`, `callID`, `delta` | live | 无可靠上游 input delta。 | 只有 provider 提供参数增量时实现；否则不发该可选 live 行为。 |
| `session.next.tool.input.ended` | `assistantMessageID`, `callID`, `text` | V1 | `ToolCall.arguments` 有结构化参数。 | 用规范 JSON 序列化形成 raw input text；与 called 共享 call ID。 |
| `session.next.tool.called` | `assistantMessageID`, `callID`, `tool`, `input`, `provider{executed,metadata?}` | V1 | 创建 pending tool part，字段不符合 schema。 | 将 arguments 作为 object input；执行端决定 `provider.executed`，不能固定猜测。 |
| `session.next.tool.progress` | `assistantMessageID`, `callID`, `structured`, `content` | V1 | `ToolOutput` 是字符串累加。 | 建 `ToolContent[]` adapter，限频发布 durable progress，避免每 stdout chunk 持久化。 |
| `session.next.tool.success` | `assistantMessageID`, `callID`, `structured`, `content`, `outputPaths?`, `result?`, `provider` | V1 | `ToolEnd { is_error:false }` 完成 part。 | 将 raw result 转为 content/result，收集 output paths，附 provider 执行信息。 |
| `session.next.tool.failed` | `assistantMessageID`, `callID`, `error`, `result?`, `provider` | V1 | `ToolError` 或 `ToolEnd { is_error:true }` 修改 part。 | 统一成 `UnknownError`；确保一次 call 只发一个 terminal success 或 failed。 |

## 6. Retry、compaction 与 revert（7）

| v2 type | data 额外必填字段 | durable | Loom 当前来源/状态 | 精确修正动作 |
| --- | --- | --- | --- | --- |
| `session.next.retried` | `attempt`, `error: RetryError` | V1 | 无 retry pipeline event。 | 接入真实 retry 后发布；填 `message/isRetryable`，可选 HTTP 信息。 |
| `session.next.compaction.started` | `messageID`, `reason` (`auto`/`manual`) | V1 | compaction 仅有旧设计，当前无可验证 event。 | 先实现事务性 compaction，再发布。 |
| `session.next.compaction.delta` | `messageID`, `text` | live | 无来源。 | 压缩摘要流存在时发布；不持久化。 |
| `session.next.compaction.ended` | `messageID`, `reason`, `text`, `recent` | V1 | 无来源。 | 事务提交后以最终 summary/recent window 发布。 |
| `session.next.revert.staged` | `revert` | V1 | handler 目前不是完整 revert state。 | 只有生成完整 `Revert.State` 后发布。 |
| `session.next.revert.cleared` | 无额外字段 | V1 | 无真实 staged state。 | 清除成功后发布。 |
| `session.next.revert.committed` | `messageID` | V1 | 无真实 commit 行为。 | 文件/状态提交成功后发布，禁止 stub 成功 event。 |

## 7. 类型级不变量

1. 每个 durable event 必须有该 session 的连续 `durable.aggregateID/seq/version`；live event 没有 `durable`。
2. 所有 `data.sessionID` 必须和 durable aggregate ID 一致。
3. 同一 text/reasoning/tool ID 必须 start 后才可 delta/end；terminal event 之后不能再发 delta。
4. `step.ended` 与 `step.failed` 对一个 assistant message 互斥，且使用 durable version 2。
5. `/api/session/:id/event` 只能重放 durable rows；`server.connected`、`server.heartbeat`、text/reasoning/input delta 都不得进入该 stream。
6. legacy `message.*` 仍由旧 translator 发出；本矩阵的 v2 publish 不删除或重命名它们。

## 8. 实施顺序

先实现 Step + Text + Reasoning 的 **7 个 durable 类型**（随后补 2 个 text/reasoning live delta）与 durable log；然后 Tool 的 6 个 durable 类型和 1 个 input live delta；最后按真实产品能力补齐通用、shell、retry、compaction 和 revert。每完成一行，必须加入该类型的 schema fixture、translator 单测、replay 测试和 legacy 双发回归。
