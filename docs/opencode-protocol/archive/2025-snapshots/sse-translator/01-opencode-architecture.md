# OpenCode 核心架构

> 返回 [README.md](README.md)

## 1.1 ProcessorContext 状态

> 参考（无直接开发任务）

```typescript
// packages/opencode/src/session/processor.ts:112-126
interface ProcessorContext {
  // ...
  currentText: SessionV1.TextPart | undefined       // 当前活跃的 text part（单例）
  reasoningMap: Record<string, SessionV1.ReasoningPart>  // 活跃 reasoning parts（按 stream ID 索引）
}
```

- `currentText`：同一时刻最多只有 **一个** 活跃 text part
- `reasoningMap`：按 LLM stream 返回的 reasoning ID 索引，支持并发 reasoning blocks（Anthropic adaptive thinking）

## 1.2 每次 LLM Stream 运行前重置

> 参考（对应 G2）

```typescript
// packages/opencode/src/session/processor.ts:498-499
// process() 开头：
ctx.currentText = undefined
ctx.reasoningMap = {}
```

- **每个 step（LLM 调用回合）开始时，活跃 part 状态被清空**
- 上一个 step 的 part 已经在 `text-end` / `reasoning-end` 中收尾，这里只是防御性清除

## 1.3 事件类型映射

> 参考（无直接开发任务）

LLM SDK 返回的流式事件：

| LLM SDK 事件 | OpenCode 处理 | Part 动作 |
|---|---|---|
| `text-start` | 创建 `ctx.currentText` | `updatePart`（完整写入） |
| `text-delta` | `ctx.currentText.text += delta` | `updatePartDelta`（增量） |
| `text-end` | 设置 `time.end`，`ctx.currentText = undefined` | `updatePart`（完整写入） |
| `reasoning-start` | 创建 `reasoningMap[id]` | `updatePart`（完整写入） |
| `reasoning-delta` | `reasoningMap[id].text += delta` | `updatePartDelta`（增量） |
| `reasoning-end` | 设置 `time.end`，从 map 删除 | `updatePart`（完整写入） |
| `tool-input-start` | 创建 pending tool part（含 summary guard） | `updatePart`（完整写入） |
| `tool-input-delta` | `ensureToolCall`（幂等，已存在则跳过） | 无（或已创建） |
| `tool-input-end` | `ensureToolCall`（幂等） | 无 |
| `tool-call` | 更新 tool part → running | `updatePart`（完整写入） |
| `tool-result` | 更新 tool part → completed（含图片归一化） | `updatePart`（完整写入） |
| `tool-error` | 更新 tool part → error（独立于 tool-result） | `updatePart`（完整写入） |
| `provider-error` | throw Error → 进入 retry/halt 管线 | 无（异常路径） |
| `step-start` | 创建 `step-start` part | `updatePart`（完整写入） |
| `step-finish` | 收尾 reasoning + patch part + compaction check | `finishReasoning` + `updatePart` |
| `finish` | no-op（显式忽略） | 无 |
