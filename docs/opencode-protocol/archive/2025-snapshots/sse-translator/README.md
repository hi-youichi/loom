# Translator 重构：对齐 OpenCode SSE 协议

## 目标

**Loom 的 SSE 事件输出与 OpenCode 协议不一致，导致前端（OpenChamber）无法正确渲染 part 序列。**

OpenCode 前端期望收到的 SSE 事件结构（`schema/session-event.ts`）：

```
session.next.text.delta      — 文本增量
session.next.reasoning.delta — 推理增量
session.next.part.updated    — part 全量更新（含 time.start/time.end）
session.next.step.started    — 回合开始
session.next.step.finished   — 回合结束（含 token 用量）
```

Loom 当前实际输出：

```
message.part.updated         — 每次 delta 都发完整 part（无 delta 事件）
message.tokens               — 独立的 token 事件（OpenCode 无对应）
（无 step-start / step-finish）
```

**差距**：
- 无回合边界（step-start / step-finish）
- 无增量事件（delta）
- reasoning/text 切换靠 part-type 搜索推断，不可靠
- token 用量作为独立事件而非嵌入 step-finish

本组文档定义了完整的改造方案，使 Loom SSE 输出与 OpenCode v2 schema 逐字段对齐。

> **核实说明**（2025-08-19）：本文档中引用的 `processor.ts` 行号基于 2025-07 代码快照，
> 文件后续已重构（函数内联到 `Effect.gen` 闭包中），行号已偏移。**逻辑描述经核实仍与当前
> `dev` 分支源码一致**。`event-reducer.ts` 在当前源码中不存在（见 04-protocol-and-id.md §2.7 修正）。

## 文档目录

| 文件 | 内容 | 任务 |
|---|---|---|
| [01-opencode-architecture.md](01-opencode-architecture.md) | OpenCode 核心架构：ProcessorContext、状态重置、事件类型映射表 | 参考 |
| [02-part-lifecycle.md](02-part-lifecycle.md) | Text/Reasoning Part 生命周期：边界处理、cleanup、状态重置 | E1-E8, E15-E16, F1, G2 |
| [03-tool-lifecycle.md](03-tool-lifecycle.md) | Tool Part 生命周期：状态机、ensureToolCall、completeToolCall、failToolCall | E11-E12, X3-X5 |
| [04-protocol-and-id.md](04-protocol-and-id.md) | Event 协议（updatePart vs delta）、PartID 生成策略、step-start/finish | E9-E10, G1, G3, X1-X2 |
| [05-error-handling.md](05-error-handling.md) | 错误处理与重试管线：process() 管线、halt()、三态返回 | E12-E13, G3, X8 |
| [06-checklist.md](06-checklist.md) | 修改清单汇总 | 全部 |
| [07-examples.md](07-examples.md) | 事件序列对比示例 + 数据流图 | E1-E10, G1 |
| [08-stream-event-refactor.md](08-stream-event-refactor.md) | StreamEvent 改造：block 生命周期变体、BlockTracker | A1-A7, B1-B6, C1-C2, D1-D2, E2-E17, F2 |
| [09-snapshot-patch.md](09-snapshot-patch.md) | Snapshot/Patch 机制：shadow git DB 架构、track/patch/revert | X6 |
| [10-compaction.md](10-compaction.md) | Context window compaction | X7 |
| [11-protocol-impact.md](11-protocol-impact.md) | 协议影响说明：SSE 事件变化、part 类型变化、客户端适配清单 | 全部 |
| [12-integration-test-plan.md](12-integration-test-plan.md) | 集成测试方案：BlockTracker 单元、全链路参数化、多回合 agent loop、错误路径 | T1-T4 |
| [appendix-a-opencode-v2-schema.md](appendix-a-opencode-v2-schema.md) | OpenCode v2 Session Event Schema 逐字段定义 + Loom 映射对照 | 参考 |
| [appendix-b-sse-payload-examples.md](appendix-b-sse-payload-examples.md) | Loom vs OpenCode SSE JSON payload 样例对比 | 参考 |
| [appendix-c-openchamber-integration.md](appendix-c-openchamber-integration.md) | OpenChamber 前端集成指南：part 渲染、step 边界、验证步骤 | H7 |
| [appendix-d-opencode-events-full.md](appendix-d-opencode-events-full.md) | OpenCode v2 Session Event 完整 32 个事件清单 + Loom 对照 | 参考 |
| [TODO.md](TODO.md) | 待办事项：H 协议验证、I 集成测试、G 字段 gap、OC 前端适配 | — |
| [13-field-gap-plan.md](13-field-gap-plan.md) | G1-G5 字段 Gap 补齐开发方案：Token 结构、Delta 事件、agent/model、cost、BlockEnd 文本 | G1-G5 |

## 实施状态

| 阶段 | 状态 | 说明 |
|---|---|---|
| A 枚举改造 | ✅ 已完成 | `TextDelta`/`ReasoningDelta`/`TurnStart`/`TurnFinish`/`Finish` 等变体已落地 |
| B MessageChunk | ✅ 已完成 | `MessageChunk`/`MessageChunkKind` 从 `StreamEvent` 中移除；作为 `StreamSink` trait 的参数类型保留 |
| C BlockTracker | ✅ 已完成 | `block_tracker.rs` 已创建并通过编译 |
| D Provider SSE | ✅ 已完成 | `think_node.rs::BlockTrackerSink` 实现了 `MessageChunk → BlockTracker → StreamEvent` 适配 |
| E Translator | ✅ 已完成 | 12 个 match arm + finalize 函数已落地 |
| F 调用方 | ✅ 已完成 | `session.rs` 两处已替换 |
| G Agent runner | ✅ 已完成 | `TurnStart`/`TurnFinish` 发射已落地 |
| **H 协议验证** | ❌ 未做 | 运行实际 session，抓取 SSE 输出，与 OpenCode `session-event.ts` schema 逐字段比对 |
| **I 集成测试** | ❌ 未做 | 见 [12-integration-test-plan.md](12-integration-test-plan.md) |

## 实施路线

按 [06-checklist.md](06-checklist.md) 的统一清单执行：

1. **A-G**（代码改造）✅ 已完成
2. **H**（协议级 diff 验证）— 运行一次实际 agent loop，抓取 SSE，与 OpenCode schema 比对
3. **I**（集成测试）— 按 [12-integration-test-plan.md](12-integration-test-plan.md) 实现

## 实际实现与设计的偏差

设计文档描述的是目标终态（`llm_client → BlockTracker → StreamEvent`），实际落地为适配层架构：

```
设计: llm_client → BlockTracker → StreamEvent → translator
实际: llm_client → MessageChunk → BlockTrackerSink(StreamSink) → BlockTracker → StreamEvent → translator
```

原因：`LlmClient::invoke_stream` trait 的签名接受 `Option<&dyn StreamSink>`，有 17 个实现。
直接改签名风险过大，因此 `BlockTrackerSink` 作为适配层保留 `StreamSink` 接口，内部调用 `BlockTracker`。
功能等价，但多一层间接。
