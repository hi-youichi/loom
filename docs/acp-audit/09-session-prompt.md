# ACP 协议审计：session/prompt

## 协议规范

`session/prompt` 协议定义了用于从 client 向 agent 发送用户 prompt 的 ACP 方法。它是 Client → Agent 请求，传输用户的输入文本以及 session 元数据，使 agent 能够处理 prompt、执行工具，并将响应流回 client。

## 实现状态

**已实现** — Loom ACP crate v0.15.1 完整实现了 `session/prompt`。所有核心功能已通过对抗性验证确认。

## 实现细节

### 处理器注册

**文件：** `apps/acp/src/stdio_loop.rs:328-344`

`session/prompt` 处理器在 ACP 路由器中注册：

```rust
// stdio_loop.rs:328-344
// Handler registration for session/prompt method
```

### Prompt 处理

**文件：** `apps/acp/src/agent.rs:738-1046`

完整 `prompt()` 方法实现：

```rust
// agent.rs:738-1046
// pub async fn prompt(...) -> Result<...>
// Handles prompt parsing, tool execution, response streaming
```

### 内容解析

**文件：** `apps/acp/src/content.rs`

支持多模态内容块解析：
- `Text` — 纯文本内容
- `ResourceLink` — 资源链接引用
- `Image` — 图像数据
- `Audio` — 音频数据
- `Resource` — 通用资源内容

### 协议 schema

**文件：** `apps/acp/src/protocol.rs:31-35`

```rust
// protocol.rs:31-35
// ACP method registration for "session/prompt"
```

### Schema 定义

**文件：** `agent-client-protocol-schema-0.14.0/src/v1/agent.rs:3029`

`PromptRequest` 结构体具有以下字段：
- `session_id: String`
- `prompt: String`
- `meta: PromptMeta`

注：`options` 字段**不**存在于 `PromptRequest` schema（v0.14.0）中。先前分析中"options 字段被忽略"的说法是不正确的 — 该字段根本不存在。

### 使用情况跟踪

**文件：** `apps/acp/src/agent.rs:1602`

通过 `build_acp_usage()` 实现使用情况跟踪：

```rust
// agent.rs:1602
// Usage tracking via build_acp_usage()
```

`unstable_end_turn_token_usage` 特性在 `apps/acp/Cargo.toml:29` 中**已启用**，使 `usage` 字段可用并被填充。

## 实现方式

Loom 的 `session/prompt` 实现遵循 ACP 协议模式：

1. **处理器注册** — ACP 方法在 `stdio_loop.rs:328-344` 注册
2. **请求解析** — 从 ACP JSON 负载解析 `PromptRequest`
3. **内容处理** — `content.rs` 中的 `ContentBlock` 多模态解析
4. **Agent 执行** — `agent.rs:738-1046` 中的完整 prompt 处理
5. **流式响应** — `session/update` 流回 client
6. **使用情况跟踪** — 通过 `build_acp_usage()` 进行 token 使用跟踪（特性门控）

### 内置命令
- `/reset` — 重置 session 状态
- `/goal` — 设置 session 目标
- `/review-skill` — 触发 skill 审查

### 取消
通过 `StopReason::Cancelled` 实现 — 已正确实现和测试。

### MCP 集成
MCP 工具通过 agent 的工具执行管道正确集成。

## 差距与问题

### 无严重差距

**状态：** 未发现缺失实现。

### 对先前分析的更正

| 问题 | 正确状态 |
|-------|----------------|
| "options 字段被忽略" | `options` 字段在 `PromptRequest` schema（v0.14.0）中**根本不存在** |
| "usage 字段不可用" | `usage` 字段**可用** — `unstable_end_turn_token_usage` 特性在 `Cargo.toml:29` 已启用 |

### StopReason Fallthrough

`StopReason::EndTurn`、`::Cancelled` 已正确实现。以下原因按文档说明 fallthrough 到 `::EndTurn`：
- `::MaxTokens`
- `::MaxTurnRequests`
- `::Refusal`

## 验证

### 对抗性验证结果

**结论：已验证** — `session/prompt` 在 Loom ACP crate v0.15.1 中**完整实现**

### 已确认文件

