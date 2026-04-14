# Send / Stop Button Design

## Overview

MessageComposer 的发送按钮在 agent 运行时切换为停止按钮，用户可随时取消运行。

## State Machine

```
┌─────────┐  click send  ┌───────────┐
│  IDLE   │─────────────▶│ STREAMING │
│ (arrow) │              │  (stop)   │
└─────────┐◀─────────────└───────────┘
           run_end / error / cancelled
```

- **IDLE**：箭头图标，点击发送消息
- **STREAMING**：方块图标，点击取消运行；agent 完成后自动回到 IDLE

## Protocol

### Request: `cancel_run`

```json
{
  "type": "cancel_run",
  "id": "<request-id>",
  "run_id": "<run-id>"
}
```

### Response: `cancel_run_ack`

```json
{
  "type": "cancel_run_ack",
  "id": "<request-id>",
  "run_id": "<run-id>"
}
```

收到 ack 后，server 会通过 CancellationToken 中止 agent，随后发送正常的 `run_end` 或 `error`（`run cancelled`）。

## Backend Changes

### 1. Protocol (`loom/src/protocol/requests.rs`)

- `ClientRequest` 枚举新增 `CancelRun(CancelRunRequest)`
- `CancelRunRequest` 包含 `id` + `run_id`

### 2. Protocol (`loom/src/protocol/responses.rs`)

- 新增 `CancelRunResponse`（`type: "cancel_run_ack"`）

### 3. Serve (`serve/src/`)

- **Active Run Registry**：connection 级别维护 `HashMap<String, RunCancellation>`，以 `run_id` 为 key
- `serve/src/run/request.rs`：`RunOptions.cancellation` 从 `None` 改为创建 `RunCancellation` 实例
- `serve/src/run/mod.rs`：`handle_run` 返回 `(run_id, RunCancellation)`，注册到 registry
- `serve/src/connection.rs`：处理 `CancelRun` 请求 → 从 registry 取出 `RunCancellation` → 调用 `cancel()` → 回复 ack
- run 结束时（无论完成/取消/出错）从 registry 移除

### Cancellation Flow (Rust)

```
cancel_run request
  → lookup RunCancellation by run_id
  → RunCancellation.cancel()
  → CancellationToken cancelled
  → agent runtime checks token at node boundaries
  → RunCompletion::Cancelled returned
  → delivery.rs sends error response "run cancelled"
```

## Frontend Changes

### 1. Types (`web/src/types/protocol/loom.ts`)

```ts
type CancelRunRequest = { type: 'cancel_run'; id: string; run_id: string }
type CancelRunResponse = { type: 'cancel_run_ack'; id: string; run_id: string }
```

### 2. Connection (`web/src/services/connection.ts`)

- 新增 `cancelRun(runId: string): Promise<void>`
- 通过 WebSocket 发送 `CancelRunRequest`
- 等待 `CancelRunResponse` ack

### 3. useChat Hook (`web/src/hooks/useChat.ts`)

- 暴露 `cancel(): void` 方法
- 内部调用 `connection.cancelRun(activeRunId)`
- 维护 `activeRunId` 状态（从 `run_stream_event.id` 获取）

### 4. MessageComposer (`web/src/components/MessageComposer.tsx`)

Props 新增：

```ts
type MessageComposerProps = {
  disabled?: boolean
  isStreaming?: boolean
  onSend: (text: string) => Promise<void>
  onCancel?: () => void
  selectedModel?: string
  onModelChange?: (model: string) => void
}
```

按钮渲染逻辑：

```
isStreaming ? <StopIcon onClick={onCancel} /> : <SendIcon type="submit" />
```

### 5. CSS (`web/src/index.css`)

停止按钮样式与发送按钮共用 `.composer__button` 容器，图标使用 `■`（方形 SVG）。

## Interaction Flow

### Normal Send

1. 用户输入 → 点击 ↑ → `onSend(text)` → `isStreaming=true`
2. 收到 `run_end` → `isStreaming=false` → 按钮恢复 ↑

### Cancel

1. 运行中 → 点击 ■ → `onCancel()` → 发送 `cancel_run`
2. 后端 `CancellationToken.cancel()` → agent 中止
3. 后端发送 `error: "run cancelled"` → 前端 `isStreaming=false` → 按钮恢复 ↑

### Error

1. 运行中出错 → 后端发送 `error` → 前端 `isStreaming=false` → 按钮恢复 ↑

## Development Plan

### Phase 1: Backend Protocol

| # | Task | File | Status |
|---|------|------|--------|
| 1.1 | 新增 `CancelRunRequest` 结构体 | `loom/src/protocol/requests.rs` | ⬜ |
| 1.2 | `ClientRequest` 枚举新增 `CancelRun` 变体 | `loom/src/protocol/requests.rs` | ⬜ |
| 1.3 | 新增 `CancelRunResponse` 结构体 | `loom/src/protocol/responses.rs` | ⬜ |
| 1.4 | `ServerResponse` 枚举新增 `CancelRun` 变体 | `loom/src/protocol/responses.rs` | ⬜ |
| 1.5 | 确认 `RunCancellation` 公共导出 | `loom/src/lib.rs` | ⬜ |

### Phase 2: Backend Serve

