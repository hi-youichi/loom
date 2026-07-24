# 修改清单汇总

> 返回 [README.md](README.md)

## 必须修改

### A. StreamEvent 枚举改造

| # | 文件 | 修改内容 |
|---|---|---|
| A1 | `foundation/stream-event/src/types/stream_event.rs` | 替换 `Messages { chunk: MessageChunk, metadata }` → `TextDelta { content, metadata }` + `ReasoningDelta { id, content, metadata }` |
| A2 | 同上 | 追加 `TextBlockStart { metadata }`、`TextBlockEnd { metadata }` |
| A3 | 同上 | 追加 `ReasoningBlockStart { id, metadata }`、`ReasoningBlockEnd { id, metadata }` |
| A4 | 同上 | 追加 `TurnStart`、`TurnFinish { reason, usage: Usage }` |
| A5 | 同上 | 追加 `ToolError { call_id, error }`、`ProviderError { message }`、`Finish` |
| A6 | 同上 | 删除 `Usage { ... }` 变体（usage 折叠进 `TurnFinish`） |
| A7 | 同上或新模块 | 定义 `Usage` 结构体：`{ prompt_tokens, completion_tokens, total_tokens, cached_tokens }` |

### B. 删除 MessageChunk 体系

| # | 文件 | 修改内容 |
|---|---|---|
| B1 | `foundation/stream-event/src/types/stream_event.rs` | 从 `StreamEvent` 枚举中移除 `Messages` 变体（保留 `MessageChunk`/`MessageChunkKind`/`StreamSink` 作为 `BlockTrackerSink` 的输入接口） |
| B2 | `foundation/stream-event/src/wire/convert.rs` | 将 `StreamEvent::Messages` 转换路径（L70/L232）改为按 `TextDelta` / `ReasoningDelta` 分流；更新测试（L406/L422/L530/L556/L854） |
| B3 | `foundation/stream-event/src/wire/protocol.rs` | 移除或重映射 `ProtocolEvent::MessageChunk`（L50/L192/L517） |
| B4 | `foundation/stream-event/src/sink/stream_writer.rs` | 将 `StreamEvent::Messages`（L139/L160）改为直接发 `TextDelta` |
| B5 | `foundation/stream-event/src/sink/event_sink.rs` | 删除整个文件（用途仅为 `MessageChunk → Messages` 转换） |
| B6 | `foundation/stream-event/src/lib.rs` | 更新 re-exports，移除 `MessageChunk` / `MessageChunkKind` / `StreamSink` |

### C. BlockTracker

| # | 文件 | 修改内容 |
|---|---|---|
| C1 | `foundation/stream-event/src/block_tracker.rs`（新建） | `BlockTracker<S>`：`on_text_delta` / `on_reasoning_delta` / `on_finish` / `close_current` |
| C2 | `foundation/stream-event/src/lib.rs` | 注册 `mod block_tracker` + re-export |

### D. Provider SSE 映射

| # | 文件 | 修改内容 |
|---|---|---|
| D1 | `agent/agent-core/src/agent/react/think_node.rs` | 新建 `BlockTrackerSink`（实现 `StreamSink` trait），内部持有 `BlockTracker`，将 `MessageChunk` 按 `is_thinking()` 路由到 `on_text_delta`/`on_reasoning_delta`；stream 首个 SSE 前发 `TurnStart`，`finish_reason` 到达时发 `TurnFinish` + `Finish` |
| D2 | 同上 | 将 `stream_usage` / `stream_finish_reason` 聚合为 `Usage` 结构体，通过 `TurnFinish` 一次性送出 |

### E. Translator 重写

| # | 文件 | 修改内容 |
|---|---|---|
| E1 | `apps/server/src/state.rs` | `AppState` 追加 `active_text: RwLock<HashMap<String, String>>` + `active_reasoning: RwLock<HashMap<String, HashMap<String, String>>>` |
| E2 | `apps/server/src/translator.rs` | 删除 `translate_chunk`（L410）；`translate_stream_event` 中 `Messages` arm 替换为 12 个新 arm（见下） |
| E3 | 同上 | `TextBlockStart` arm：创建 text part，写入 `active_text[msg_id]` |
| E4 | 同上 | `TextDelta` arm：按 `active_text[msg_id]` 追加文本，emit `message.part.updated` |
| E5 | 同上 | `TextBlockEnd` arm：`finalize_text_part`，移除 `active_text[msg_id]` |
| E6 | 同上 | `ReasoningBlockStart` arm：创建 reasoning part，写入 `active_reasoning[msg_id][id]` |
| E7 | 同上 | `ReasoningDelta` arm：按 `active_reasoning[msg_id][id]` 追加，emit |
| E8 | 同上 | `ReasoningBlockEnd` arm：按 `id` 调用 `finalize_reasoning_part`，移除映射 |
| E9 | 同上 | `TurnStart` arm：创建 `step-start` part |
| E10 | 同上 | `TurnFinish` arm：`finalize_all_reasoning_parts`，创建 `step-finish` part（含 usage） |
| E11 | 同上 | `ToolCall` arm：移除 `finalize` 调用（block end 事件已先行到达） |
| E12 | 同上 | `ToolError` arm：调用 `fail_tool_call` |
| E13 | 同上 | `ProviderError` arm：进入错误处理路径 |
| E14 | 同上 | `Finish` arm：no-op |
| E15 | 同上 | 新增 `finalize_text_part`、`finalize_reasoning_part`、`finalize_all_reasoning_parts` |
| E16 | 同上 | 删除 `close_open_text_parts`（L343）；调用方改用 `finalize_all_reasoning_parts` |
| E17 | 同上 | 更新单元测试：移除 `translate_chunk` / `MessageChunk` 引用（L520/L553/L554/L555/L588/L589/L608/L616/L828/L829/L852/L857/L861） |

