# Run Command 执行结果不显示问题分析与优化方案

## 1. 问题描述

ACP 客户端（如 Zed、JetBrains）收到 `bash`/`powershell` 工具的 `ToolCallUpdate` 时，**执行结果不在 UI 中渲染**。

用户只看到：
- ✅ `ToolCall`（title: `"cargo build"`，kind: `execute`，status: `pending`）
- ✅ `ToolCallUpdate`（status: `completed`）
- ❌ 但看不到命令的实际输出（如编译结果、测试输出等）

---

## 2. 根因分析

### 2.1 完整数据流追踪

```
BashTool.call() 
  → ToolCallContent::Text("stdout:\n...\nstderr:\n...")
  → normalize_tool_output()
  → display_text (可能被截断/摘要化)
  → StreamEvent::ToolEnd { result: display_text, is_error: false }
  → StreamUpdate::ToolCallUpdated { status: "success", output: Some(display_text) }
  → stream_update_to_session_notification()
  → ToolCallUpdateFields::new()
      .status(ToolCallStatus::Completed)
      .content(vec![display_text.into()])       // ← 这里
      .raw_output(json!(display_text))
  → SessionUpdate::ToolCallUpdate(...)
  → JSON-RPC 发送给客户端
```

### 2.2 content 字段的实际 JSON

发送给 ACP 客户端的 JSON：

```json
{
  "sessionId": "session-123",
  "sessionUpdate": {
    "toolCallUpdate": {
      "toolCallId": "call-001",
      "status": "completed",
      "content": [
        {
          "type": "content",
          "content": {
            "type": "text",
            "text": "stdout:\n   Compiling loom v0.1.0\n    Finished `dev` profile\n"
          }
        }
      ],
      "rawOutput": "stdout:\n   Compiling loom v0.1.0\n    Finished `dev` profile\n"
    }
  }
}
```

**这个结构是正确的**。`content` 包含了 `ToolCallContent::Content(Content { ContentBlock::Text(...) })`。

### 2.3 可能的问题点

#### 问题 1：normalize_tool_output 可能丢弃了内容

`loom/src/agent/react/act_node.rs:569` 中：

```rust
let normalized = normalize_tool_output(
    &tc.name,
    &args,
    content.as_text().unwrap(),
    false,
    tool_output_hints.get(&tc.name),
    NormalizationConfig::runtime_default()
        .with_used_observation_chars(used_observation_chars),
);
let display_text = normalized.display_text.clone();
```

当输出过长时，`normalize_tool_output` 可能：
- `Inline` → 直接传递
- `HeadTail` → 只保留头尾
- `SummaryOnly` → 只发送摘要，如 `"[Output saved to ~/.loom/output/bash_xxx.txt (12345 chars)]"`
- `FileRef` → 只发文件引用

**如果命令输出超过预算，客户端收到的 content 可能是一个文件引用而非实际输出。**

#### 问题 2：ToolOutput 事件被忽略

`StreamEvent::ToolOutput`（运行中的中间输出）发出的更新：

```rust
// stream_bridge.rs:268-272
let mut fields = ToolCallUpdateFields::new().status(status);
if let Some(ref s) = output {
    fields = fields
        .content(vec![s.clone().into()])
        .raw_output(parse_text_output_to_raw_value(s));
}
```

每次 `ToolOutput` 都会 **覆盖** `content`，不是追加。最终 `ToolEnd` 的 content 会覆盖所有中间输出。这是正确行为。

#### 问题 3：display_text 被截断

`act_node.rs:503`:

```rust
content: truncate_for_display(&content, display_limit),
```

流式输出中的中间 content 已经被 `truncate_for_display` 截断。但最终的 `ToolEnd.result` 使用的是 `display_text`（normalize 后的），也可能被截断。

#### 问题 4（最可能）：ACP 客户端不渲染 execute 类型的 content

部分 ACP 客户端（如 Zed）对于 `kind: execute` 的工具，**可能只渲染 Terminal 类型的 content，不渲染 Text 类型的 content**。这是客户端的设计选择：execute 类型工具应该在 IDE 终端中展示，而不是在聊天 UI 中。

---

## 3. 优化方案

### 方案 A：使用 ACP Terminal 协议（推荐）

ACP 协议定义了 `Terminal` 内容类型，专门用于命令执行。IDE 客户端会将其渲染为嵌入式终端。

**原理**：

```
当前：ToolCallUpdate.content = [Content(Text("output..."))]
                    ↓ 客户端可能不渲染 execute 类型的 Text content
                    
优化：ToolCallUpdate.content = [Terminal(terminal_id)]
                    ↓ 客户端渲染为嵌入式终端面板
```

**实现步骤**：

1. 在 `stream_bridge.rs` 中，当 `ToolKind::Execute` 工具的 `ToolEnd` 到来时：
   - 调用 ACP 客户端的 `terminal/create` 创建终端
   - 通过 `terminal/send_text` 发送命令输出
   - 在 `ToolCallUpdate.content` 中使用 `ToolCallContent::Terminal` 替代 `Content(Text)`

2. 具体改动在 `stream_update_to_session_notification`：

```rust
StreamUpdate::ToolCallUpdated { tool_call_id, status, output } => {
    let status = /* ... */;
    let mut fields = ToolCallUpdateFields::new().status(status);
    if let Some(ref s) = output {
        // 如果是 execute 类型的最终结果，使用 Terminal
        if status == ToolCallStatus::Completed {
            // 需要在这里获取 kind 信息来判断
            // 方案：在 StreamUpdate::ToolCallUpdated 中增加 kind 字段
            // 或者在此处根据历史 tool_calls_map 查找 kind
        }
        fields = fields
            .content(vec![s.clone().into()])
            .raw_output(parse_text_output_to_raw_value(s));
    }
    // ...
}
```