| # | Task | File | Status |
|---|------|------|--------|
| 2.1 | 定义 `ActiveRunRegistry`（`HashMap<String, RunCancellation>` + 插入/移除/取消方法） | `serve/src/connection.rs` | ⬜ |
| 2.2 | `prepare_run` 创建 `RunCancellation` 实例，设置 `opts.cancellation` | `serve/src/run/request.rs` | ⬜ |
| 2.3 | `handle_run` 返回 `run_id` + `RunCancellation`，注册到 registry，run 结束后移除 | `serve/src/run/mod.rs` | ⬜ |
| 2.4 | `handle_request_and_send` 处理 `CancelRun`：查找 registry → 调用 `cancel()` → 回复 ack | `serve/src/connection.rs` | ⬜ |
| 2.5 | cargo clippy 通过 | | ⬜ |

### Phase 3: Frontend Connection

| # | Task | File | Status |
|---|------|------|--------|
| 3.1 | 新增 `CancelRunRequest` / `CancelRunResponse` 类型 | `web/src/types/protocol/loom.ts` | ⬜ |
| 3.2 | `LoomConnection` 新增 `cancelRun(runId)` 方法，发送请求并等待 ack | `web/src/services/connection.ts` | ⬜ |

### Phase 4: Frontend UI

| # | Task | File | Status |
|---|------|------|--------|
| 4.1 | `useChat` 维护 `activeRunId` 状态，暴露 `cancel()` | `web/src/hooks/useChat.ts` | ⬜ |
| 4.2 | `MessageComposer` 接收 `isStreaming` / `onCancel`，条件渲染停止/发送图标 | `web/src/components/MessageComposer.tsx` | ⬜ |
| 4.3 | `ChatPage` 传递 `isStreaming` / `onCancel` 给 `MessageComposer` | `web/src/pages/ChatPage.tsx` | ⬜ |
| 4.4 | CSS：停止按钮图标样式 | `web/src/index.css` | ⬜ |

### Phase 5: E2E & Verification

| # | Task | File | Status |
|---|------|------|--------|
| 5.1 | 协议序列化/反序列化单元测试 | `loom/src/protocol/` | ⬜ |
| 5.2 | `ActiveRunRegistry` 单元测试（insert/cancel/remove） | `serve/src/connection.rs` | ⬜ |
| 5.3 | Playwright：按钮 idle/streaming/stop 状态切换 | `web/e2e/send-button.spec.ts` | ⬜ |
| 5.4 | `cargo clippy -- -D warnings` 通过 | | ⬜ |
| 5.5 | `npm run lint` + `npm run typecheck` 通过 | | ⬜ |

### Dependencies

```
Phase 1 ──▶ Phase 2 ──▶ Phase 3 ──▶ Phase 4 ──▶ Phase 5
(protocol)   (serve)     (conn)      (UI)        (test)
```

Phase 1-2 可与 Phase 3-4 并行开发，Phase 5 最后。

## Test Plan

### Backend: Unit Tests

#### 协议测试 (`loom/src/protocol/`)

| Case | Description |
|------|-------------|
| `cancel_run_request_roundtrip` | `CancelRunRequest` JSON 序列化/反序列化 |
| `cancel_run_response_roundtrip` | `CancelRunResponse` JSON 序列化/反序列化 |
| `client_request_deserialize_cancel` | `ClientRequest` 反序列化 `type: "cancel_run"` |

#### Registry 测试 (`serve/src/connection.rs`)

| Case | Description |
|------|-------------|
| `insert_and_remove` | 注册 run → 移除 → map 为空 |
| `cancel_existing_run` | 注册 run → cancel → 确认 CancellationToken 已取消 |
| `cancel_unknown_run` | cancel 不存在的 run_id → 返回 error |
| `auto_cleanup_on_run_end` | run 结束后 registry 自动移除对应条目 |

---

### Frontend: `web/e2e/send-button.spec.ts`

启动 test server（带 mock LLM），Playwright 操控 ChatPage DOM，验证按钮状态切换。

#### 测试步骤

```
1. 启动 mock LLM server（慢 SSE）
2. 打开 ChatPage
3. textarea 输入消息
4. 点击发送按钮（↑，aria-label="Send message"）
5. 断言按钮变为停止图标，aria-label 为 "Stop"
6. 断言 textarea 被清空
7. 断言消息列表中出现用户消息
8. 点击停止按钮（■）
9. 断言按钮恢复为发送图标（↑），aria-label 为 "Send message"
10. 断言消息列表中出现 assistant 消息（可能包含 error 或 partial reply）
```

#### 测试用例

| Case | Description |
|------|-------------|
| `send button shows while idle` | 初始状态按钮为发送图标，disabled（无输入） |
| `send button enables with input` | 输入文字后按钮变为可点击 |
| `send button becomes stop while streaming` | 点击发送后按钮变为停止图标 |
| `stop button cancels and restores send` | 点击停止后按钮恢复发送图标 |
| `send button restores after run completes` | agent 正常完成后按钮恢复发送图标（不点停止） |

#### Mock LLM Server

复用 `web/e2e/helpers/mock-llm-server.ts`，配置慢 SSE 模式：

```ts
// 每个 SSE chunk 延迟 300ms
mockLlmServer.addStreamingResponse([
  { delta: 'Hello', delay: 300 },
  { delta: ' world', delay: 300 },
])
```

#### Selectors

```ts
const sendButton = page.locator('.composer__button')
const textarea = page.locator('#message-input')

// 判断状态
await expect(sendButton).toHaveAttribute('aria-label', 'Send message')   // idle
await expect(sendButton).toHaveAttribute('aria-label', 'Stop')          // streaming
```

#### 注意事项

- Mock LLM 的响应要足够慢，否则 run 在 stop 按钮出现前就结束了
- 需要处理 WebSocket 连接建立的时机：test server 启动后再打开页面
- `e2e/send-button.spec.ts` 已存在部分测试，在此基础上扩展 stop 相关用例