### F. 调用方更新

| # | 文件 | 修改内容 |
|---|---|---|
| F1 | `apps/server/src/handlers/session.rs` | `close_open_text_parts` 调用（L369、L1337）改为 `finalize_all_reasoning_parts` |
| F2 | `apps/server/src/translator.rs` | `translate_and_emit`（L68）签名不变；内部 dispatch 移除 `Messages → translate_chunk` 分支 |

### G. Agent runner

| # | 文件 | 修改内容 |
|---|---|---|
| G1 | `agent/agent-core/src/agent/react/`（think node） | stream 开始时 emit `TurnStart`；stream 结束时 emit `TurnFinish` + `Finish`（由 BlockTracker.on_finish 产出 block end） |
| G2 | `agent/agent-core/src/run/runner.rs` 或 `agent.rs` | `run` 开头清除 `active_text[msg_id]` + `active_reasoning[msg_id]` |
| G3 | 同上 | 错误路径设置 `message.error` + 发送 `session.error` 事件 |

## 不需要修改

| 文件 | 原因 |
|---|---|
| `apps/server/src/translator.rs` tool part 核心逻辑 | 状态机已与 OpenCode 对齐（`create_or_update_tool_part`） |
| `apps/server/src/sse.rs` | SSE 序列化逻辑不受影响 |
| `foundation/llm/src/client/openai_compat/stream.rs` | SSE DTO 已包含 `content` + `reasoning_content` 字段，无需改动 |

## 测试策略

### T1. `stream-event` crate（A/C）

| 测试 | 文件 | 验证点 |
|---|---|---|
| enum 序列化/反序列化 | `types/stream_event.rs` `#[cfg(test)]` | `TextDelta`、`ReasoningDelta`、`TextBlockStart/End`、`ReasoningBlockStart/End`、`TurnStart`、`TurnFinish`、`ToolError`、`ProviderError`、`Finish` 的 serde round-trip |
| BlockTracker 状态机 | `block_tracker.rs` `#[cfg(test)]` | `None → Text → Text → Reasoning → Text → Finish` 序列产出正确事件；`close_current` 在 `None` 时不产出 |
| BlockTracker 边界 | 同上 | 连续 `on_text_delta` 只产出一个 `TextBlockStart`；类型翻转时先 `End` 旧 block |
| Wire 协议 round-trip | `wire/convert.rs` | `TextDelta`/`ReasoningDelta` 的 `StreamEvent ↔ ProtocolEvent` 双向转换（替代旧 `MessageChunk` 测试） |

### T2. `translator.rs`（E）

| 测试 | 验证点 |
|---|---|
| text block 生命周期 | `TextBlockStart → TextDelta ×2 → TextBlockEnd` 产出一个 `type: "text"` part，`time.start` 和 `time.end` 均设置 |
| reasoning block 生命周期 | `ReasoningBlockStart { id: "r0" } → ReasoningDelta ×2 → ReasoningBlockEnd { id: "r0" }` 产出一个 `type: "reasoning"` part，按 id 路由 |
| 并发 reasoning | 两个 `ReasoningBlockStart`（不同 id）交替 `ReasoningDelta`，各自独立追加 |
| 回合边界 | `TurnStart` → ... → `TurnFinish { reason, usage }` 产出 `step-start` + `step-finish` part；`step-finish` 的 `tokens` 字段正确 |
| 错误路径 | `ToolError` 调用 `fail_tool_call`；`ProviderError` 进入错误处理 |
| 孤儿 delta | `TextDelta` 无前置 `TextBlockStart` → 静默丢弃；`ReasoningDelta { id: "unknown" }` → 静默丢弃 |
| 状态重置 | run 结束后 `active_text` 和 `active_reasoning` 为空 |

### T3. `llm_client.rs`（D）

| 测试 | 验证点 |
|---|---|
| OpenAI SSE → StreamEvent | 模拟 `delta.content` + `delta.reasoning_content` 交替的 SSE 流，验证 BlockTracker 产出的 `TextBlockStart/End` 和 `ReasoningBlockStart/End` 时序正确 |
| Usage 聚合 | `finish_reason` 到达时 `TurnFinish.usage` 字段包含 `prompt_tokens` / `completion_tokens` / `total_tokens` / `cached_tokens` |

### T4. 已有测试更新（E17/B2）