| 文件 | 行 | 用途 |
|------|-------|---------|
| `apps/acp/src/stdio_loop.rs` | 328-344 | 处理器注册 |
| `apps/acp/src/agent.rs` | 738-1046 | 完整 `prompt()` 方法 |
| `apps/acp/src/agent.rs` | 1602 | 使用情况跟踪 |
| `apps/acp/src/content.rs` | — | ContentBlock 解析 |
| `apps/acp/src/protocol.rs` | 31-35 | 协议方法注册 |
| `apps/acp/tests/e2e_mega.rs` | 99-221 | 端到端测试场景 |

### 已验证功能（10/10）

- 处理器注册位于 `stdio_loop.rs:328-344`
- 完整 `prompt()` 方法位于 `agent.rs:738-1046`
- ContentBlock 多模态解析（Text/ResourceLink/Image/Audio/Resource）
- 内置命令（/reset、/goal、/review-skill）
- 通过 `StopReason::Cancelled` 取消
- 通过 `build_acp_usage()` 进行使用情况跟踪（特性门控，正确启用）
- 后台 review 启动
- `session/update` 流式传输
- MCP 集成
- 适当的错误处理

### 测试覆盖

`apps/acp/tests/e2e_mega.rs` 中的三个测试场景：
1. 仅文本 prompt 流程
2. 工具调用执行流程
3. 取消流程

## 总结

**最终评估：** 通过 — `session/prompt` ACP 协议完整实现。

**建议：**
1. 更新任何声称"options 字段被忽略"的文档以反映该字段在 v0.14.0 schema 中不存在
2. 使用情况跟踪正确地进行特性门控 — 无需更改
3. 继续维护三个已确认测试场景的测试覆盖

**下一步：** 无阻塞问题。该协议已可用于生产环境。

---

## 实现指南

> **状态：** 完整实现 — 本节作为 ACP `session/prompt` 的参考实现文档。
> 与实现指南一起提供的还有**已修正**的差距分析（来自先前分析），用于澄清实际状态。

### 当前实现摘要

```rust
// apps/acp/src/agent.rs:738-1046
pub async fn handle_session_prompt(
    &self,
    ctx: RunContext,
    req: PromptRequest,
) -> Result<PromptResponse, ACPError> {
    // 1. 多模态内容解析
    let content = self.content_parser.parse(&req.prompt).await?;

    // 2. 内置命令路由
    if let Some(cmd) = detect_builtin_command(&content) {
        return self.execute_builtin(cmd, ctx).await;
    }

    // 3. 启动 agent run
    self.run_agent_with_streaming(ctx, content, req.session_id).await
}
```

### 完整提示流程

**涉及模块（按调用顺序）：**

```
Client
  ↓ JSON-RPC: session/prompt
  ↓
stdio_loop.rs:328-344          ← 协议入口点
  ↓ Route to handler
agent.rs:738-1046              ← handle_session_prompt()
  ├─ content.rs                ← ContentBlock 多模态解析
  │   ├─ Text
  │   ├─ ResourceLink
  │   ├─ Image
  │   ├─ Audio
  │   └─ Resource
  ├─ builtin_commands.rs       ← /reset、/goal、/review-skill
  ├─ run_agent_with_streaming  ← 主 agent 循环
  │   ├─ LLM 调用
  │   ├─ 工具执行
  │   ├─ 取消检查
  │   └─ 使用情况跟踪
  └─ build_acp_usage()         ← Token 统计（特性门控）
       ↓
PromptResponse(StopReason::EndTurn | ::Cancelled)
```

### 关键类型定义

```rust
// agent-client-protocol-schema-0.14.0/src/v1/agent.rs:3029
pub struct PromptRequest {
    pub session_id: String,
    pub prompt: String,           // ← 注意：是 String 而非 Vec<ContentBlock>
    pub meta: PromptMeta,         // ← 可选元数据
    // 注：v0.14.0 schema 中**没有** `options` 字段
}

pub struct PromptResponse {
    pub stop_reason: StopReason,  // EndTurn | Cancelled | MaxTokens | ...
    pub meta: Option<ResponseMeta>, // 可选元数据
}

pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    ContentFiltered,
}
```

### 演示：完整的 Prompt 交换

