# 附录 C：OpenChamber 前端集成指南

> 返回 [README.md](README.md)
> 目标：OpenChamber 前端正确消费 Loom SSE 事件流，渲染与 OpenCode 一致的 part 序列。

## C.1 协议兼容策略

Loom SSE 使用 OpenCode **v1 兼容路径**（`message.part.updated`），不直接发 v2 `session.next.*` 事件。
OpenChamber 前端需同时处理两种来源：

| 来源 | 事件名 | 传输方式 |
|---|---|---|
| Loom | `message.part.updated` | 累积文本（`part.text` 完整） |
| OpenCode 原生 | `session.next.text.delta` 等 | 增量文本（`delta` 字段） |

**建议**：OpenChamber 前端在 SSE 适配层做事件归一化，统一为内部 part 状态。

## C.2 Part 类型渲染

| `part.type` | 渲染 | 来源事件 | 关键字段 |
|---|---|---|---|
| `text` | Markdown 渲染 | `TextBlockStart` → `TextDelta ×N` → `TextBlockEnd` | `text`, `time.start`, `time.end` |
| `reasoning` | 灰色可折叠推理块 | `ReasoningBlockStart` → `ReasoningDelta ×N` → `ReasoningBlockEnd` | `text`, `metadata.loom_node` |
| `tool` | 工具调用卡片 | `ToolCall` → `ToolStart` → `ToolOutput ×N` → `ToolEnd` | `tool`, `state.status`, `input`, `output` |
| `step-start` | 回合分隔线 + "Step N" 标签 | `TurnStart` | `time.start` |
| `step-finish` | Token 用量标签 | `TurnFinish` | `tokens: { prompt, completion, total, cached }`, `reason` |

## C.3 step-start / step-finish 渲染方式

```
┌─────────────────────────────────────────┐
│  ── Step 1 ───────────────────────────  │  ← step-start part
│                                         │
│  [reasoning] Let me think...            │  ← reasoning part
│  [text] Running ls                      │  ← text part
│  [tool] bash: ls                        │  ← tool part
│                                         │
│  ── Step 1 · 150 tokens · stop ───────  │  ← step-finish part
└─────────────────────────────────────────┘
```

- `step-start`：渲染为虚线分隔符 + 递增 Step 编号
- `step-finish`：渲染为 token 用量标签（`prompt` / `completion` / `total`），附 `reason`

## C.4 message.part.updated 消费逻辑

```typescript
function handlePartUpdated(event: SSEEvent) {
  const { part } = event.data;

  const existing = parts.findIndex(p => p.id === part.id);
  if (existing >= 0) {
    // 更新已有 part（文本累积、状态变更、时间戳补盖）
    parts[existing] = { ...parts[existing], ...part };
  } else {
    // 新 part（插入末尾）
    parts.push(part);
  }

  // 触发 UI 重渲染
  emit("parts-updated", parts);
}
```

## C.5 message.tokens 事件移除

Loom 改造后不再发送 `message.tokens` 事件。Token 数据已嵌入 `step-finish` part 的 `tokens` 字段。

```typescript
// 旧代码（需移除）
case "message.tokens":
  usage = event.data;

// 新代码
case "message.part.updated":
  if (part.type === "step-finish") {
    usage = part.tokens;  // { prompt, completion, total, cached }
  }
```

## C.6 错误处理

| SSE 事件 | 前端行为 |
|---|---|
| `session.error` | 显示错误横幅，标记当前 message 为 error 状态 |
| `message.part.updated` (tool, status: error) | 工具卡片标红，显示 error 字段 |

## C.7 验证步骤

1. 启动 Loom server：`cargo run --bin loom-server`
2. 启动 OpenChamber：`OPENCODE_HOST=http://127.0.0.1:18081 bun run packages/web/dev`
3. 发送 prompt："hello"
4. 验证：
   - text part 流式显示文本
   - reasoning part（如 provider 返回）以灰色块显示
   - tool part（如触发工具）显示调用卡片
   - step-start 渲染为分隔线
   - step-finish 显示 token 用量
5. 发送第二个 prompt（多回合场景）：
   - 第二个 step-start 显示 "Step 2"
   - 第一个回合的 parts 保持不变