以下测试文件需移除 `translate_chunk` / `MessageChunk` / `MessageChunkKind` 引用：

| 文件 | 行号 |
|---|---|
| `apps/server/src/translator.rs` `#[cfg(test)]` | L520, L553-L555, L588-L589, L608, L616, L828-L829, L852, L857, L861 |
| `foundation/stream-event/src/wire/convert.rs` `#[cfg(test)]` | L406, L422, L530, L556, L854 |

## 协议验证（H — 完成后才算对齐目标达成）

> 设计依据：[11-protocol-impact.md](11-protocol-impact.md)、[附录 A](appendix-a-opencode-v2-schema.md)、[附录 B](appendix-b-sse-payload-examples.md)
> 前置条件：Loom server 已启动（`cargo run --bin loom-server -- serve --host 127.0.0.1 --port 18081`）

### H1-H6：SSE 协议抓包验证

**操作步骤**：

```bash
# 1. 启动 Loom server
cargo run --bin loom-server -- serve --host 127.0.0.1 --port 18081

# 2. 创建 session
curl -X POST http://127.0.0.1:18081/api/session -H "Content-Type: application/json" -d '{"id":"test-sess"}'

# 3. 连接 SSE 流（后台）
curl -N http://127.0.0.1:18081/api/session/test-sess/stream > sse_output.txt &

# 4. 发送 prompt（触发 reasoning + text + tool）
curl -X POST http://127.0.0.1:18081/api/session/test-sess/prompt -H "Content-Type: application/json" -d '{"text":"list files in current directory"}'

# 5. 等待完成后检查输出
cat sse_output.txt
```

**逐项验证**：

| # | 验证内容 | grep 命令 | 通过标准 |
|---|---|---|---|
| H1 | text block SSE 序列 | `grep "type.*text" sse_output.txt` | 有空 text part（start）→ 累积 text part（delta）→ 加盖 `time.end` 的 text part（end） |
| H2 | reasoning block SSE 序列 | `grep "type.*reasoning" sse_output.txt` | 同 H1，按 reasoning id 独立 |
| H3 | step-start / step-finish | `grep "type.*step-" sse_output.txt` | 有 `step-start` part + `step-finish` part（含 `tokens.prompt`/`tokens.completion`） |
| H4 | message.tokens 已删除 | `grep "message.tokens" sse_output.txt` | **无输出**（事件不存在） |
| H5 | 多回合序列 | 发送第二个 prompt 后检查 | 两个 `step-start` + 两个 `step-finish` |
| H6 | 错误路径 | 断开网络触发 provider 错误 | `session.error` 事件存在 |

### H7：OpenChamber 前端渲染验证

**操作步骤**：

1. 启动 Loom server（同上）
2. 启动 OpenChamber：
   ```powershell
   $env:OPENCODE_HOST = "http://127.0.0.1:18081"
   $env:OPENCODE_SKIP_START = "true"
   cd C:\Users\heycj\dev\openchamber-feat-dev
   bun run packages/web/dev
   ```
3. 在 OpenChamber 中发送 prompt："hello"
4. **人工验证**：
   - text part 流式显示
   - reasoning part 以灰色块显示（如 provider 返回）
   - tool part 显示工具卡片（如触发工具）
   - step-start 渲染为回合分隔线
   - step-finish 显示 token 用量
   - 无 `message.tokens` 相关报错
5. 发送第二个 prompt：
   - 第二个回合显示 "Step 2" 分隔线
   - 第一个回合的 parts 保持不变

## 集成测试（I — 实现方案见 [12-integration-test-plan.md](12-integration-test-plan.md)）

| # | 文件 | 内容 |
|---|---|---|
| I1 | `foundation/stream-event/src/block_tracker.rs` | L3-1: BlockTracker 单元测试（6 个） |
| I2 | `apps/server/tests/stream_integration.rs`（新建） | L3-2: 全链路参数化测试（I1-I6） |
| I3 | 同上 | L3-3: 多回合 agent loop |
| I4 | 同上 | L3-4: 错误路径（4 个） |
| I5 | 同上 | L3-5: 状态重置 |

## 独立增强（不阻塞核心重构）

| # | 文件 | 修改内容 |
|---|---|---|
| X1 | `translator.rs` | 追加路径发送 `message.part.delta` 事件（仅 delta 字段） |
| X2 | `translator.rs` / `state.rs` | PartID 改为单调递增（ULID 风格） |
| X3 | `translator.rs` | cleanup 中 tool 250ms 宽限期等待在途工具 |
| X4 | `translator.rs` | aborted tool 标记 `interrupted: true` 元数据 |
| X5 | `translator.rs` | `failToolCall` 权限拒绝时设置 `blocked`，控制 agent loop 终止 |
| X6 | `translator.rs` | `step-finish` 生成 patch part（需 snapshot 机制，见 [09-snapshot-patch.md](09-snapshot-patch.md)） |
| X7 | 全局 | Context window compaction（见 [10-compaction.md](10-compaction.md)） |
| X8 | `foundation/llm/` | LLM 调用层添加重试策略（指数退避） |