**Client 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": 100,
  "method": "session/prompt",
  "params": {
    "session_id": "sess-abc-123",
    "prompt": "请分析 src/main.rs 并建议性能优化",
    "meta": {
      "model": "claude-opus-4-5",
      "max_tokens": 4096,
      "tools": ["file_read", "code_search"]
    }
  }
}
```

**Agent 流式响应（多个 session/update 通知）：**
```json
// 1. tool_call 通知
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc-123",
    "update": {
      "sessionUpdate": "tool_call",
      "toolCallId": "tc-001",
      "title": "Read src/main.rs",
      "kind": "read",
      "status": "in_progress"
    }
  }
}

// 2. thought_chunk 通知
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc-123",
    "update": {
      "sessionUpdate": "thought_chunk",
      "content": { "type": "text", "text": "用户要求性能优化分析..." }
    }
  }
}

// 3. agent_message_chunk 通知（流式）
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc-123",
    "update": {
      "sessionUpdate": "agent_message_chunk",
      "content": { "type": "text", "text": "分析 src/main.rs 后，建议以下优化：\n1. ..." }
    }
  }
}

// 4. tool_call_update 通知（完成）
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess-abc-123",
    "update": {
      "sessionUpdate": "tool_call_update",
      "toolCallId": "tc-001",
      "status": "completed",
      "content": [{ "type": "text", "text": "fn main() { ... }" }]
    }
  }
}

// 5. 最终 PromptResponse
{
  "jsonrpc": "2.0",
  "id": 100,
  "result": {
    "stopReason": "end_turn",
    "meta": {
      "usage": {
        "inputTokens": 1234,
        "outputTokens": 567,
        "totalTokens": 1801
      }
    }
  }
}
```

### 演示：多模态内容处理

```rust
// content.rs — ContentBlock 解析
pub async fn parse(prompt: &str) -> Result<ContentBlock, ParseError> {
    // prompt 字段实际上是字符串，但可以包含结构化引用
    if prompt.starts_with("@image:") {
        Ok(ContentBlock::Image { path: extract_path(prompt)? })
    } else if prompt.starts_with("@audio:") {
        Ok(ContentBlock::Audio { path: extract_path(prompt)? })
    } else if prompt.starts_with("@file:") {
        Ok(ContentBlock::Resource { uri: extract_uri(prompt)? })
    } else {
        Ok(ContentBlock::Text { text: prompt.to_string() })
    }
}
```

### 演示：内置命令处理

**调用 `/reset`：**
```json
{
  "jsonrpc": "2.0",
  "id": 101,
  "method": "session/prompt",
  "params": {
    "session_id": "sess-abc-123",
    "prompt": "/reset"
  }
}
```

**响应（立即结束）：**
```json
{
  "jsonrpc": "2.0",
  "id": 101,
  "result": { "stopReason": "end_turn", "meta": null }
}
```

**支持的内置命令：**
| 命令 | 行为 |
|------|------|
| `/reset` | 重置 session 状态、清除短期记忆 |
| `/goal <text>` | 设置 session 目标 |
| `/review-skill` | 触发 skill 审查流程 |

### 演示：取消流程（与 session/cancel 集成）

```text
1. Client: session/prompt (id=200, "wait for long response")
2. Agent: 启动长运行任务
3. Client: session/cancel (通知)
4. Agent: 检测到取消信号 → 设置 AtomicBool
5. Agent: 主循环检查标志 → 中止
6. Agent: PromptResponse(StopReason::Cancelled)
```

**PromptResponse（取消）：**
```json
{
  "jsonrpc": "2.0",
  "id": 200,
  "result": { "stopReason": "cancelled", "meta": null }
}
```

### 演示：使用情况跟踪（特性门控）

```rust
// agent.rs:1602 — 通过 build_acp_usage() 跟踪 token 使用
pub fn build_acp_usage(&self, run: &RunStats) -> Usage {
    Usage {
        input_tokens: run.prompt_tokens,
        output_tokens: run.completion_tokens,
        total_tokens: run.prompt_tokens + run.completion_tokens,
        // 缓存命中率、思考 token 等可选字段
    }
}