**问题**：当前 `StreamUpdate::ToolCallUpdated` 没有 `kind` 信息，无法判断是否为 execute 类型。需要扩展。

### 方案 B：在 rawOutput 中保证完整输出

最简单的改进：确保 `rawOutput` 始终包含完整的、未截断的命令输出，让客户端可以选择渲染。

```rust
// 当前
fn parse_text_output_to_raw_value(output: &str) -> serde_json::Value {
    serde_json::json!(output)   // output 可能已经被 normalize 截断
}

// 改进：在 StreamUpdate::ToolCallUpdated 中增加 raw_full_output 字段
ToolCallUpdated {
    tool_call_id: String,
    status: String,
    output: Option<String>,           // display_text (可能截断)
    raw_full_output: Option<String>,  // 完整未截断输出
}
```

### 方案 C：在 content 中使用结构化输出

将命令输出包装为结构化的 content，包含 exit code、stdout、stderr 分离：

```json
{
  "content": [
    {
      "type": "content",
      "content": {
        "type": "text", 
        "text": "$ cargo build\n   Compiling loom v0.1.0\n    Finished `dev` profile\n"
      },
      "_meta": {
        "exitCode": 0,
        "stdout": "   Compiling loom v0.1.0\n    Finished `dev` profile\n",
        "stderr": "",
        "truncated": false
      }
    }
  ]
}
```

### 方案 D：在 ToolCall 创建时就使用 Terminal（最优体验）

最佳用户体验方案：在 `ToolCall` 创建时就通过 `terminal/create` 在 IDE 中创建终端，命令在终端中实时执行：

1. `StreamEvent::ToolCall { name: "bash" }` 到来
2. `stream_bridge` 调用客户端 `terminal/create`，获取 `terminal_id`
3. `StreamUpdate::ToolCallStarted` 时，在 content 中放 `Terminal(terminal_id)`
4. `StreamEvent::ToolOutput` 时，通过 `terminal/send_text` 实时推送输出到终端
5. `StreamEvent::ToolEnd` 时，调用 `terminal/release`

```
时间线：
t0: ToolCall { name: "bash", args: { command: "cargo build" } }
    → terminal/create → terminal_id = "term-xxx"
    → ToolCall { content: [Terminal("term-xxx")], status: pending }

t1: ToolStart { name: "bash" }
    → ToolCallUpdate { status: in_progress }

t2: ToolOutput { content: "   Compiling loom v0.1.0\n" }
    → terminal/send_text("term-xxx", "   Compiling loom v0.1.0\n")
    → 客户端在终端面板中实时显示输出

t3: ToolEnd { result: "    Finished `dev` profile\n", is_error: false }
    → terminal/send_text("term-xxx", "    Finished `dev` profile\n")
    → ToolCallUpdate { status: completed }
    → terminal/release("term-xxx")
```

**前提**：ACP 客户端支持 `terminal/create`、`terminal/send_text`、`terminal/release` 方法。

---

## 4. 方案对比

| | 方案 A (Terminal content) | 方案 B (rawOutput 保全) | 方案 C (结构化) | 方案 D (Terminal 全流程) |
|---|---|---|---|---|
| 改动范围 | stream_bridge | stream_bridge + act_node | stream_bridge | stream_bridge + client_methods |
| 客户端支持 | 需支持 Terminal content | 通用 | 需解析 _meta | 需支持 terminal/* |
| 体验 | ⭐⭐⭐ 终端渲染 | ⭐⭐ 文本渲染 | ⭐⭐ 文本渲染 | ⭐⭐⭐⭐ 实时终端 |
| 实时输出 | ❌ 最终一次性 | ❌ 最终一次性 | ❌ 最终一次性 | ✅ 实时流 |
| 复杂度 | 中 | 低 | 中 | 高 |

---

## 5. 推荐实施路径

### Phase 1：快速修复（方案 B）

确保 `rawOutput` 包含完整的、未被 normalize 截断的命令输出：

1. 在 `StreamUpdate::ToolCallUpdated` 中增加 `raw_full_output: Option<String>` 字段
2. 在 `act_node.rs` 发送 `ToolEnd` 时同时传递未 normalize 的原始输出
3. `stream_bridge` 优先使用 `raw_full_output` 作为 `rawOutput`

### Phase 2：Terminal 集成（方案 D）

1. 检查客户端 `ClientCapabilities.terminal` 是否支持
2. 在 `SessionNotifier` 中注入 client 引用
3. execute 类型工具使用 `terminal/create` + `send_text` + `release` 全流程
4. fallback：不支持 terminal 的客户端使用 Phase 1 的文本输出

---

## 6. 相关文件

| 文件 | 相关度 | 说明 |
|------|--------|------|
| `loom-acp/src/stream_bridge.rs:254` | 🔴 核心改动点 | ToolCallUpdated content 构造 |
| `loom-acp/src/stream_bridge.rs:380` | 🔴 历史回放 | Message::Tool content 构造 |
| `loom-acp/src/terminal.rs` | 🟡 已有基础设施 | TerminalManager（本地模拟） |
| `loom-acp/src/tools/terminal_tools.rs` | 🟡 已有工具 | create_terminal / output / release |
| `loom-acp/src/client_methods.rs` | 🟡 客户端调用 | ACP client method 实现 |
| `loom-acp/src/client_capabilities.rs:102` | 🟢 能力检测 | `supports_terminal()` |
| `loom/src/agent/react/act_node.rs:569` | 🟢 上游 | normalize_tool_output |
| `loom/src/tools/bash/mod.rs:201` | 🟢 源头 | BashTool 返回 ToolCallContent::Text |
| `stream-event/src/event.rs:165` | 🟢 协议 | StreamEvent::ToolOutput { content: String } |
