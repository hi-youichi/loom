# 方案：Tool 消息模型落地后的修补与加固

> 前置文档：[openai-tool-conversation-format.md](./openai-tool-conversation-format.md)（§9 实施总结引用本文档）  
> 状态：**Phase 1–2 已实施**（代码）；Phase 3（ACP 协议）仍待定  
> 范围：`Message::Tool` / `AssistantPayload` 首版落地后发现的遗留问题与改进项

## 1. 背景

`Message` 枚举已完成首版扩展（`Assistant(AssistantPayload)` + `Tool { tool_call_id, content }`），
37 个文件的消费端均已迁移。代码审查发现以下需要跟进的风险点和优化项。

## 2. 问题清单

| # | 严重度 | 模块 | 摘要 |
|---|--------|------|------|
| P1 | **高** | `observe_node` | `tool_call_id` 按数组索引回退匹配，可能错配 |
| P2 | **高** | `sqlite_store` | tool 反序列化失败时 `tool_call_id` 为空字符串，下游 API 400 |
| P3 | 中 | `act_node` / 全链路 | `ToolResult.call_id` 未被 ActNode 始终填充，根源问题 |
| P4 | 中 | `serve` + `sqlite_store` | `message_to_item` / `message_to_role_content` 序列化逻辑重复 |
| P5 | 低 | `loom-acp` | `Message::Tool` 映射为 `UserMessageChunk`，语义不准确 |
| P6 | 低 | `message.rs` | `assistant_content_for_chat_api` 行为变更（`""` → `\u{2060}`）未文档化 |
| P7 | 低 | `message.rs` 测试 | `Display` 测试缺少 `Tool` 变体覆盖（200 字符截断逻辑未验证） |

## 3. 方案设计

### 3.1 P1 + P3：保证 tool_call_id 全链路一致

**根因**：`tool_call_id` 的可靠性取决于三个环节的传递链——

```
ThinkNode (LLM 返回 tool_calls[].id)
  → state.tool_calls[].id      ← normalize_tool_call_ids 已保证非空
    → ActNode 执行工具
      → ToolResult.call_id     ← ⚠️ 当前并非总是从 ToolCall.id 回填
        → ObserveNode 写入 Message::Tool { tool_call_id }
```

断链位置在 **ActNode**：当 `ToolSource::call_tool` 返回 `ToolResult` 时，
`call_id` 取决于工具源实现（MCP 有 id，内建工具通常无）。

**方案**：

1. **ActNode 在执行工具后强制回填 `call_id`**：

```rust
// act_node.rs — 执行完每个 tool_call 后
for (tc, mut result) in tool_calls.iter().zip(results.iter_mut()) {
    if result.call_id.is_none() {
        result.call_id = tc.id.clone();
    }
}
```

2. **ObserveNode 移除索引回退**，简化为：

```rust
let tool_call_id = tr
    .call_id
    .clone()
    .unwrap_or_else(|| format!("call_{}", uuid6()));
```

此时 `unwrap_or_else` 仅用于极端防御（正常路径不应进入）。
可添加 `tracing::warn!` 以便排查。

3. **ThinkNode `normalize_tool_call_ids` 保持不变**（已在首版实现）。

**验证**：扩展 `react_nodes.rs` 中 observe 测试，断言 `Message::Tool.tool_call_id`
与 `ToolCall.id` 严格一致而非 fallback 值。

### 3.2 P2：sqlite_store tool 反序列化降级

**现状**：`row_to_message("tool", content)` 解析 JSON 失败时：

```rust
Message::Tool {
    tool_call_id: String::new(),  // ← 空字符串
    content: content.to_string(),
}
```

**方案**：

1. 解析失败时生成 fallback id 并记录警告：

```rust
"tool" => {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        // ... 正常路径 ...
    }
    tracing::warn!(raw = content, "malformed tool message in DB, generating fallback id");
    Message::Tool {
        tool_call_id: format!("call_{}", uuid6()),
        content: content.to_string(),
    }
}
```

2. 考虑在 `SqliteUserMessageStore::append` 写入时做校验——
   若 `tool_call_id` 为空则在写入前补全，从源头避免脏数据。

### 3.3 P4：序列化逻辑去重

`serve/src/user_messages.rs::message_to_item` 与
`loom/src/user_message/sqlite_store.rs::message_to_role_content` 逻辑近似。

**方案**：在 `Message` 上增加公共方法：

```rust
impl Message {
    /// 返回 (role, content_string)，适用于纯文本持久化场景。
    /// Assistant 带 tool_calls 时 content 为 JSON；Tool 同理。
    pub fn to_role_content_pair(&self) -> (&'static str, String) { ... }
}
```

两处调用方各自替换为该方法。测试集中在 `message.rs` 中。