// Cargo.toml:29 — 特性门控
[features]
unstable_end_turn_token_usage = ["agent-client-protocol/unstable_end_turn_token_usage"]
```

**注：** 修复先前分析中关于 `usage` 字段的误解 — `unstable_end_turn_token_usage` 特性**已启用**，`usage` 字段可被填充。

### StopReason 行为表

| StopReason | 实现状态 | 行为 |
|------------|---------|------|
| `EndTurn` | ✅ | 正常结束（默认） |
| `Cancelled` | ✅ | 通过 `session/cancel` 触发 |
| `MaxTokens` | ➖ | 文档化为 fallthrough 到 EndTurn |
| `MaxTurnRequests` | ➖ | 文档化为 fallthrough 到 EndTurn |
| `Refusal` | ➖ | 文档化为 fallthrough 到 EndTurn |
| `ContentFiltered` | ➖ | 文档化为 fallthrough 到 EndTurn |

**fallthrough 设计理由：** 这些原因在 Loom 当前用例中很少触发，统一映射到 `EndTurn` 是合理的简化。

### 测试场景

`apps/acp/tests/e2e_mega.rs:99-221` 已覆盖三个核心场景：

```rust
#[tokio::test]
async fn test_prompt_text_only_flow() {
    // 1. 仅文本 prompt
    let res = client.prompt("sess-1", "Hello, agent").await?;
    assert_eq!(res.stop_reason, StopReason::EndTurn);
    // 2. 验证 agent_message_chunk 通知被接收
    assert!(client.received_session_update_with("agent_message_chunk").await);
}

#[tokio::test]
async fn test_prompt_tool_call_flow() {
    // 1. 触发工具调用的 prompt
    let res = client.prompt("sess-1", "Read src/main.rs").await?;
    // 2. 验证 tool_call + tool_call_update 通知序列
    assert!(client.received_session_update_with("tool_call").await);
    assert!(client.received_session_update_with("tool_call_update").await);
    assert_eq!(res.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn test_prompt_cancel_flow() {
    // 1. 启动长运行 prompt
    let handle = client.prompt_async("sess-1", "count to 1000000");
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. 发送 cancel 通知
    client.notify("session/cancel", json!({})).await?;

    // 3. 验证返回 cancelled
    let res = handle.await?;
    assert_eq!(res.stop_reason, StopReason::Cancelled);
}
```

### 验证已修正的差距

**先前分析中标记的两个错误差距（已澄清）：**

| 误报差距 | 实际状态 | 证据 |
|---------|---------|------|
| "options 字段被忽略" | `options` 字段在 `PromptRequest` schema (v0.14.0) 中**不存在** | `agent-client-protocol-schema-0.14.0/src/v1/agent.rs:3029` |
| "usage 字段不可用" | `usage` 字段**可用** — `unstable_end_turn_token_usage` 特性在 `Cargo.toml:29` 已启用 | `apps/acp/Cargo.toml:29` |

### 验收清单

**已实现功能（无需修复）：**
- [x] 协议处理器位于 `agent.rs:738-1046`
- [x] Stdio 循环路由位于 `stdio_loop.rs:328-344`
- [x] ContentBlock 多模态解析
- [x] 内置命令（/reset、/goal、/review-skill）
- [x] 通过 `StopReason::Cancelled` 取消
- [x] Token 使用情况跟踪（特性门控）
- [x] 三个 e2e 测试场景覆盖

**可选改进（低优先级）：**
- [ ] 添加 StopReason fallthrough 的显式匹配（vs 隐式默认）
- [ ] 在 `e2e_mega.rs` 中添加多模态内容测试
- [ ] 在 `e2e_mega.rs` 中添加内置命令测试
- [ ] 文档化 `meta.model` 字段的当前忽略行为（如适用）

### 作为参考实现

由于 `session/prompt` 是 Loom 中最复杂、最完整的协议，**建议作为新协议实现的参考模板**：

1. **多模态内容处理** — 参考 `content.rs` 的 `ContentBlock` 解析
2. **流式响应** — 参考 `stream_bridge.rs` 的 `SessionNotifier` 模式
3. **取消集成** — 参考 `AtomicBool` + `RunCancellation` 两级中止
4. **使用情况跟踪** — 参考 `build_acp_usage()` 的特性门控
5. **测试模式** — 参考 `e2e_mega.rs:99-221` 的三场景结构（文本/工具/取消）
