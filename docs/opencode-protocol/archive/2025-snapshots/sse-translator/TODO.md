# 待办事项

> 返回 [README.md](README.md)

## 已完成

| 事项 | 完成日期 | 验证方式 |
|---|---|---|
| A-G 代码改造 | 2025-08-19 | `cargo check --workspace` ✅ · `cargo test -p stream-event` 28/28 ✅ · `cargo test -p loom-server --lib` 64/64 ✅ · `cargo clippy` 0 errors ✅ |
| 文档体系 | 2025-08-19 | 15 篇文档（01-12 + 附录 A-C），围绕"SSE 协议对齐"目标 |

## 未完成

### H — 协议验证（P0）

> 目标：证明 Loom 实际 SSE 输出与设计文档一致
> 步骤详见 [06-checklist.md](06-checklist.md) §H

| # | 任务 | 耗时预估 | 阻塞 |
|---|---|---|---|
| H1 | curl 抓包 SSE，验证 text block 序列（start → delta → end） | 30min | 需可用的 LLM API key |
| H2 | 验证 reasoning block 序列 | 同上 | 同上 |
| H3 | 验证 step-start / step-finish part 含 tokens 字段 | 同上 | 同上 |
| H4 | 确认 `message.tokens` 事件已消失 | 5min | — |
| H5 | 多回合场景验证（两次 prompt） | 30min | 同 H1 |
| H6 | 错误路径验证（断网触发 ProviderError） | 15min | — |
| H7 | OpenChamber 前端渲染人工验证 | 1h | 需 openchamber-feat-dev 启动 |

### I — 集成测试（P1）

> 目标：自动化回归保护
> 方案详见 [12-integration-test-plan.md](12-integration-test-plan.md)

| # | 任务 | 文件 | 用例数 |
|---|---|---|---|
| I1 | BlockTracker 单元测试 | `foundation/stream-event/src/block_tracker.rs` | 6 |
| I2 | 全链路参数化测试（BlockTracker → translator → SSE buffer） | `apps/server/tests/stream_integration.rs`（新建） | 6 |
| I3 | 多回合 agent loop | 同上 | 1 |
| I4 | 错误路径 | 同上 | 4 |
| I5 | 状态重置 | 同上 | 1 |

### 字段 gap 补齐（P1-P2）

> 目标：Loom SSE 与 OpenCode v2 schema 逐字段一致
> 依据：[附录 A](appendix-a-opencode-v2-schema.md) §A.8

| # | Gap | 当前 Loom | OpenCode v2 | 优先级 |
|---|---|---|---|---|
| G1 | Token 结构 | `{ prompt, completion, total, cached }` | `{ input, output, reasoning, cache: { read, write } }` | P1 |
| G2 | Delta 增量事件 | `message.part.updated`（累积文本） | `message.part.delta`（增量）| P1（任务 X1） |
| G3 | step-start 携带 agent/model | 不发送 | `agent`, `model` | P2 |
| G4 | step-finish 携带 cost | 不发送 | `cost: number` | P2 |
| G5 | BlockEnd 携带全量文本 | 不携带 | `text` 完整 | P2 |

### OpenChamber 前端适配（P1）

> 目标：OpenChamber 正确消费 Loom SSE
> 步骤详见 [附录 C](appendix-c-openchamber-integration.md)

| # | 任务 | 文件 |
|---|---|---|
| OC1 | 移除 `message.tokens` 事件监听 | openchamber-feat-dev SSE handler |
| OC2 | 新增 `step-start` / `step-finish` part 渲染 | UI 组件 |
| OC3 | 确认 `message.part.updated` 累积文本兼容 | SSE handler |
| OC4 | 新增 `session.error` 错误展示 | UI 组件 |

## 依赖关系

```
H（协议验证）─┬─ 需要可用的 LLM API key
              └─ H7 需要 openchamber-feat-dev

I（集成测试）── 不依赖外部 API，可立即开始

G1-G5（字段 gap）── G1/G2 阻塞 OpenChamber 完全兼容

OC1-OC4（前端适配）── 依赖 H 验证通过
```

## 建议执行顺序

1. **I1-I5**（集成测试）— 不依赖外部，立即开始
2. **H1-H6**（SSE 抓包）— 需 API key，可与 I 并行
3. **G1**（Token 结构对齐）— 改 `Usage` 字段名和结构
4. **G2 / X1**（Delta 增量事件）— translator 追加 `message.part.delta`
5. **H7**（OpenChamber 验证）— 前端适配完成后
6. **G3-G5**（低优先级字段补齐）