### 3.4 P5：ACP Tool 消息映射

**现状**：

```rust
Message::Tool { content, .. } => SessionNotification::new(
    session_id.clone(),
    SessionUpdate::UserMessageChunk(ContentChunk::new(content.clone().into())),
)
```

**方案**：

- 短期：添加 `// TODO: map to dedicated ToolResultChunk when ACP protocol supports it` 注释，
  保持现有行为（ACP 协议当前无 Tool 角色概念）。
- 中期：ACP 协议扩展 `SessionUpdate::ToolResultChunk { tool_call_id, content }` 后，
  此处对应更新。

### 3.5 P6：`assistant_content_for_chat_api` 行为变更

旧行为：空 content → 返回 `""`（仍然空，实际未起到占位作用）。  
新行为：空 content → 返回 `"\u{2060}"`（WORD JOINER，对 API 校验非空）。

**方案**：无需代码改动，但需要：

1. 在提交消息中明确说明此语义变更。
2. 确认 BigModel 的 Kimi 系列对 WORD JOINER 的处理——Kimi 路径有独立的
   `use_space_for_empty_assistant` 分支（用空格），不走此函数，风险低。

### 3.6 P7：补充 Display 测试

```rust
#[test]
fn message_display_tool_truncates_at_200() {
    let long = "x".repeat(300);
    let msg = Message::Tool {
        tool_call_id: "c1".into(),
        content: long.clone(),
    };
    let display = msg.to_string();
    assert!(display.starts_with("tool[c1]: "));
    assert_eq!(display.len(), "tool[c1]: ".len() + 200);
}

#[test]
fn message_display_tool_short() {
    let msg = Message::Tool {
        tool_call_id: "c1".into(),
        content: "ok".into(),
    };
    assert_eq!(msg.to_string(), "tool[c1]: ok");
}
```

## 4. 实施顺序

```
Phase 1 (安全关键)
  ├─ P1 + P3: ActNode call_id 回填 + ObserveNode 去掉索引回退
  └─ P2: sqlite_store fallback id
Phase 2 (代码质量)
  ├─ P4: 序列化去重
  ├─ P7: 补测试
  └─ P6: commit message 说明
Phase 3 (协议演进)
  └─ P5: ACP Tool 消息（依赖协议扩展）
```

## 5. 测试策略

| 场景 | 类型 | 验证点 |
|------|------|--------|
| ActNode 回填 call_id | 单元 | `ToolResult.call_id == ToolCall.id` |
| ObserveNode 无索引回退 | 单元 | `Message::Tool.tool_call_id` 与输入 `ToolResult.call_id` 一致 |
| sqlite 写入/读取 tool 消息往返 | 集成 | 含 tool_calls 的 assistant + tool 消息完整 round-trip |
| sqlite 脏数据降级 | 单元 | 解析失败时 `tool_call_id` 非空 |
| Display 截断 | 单元 | 超 200 字符的 tool content 被截断 |
| OpenAI 多轮工具 | 集成 (ignored) | 两轮 tool 调用不 400 |

## 6. 风险

- **ActNode call_id 回填依赖执行顺序**：当前 ActNode 按 `tool_calls` 顺序执行并
  zip 结果，若引入并发执行需保证对应关系不变。
- **已有 SQLite 数据**：旧数据中 `role = "tool"` 行不存在（之前是 `role = "user"`），
  无迁移需求；但若中间版本已写入空 `tool_call_id` 的行，需补偿脚本或容忍 fallback。

## 7. 实施记录（已完成）

| 项 | 实现要点 |
|----|----------|
| P1 + P3 | `act_node.rs`：`backfill_tool_result_call_ids`，在 `run` / `run_with_context` 末尾调用；单元测试 2 个。 |
| P1 | `observe_node.rs`：去掉 `tool_results` 与 `tool_calls` 的索引对齐；仅 `ToolResult.call_id` 或 `warn` + `uuid6`。 |
| P2 | `sqlite_store.rs`：`row_to_message("tool")` 解析失败或缺 id 时生成 `call_{uuid6}` 并 `tracing::warn`。 |
| P2 写入 | `Message::to_role_content_pair_for_store`：持久化前空 `tool_call_id` 补 id；`append` 使用该路径。 |
| P4 | `message.rs`：`to_role_content_pair` / `to_role_content_pair_for_store`；`serve/user_messages.rs` 用前者；SQLite 用后者。 |
| P5 | `loom-acp/src/agent.rs`：`Message::Tool` 分支上增加 TODO 注释。 |
| P6 | `assistant_content_for_chat_api` 文档说明由 `""` 改为 WORD JOINER 的语义。 |
| P7 | `message.rs`：`message_display_tool_short`、 `message_display_tool_truncates_at_200`。 |
