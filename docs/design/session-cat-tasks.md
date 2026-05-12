---
sidebar_position: 6
title: "session cat 开发任务"
description: "session cat 命令的剩余开发任务、Turn 粒度拆分方案和详细实施步骤"
---

# session cat 开发任务

## 概述

`session cat` 的基础功能已实现（checkpoint 重建 → Codex NDJSON），但 Turn 粒度与 Codex 协议不一致。本文档定义剩余开发任务。

## 背景：Turn 粒度差异

**Codex 协议**：1 次 LLM 调用 = 1 个 turn。

```
Turn 1: LLM → reasoning + tool_call → turn.completed (usage)
Turn 2: [tool result 输入] LLM → reasoning + tool_call → turn.completed (usage)  
Turn 3: [tool result 输入] LLM → final reply → turn.completed (usage)
```

**Loom 当前**：1 个 checkpoint = 1 个 turn，一个 checkpoint 可能包含多个 LLM 调用。

```
Turn 1: [checkpoint] 包含 3 次 LLM 调用、15 个 tool_call、8 个 agent_message
```

**目标**：按 Assistant 消息拆分 turn，与 Codex 语义对齐。

---

## 任务 1：按 Assistant 消息拆分 Turn（核心）

### 当前行为

输入消息序列（1 个 checkpoint）：

```
System → User → Assistant₁(reasoning + 2 tool_calls) → Tool₁ → Tool₂ → Assistant₂(reasoning + 1 tool_call) → Tool₃ → Assistant₃(reply text)
```

当前输出：

```
turn.started
  item_started/completed: reasoning₁
  item_started/completed: mcp_tool_call₁
  item_started/completed: mcp_tool_call₂
  item_started/completed: mcp_tool_call₃
  item_started/completed: agent_message₂  ← 错误：应该是独立 turn
  item_started/completed: agent_message₃
turn.completed (usage)
```

### 目标行为

```
turn.started
  item_started/completed: reasoning₁
  item_started/completed: mcp_tool_call₁
  item_started/completed: mcp_tool_call₂
turn.completed (usage₁)

turn.started
  item_started/completed: mcp_tool_call₃
turn.completed (usage₂)

turn.started
  item_started/completed: reasoning₃
  item_started/completed: agent_message₃
turn.completed (usage₃)
```

### 拆分规则

1. 遍历 `new_messages`
2. 每个 `Message::Assistant` 开启一个新 turn（第一个除外，复用已有的 `turn_started`）
3. `Message::Tool` 不作为独立 item，其内容用于填充前一个 `Assistant` 消息中对应 `tool_call` 的 `result` 字段
4. 遇到下一个 `Message::Assistant` 时：
   - 先 emit `turn.completed`（当前 turn 的 usage）
   - 再 emit `turn.started`（新 turn）
5. `Message::User` 不触发 turn 拆分（ReAct 循环中 User 消息通常不出现在中间）

> **注意**：`Message::Tool` 不产生独立事件，它是 `mcp_tool_call` / `command_execution` item 的 `result` 来源。在 `item_started` 阶段 `result` 为 null，找到对应 Tool 消息后填充到 `item_completed` 的 `result` 字段。

### Usage 差值计算

Checkpoint 的 `total_usage` 是累计值（所有 LLM 调用之和），需要差值推算每个 turn 的 usage。`usage` 是上次 LLM 调用的用量（per-turn），可直接使用。

**方案**：

```rust
// 伪代码
let mut prev_total_usage = CodexUsage::zero();

for (turn_idx, turn_messages) in split_turns(&new_messages).iter().enumerate() {
    events.push(CodexEvent::TurnStarted);
    
    // emit items...
    
    // 优先使用 per-turn usage（state.usage），否则用 total_usage 差值推算
    if let Some(ref turn_usage) = state.usage {
        events.push(CodexEvent::TurnCompleted { usage: to_codex_usage(turn_usage) });
    } else if let Some(ref total_usage) = state.total_usage {
        let current_total = to_codex_usage(total_usage);
        let turn_usage = current_total.clone() - prev_total_usage;
        events.push(CodexEvent::TurnCompleted { usage: turn_usage });
        prev_total_usage = current_total;
    } else {
        events.push(CodexEvent::TurnCompleted { usage: CodexUsage::zero() });
    }
}
```

**注意**：如果 checkpoint 没有按 turn 粒度记录 usage（只有最终累计值），则只能为最后一个 turn 提供精确 usage，前面的 turn 只能均分或置零。需要检查 `ReActState` 中是否有 per-turn usage。

**降级策略**：如果无法获取 per-turn usage，将总 usage 全部归到最后一个 turn，前面的 turn usage 置零。

### 修改文件

| 文件 | 变更 |
|------|------|
| `cli/src/codex_event_builder.rs` | `build_codex_events` 函数重写 turn 拆分逻辑 |
| `cli/src/codex_event_builder.rs` | 新增 `split_turns()` 辅助函数 |
| `cli/src/codex_event_builder.rs` | 新增 `CodexUsage` 的 `Sub` 运算 |

### 实现步骤

1. 在 `codex_event_builder.rs` 中新增 `split_turns(messages: &[Message]) -> Vec<Vec<&Message>>`
   - 按 `Message::Assistant` 边界切割
   - 每个 chunk 以 `Assistant` 开头，后续紧跟的 `Tool` 消息归入同一 chunk
   - `User` / `System` 消息跳过或归入前一个 chunk

2. 为 `CodexUsage` 实现 `std::ops::Sub`

3. 重写 `build_codex_events` 中的 checkpoint 遍历逻辑：
   - 外层循环：遍历 checkpoints
   - 计算每个 checkpoint 的 delta messages
   - 内层循环：`split_turns(delta_messages)` 遍历每个 turn
   - 每个 turn emit `turn.started` → items → `turn.completed`

4. 验证：用现有 session 数据对比输出

### 验证命令

```bash
# Turn 数量应该等于 Assistant 消息数量
cargo run -p cli -- --json session cat session-13f229ef-1086-401b-960f-441ef4634087 2>/dev/null | \
  python3 -c "
import json, sys
events = [json.loads(l) for l in sys.stdin]
turns_started = sum(1 for e in events if e['type'] == 'turn_started')
turns_completed = sum(1 for e in events if e['type'] == 'turn_completed')
items = sum(1 for e in events if e['type'] == 'item_completed')
print(f'turns: {turns_started} started / {turns_completed} completed')
print(f'items: {items}')
"
```

预期：turn 数量显著增加（从 4 → 约 50+），每个 turn 含 1-3 个 item。

### 测试方案

在 `cli/src/codex_event_builder.rs` 底部新增 `#[cfg(test)] mod tests`，纯函数单元测试，零外部依赖。

#### Fixture 辅助函数

```rust
fn usage(prompt: u32, completion: u32) -> LlmUsage {
    LlmUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        ..Default::default()
    }
}

fn assistant_with_tools(reasoning: Option<&str>, content: &str, tools: Vec<(&str, &str)>) -> Message
fn assistant_reply(content: &str) -> Message
fn tool_result(call_id: &str, text: &str) -> Message
fn checkpoint(messages: Vec<Message>, usage: Option<LlmUsage>, total_usage: Option<LlmUsage>) -> ReActState

fn count_event_type(events: &[CodexEvent], matcher: fn(&CodexEvent) -> bool) -> usize
fn is_turn_started(e: &CodexEvent) -> bool
fn is_turn_completed(e: &CodexEvent) -> bool
```

#### 测试用例

| # | 测试名 | 场景 | 核心断言 |
|---|--------|------|----------|
| 1 | `single_assistant_no_tools` | 1 checkpoint: System + User + Assistant(reply) | 1 turn_started + 1 turn_completed，1 agent_message item |
| 2 | `single_assistant_with_tools` | 1 checkpoint: Assistant(tool_call) → Tool → Assistant(reply) | **2 turns**（拆分点：第 2 个 Assistant），turn[0] 含 1 mcp_tool_call，turn[1] 含 1 agent_message |
| 3 | `multi_assistant_in_one_checkpoint` | 1 checkpoint: Assistant₁(reasoning+2 tools) → Tool₁ → Tool₂ → Assistant₂(tool) → Tool₃ → Assistant₃(reply) | **3 turns**，与「目标行为」完全一致 |
| 4 | `multi_checkpoint_delta` | 2 checkpoints: cp1=[User+Assistant₁+Tool₁]，cp2=[+Assistant₂+Tool₂+Assistant₃] | cp1 产生 1 turn，cp2 delta 产生 **2 turns** |
| 5 | `checkpoint_no_assistant_skipped` | 2 checkpoints: cp1=[User+Assistant]，cp2=[+Tool only] | cp2 delta 无 Assistant → 跳过，总共 1 turn |
| 6 | `empty_checkpoints` | 0 checkpoints | 仅 ThreadStarted，无 turn |
| 7 | `usage_per_turn_with_state_usage` | 每个 checkpoint 有 `state.usage` | 每个 turn_completed 的 usage 等于该 checkpoint 的 `usage` |
| 8 | `usage_delta_from_total_usage` | 仅 `total_usage` 无 `usage` | 最后一个 turn 获得总 usage，前面的 turn usage 为零（降级策略） |
| 9 | `usage_zero_when_no_usage` | 无任何 usage | 所有 turn_completed 的 usage 均为零 |
| 10 | `item_id_sequential` | 多个 turn | item_id 全局递增（item_0, item_1, item_2…），跨 turn 不重置 |
| 11 | `tool_result_filled_in_items` | Assistant(bash call) → Tool(ok) / Assistant(mcp call) → Tool(error) | command_execution item 的 exit_code/status 正确，mcp_tool_call item 的 error/status 正确 |
| 12 | `user_message_not_splitting` | 中间穿插 User 消息 | User 消息不触发 turn 拆分，仅 Assistant 边界拆分 |

#### 关键断言模式

```rust
// turn 数量 = Assistant 消息数量
assert_eq!(count(&events, is_turn_started), expected_assistant_count);
// turn_started / turn_completed 配对
assert_eq!(count(&events, is_turn_started), count(&events, is_turn_completed));
// 每个 turn 内 item 类型顺序
// e.g. turn 1: [reasoning, mcp_tool_call, mcp_tool_call]
//      turn 2: [mcp_tool_call]
//      turn 3: [agent_message]
```

#### 测试 3 与设计文档「目标行为」的映射

输入：

```
Assistant₁(reasoning + 2 tools) → Tool₁ → Tool₂ → Assistant₂(tool) → Tool₃ → Assistant₃(reply)
```

预期输出：

```
turn 1: reasoning₁ + mcp_tool_call₁ + mcp_tool_call₂
turn 2: mcp_tool_call₃
turn 3: agent_message₃
```

#### 不需要测试的范围

- `print_cat_text`（纯输出格式，视觉验证）
- `split_server_tool`（纯字符串分割，trivial）
- 流式/并发场景（`build_codex_events` 是纯函数，无副作用）

---

## 任务 2：降级策略 — 未完成的 Turn emit turn.failed

### 检测场景

从 checkpoint 判断 turn 未完成有两种场景：

**场景 A：最后一个 checkpoint 无 usage**

```
... → Assistant₁(tool_calls) → Tool₁ → Tool₂
```

LLM 还没返回（或返回了但没 checkpoint），最后一段消息以 Tool 结尾。Checkpoint 无 usage。

**场景 B：最后一个 checkpoint 有 Assistant 但无有效内容**

```
... → Assistant₂(reasoning only, no content, no tool_calls)
```

LLM 返回了空响应或被中断。有 Assistant 消息但无实际产出。

**不在范围内**：

- LLM 报错后 Loom 自动重试成功 → 正常 checkpoint，不需要 `turn.failed`
- 首次请求就认证失败 → 无 checkpoint，`session cat` 找不到 session，直接报错

### 数据来源分析

`ReActState` 中可用于判断的字段：

| 字段 | 类型 | 含义 |
|------|------|------|
| `messages` | `Vec<Message>` | 消息历史，最后一条可能是 Tool / Assistant |
| `usage` | `Option<LlmUsage>` | 上次 LLM 调用的 token 用量，None 表示未收到 LLM 响应 |
| `total_usage` | `Option<LlmUsage>` | 累计 token 用量（所有 LLM 调用之和） |
| `should_continue` | `bool` | ReAct 循环是否应继续 |
| `turn_count` | `u32` | 累计 turn 数 |

关键判断逻辑：

```rust
fn is_last_turn_incomplete(state: &ReActState) -> bool {
    // 如果有 usage，说明 LLM 正常返回了
    if state.usage.is_some() {
        return false;
    }

    // 检查最后一条非系统消息
    let last_non_system = state.messages.iter().rev().find(|m| !matches!(m, Message::System(_)));

    match last_non_system {
        // 以 Tool 结尾：LLM 还没返回
        Some(Message::Tool { .. }) => true,
        // 以 Assistant 结尾但没有 content 和 tool_calls：空响应
        Some(Message::Assistant(p)) if p.content.is_empty() && p.tool_calls.is_empty() => true,
        // 以 Assistant 结尾有内容：正常
        _ => false,
    }
}
```

### 代码方案

在 `build_codex_events` 末尾，处理完最后一个 checkpoint 的最后一个 turn 后，检测并替换：

```rust
pub fn build_codex_events(
    session_id: &str,
    checkpoints: &[CheckpointEntry],  // 任务 4 引入 CheckpointEntry
) -> Vec<CodexEvent> {
    let mut events = Vec::new();
    // ... thread_started, turn 拆分逻辑 ...

    for entry in checkpoints.iter() {
        let state = &entry.state;
        // ... turn 拆分、item emit ...
    }

    // ===== 降级检测：最后一个 turn 是否未完成 =====
    let last_state = checkpoints.last().unwrap();
    if is_last_turn_incomplete(last_state) {
        // 从 events 末尾找到最后一个 turn.completed 并替换为 turn.failed
        replace_last_turn_completed_with_failed(&mut events);
    }

    events
}

fn replace_last_turn_completed_with_failed(events: &mut Vec<CodexEvent>) {
    // 从后往前找最后一个 turn.completed
    for i in (0..events.len()).rev() {
        if matches!(&events[i], CodexEvent::TurnCompleted { .. }) {
            events[i] = CodexEvent::TurnFailed {
                error: CodexErrorInfo {
                    message: "turn did not complete (checkpoint may be incomplete)".to_string(),
                },
            };
            return;
        }
    }
}

fn is_last_turn_incomplete(state: &ReActState) -> bool {
    if state.usage.is_some() {
        return false;
    }

    let last_non_system = state
        .messages
        .iter()
        .rev()
        .find(|m| !matches!(m, Message::System(_)));

    match last_non_system {
        Some(Message::Tool { .. }) => true,
        Some(Message::Assistant(p)) if p.content.is_empty() && p.tool_calls.is_empty() => true,
        _ => false,
    }
}
```

### 边界情况

| 场景 | `usage` | 最后消息 | 结果 |
|------|---------|---------|------|
| 正常完成 | `Some` | Assistant(reply) | `turn.completed` ✓ |
| LLM 空响应 | `None` | Assistant(reasoning only) | `turn.failed` |
| LLM 未返回 | `None` | Tool | `turn.failed` |
| User 中断 | `None` | Assistant(partial) | `turn.completed`（有内容就算完成） |
| 唯一 checkpoint 是初始化 | `None` | User | 不触发（无 Assistant 消息，不会产生 turn） |

### 修改文件

| 文件 | 变更 |
|------|------|
| `cli/src/codex_event_builder.rs` | 新增 `is_last_turn_incomplete()` 函数 |
| `cli/src/codex_event_builder.rs` | 新增 `replace_last_turn_completed_with_failed()` 函数 |
| `cli/src/codex_event_builder.rs` | `build_codex_events` 末尾调用降级检测 |

---

## 任务 3：工具结果包装文本清洗（源头修复）

### 问题

Codex 的工具结果是纯内容，无包装前缀。Loom 的 `Message::Tool` 内容包含 `Tool {name} result:\n` 前缀。

**Codex 期望**：
```json
{"command":"cargo build","aggregated_output":"   Compiling cli...\n    Finished dev","exit_code":0,"status":"completed"}
```

**Loom 当前**：
```json
{"command":"cargo build","aggregated_output":"Tool bash result:\n   Compiling cli...\n    Finished dev","exit_code":0,"status":"completed"}
```

### 源头分析

前缀在 `observe_node.rs:71` 生成，写入 `Message::Tool.content`：

```rust
// loom/src/agent/react/observe_node.rs:71
let mut body = format!("Tool {} {}:\n{}", name, label, observation);

// loom/src/agent/react/observe_node.rs:93-96
messages.push(Message::Tool {
    tool_call_id,
    content: ToolCallContent::text(body),  // ← 包含前缀
});
```

`Message::Tool` 被持久化到 checkpoint，是 `session cat` 读取的数据源。

### 源头修复方案

将前缀从 `Message::Tool.content` 中移除，改为仅在 stream/display 输出中保留前缀。

#### 修改 1：observe_node.rs — 存储纯内容

```rust
// loom/src/agent/react/observe_node.rs

// 修改前：
let mut body = format!("Tool {} {}:\n{}", name, label, observation);
if let Some(ref storage_ref) = tr.storage_ref {
    body.push_str(&format!("\n\nFull output saved to: {}", storage_ref.path.display()));
}
// ...
messages.push(Message::Tool {
    tool_call_id,
    content: ToolCallContent::text(body),
});

// 修改后：
let mut content = observation.to_string();
if let Some(ref storage_ref) = tr.storage_ref {
    content.push_str(&format!("\n\nFull output saved to: {}", storage_ref.path.display()));
}
messages.push(Message::Tool {
    tool_call_id,
    content: ToolCallContent::text(content),
});
```

前缀 `Tool {name} {label}:` 仅用于 TUI 显示，不再写入 checkpoint。

#### 修改 2：影响确认

Stream 事件的 `ToolEnd.result` 使用 `normalized.display_text`（来自 `tool_output_normalizer`），**不经过 observe_node**，天然无前缀，无需修改。

**影响分析**：

| 消费端 | 当前数据来源 | 影响 |
|--------|------------|------|
| LLM 下一轮输入 | `Message::Tool.content` | 去掉前缀后更干净，LLM 不需要前缀 |
| Checkpoint 存储 | `Message::Tool.content` | 新 checkpoint 无前缀，旧 checkpoint 有前缀 |
| Stream 事件 display | `ToolResult.display_text` | 不受影响 |
| `session cat` | 读 `Message::Tool.content` | 新 checkpoint 直接干净 |
| Compaction 压缩 | `compress/compaction.rs` 使用 `"Tool X returned: "` 模式 | **需同步更新** |

#### 修改 3：compaction 模式更新

`compress/compaction.rs` 中 `is_tool_result_message()` 有两条匹配路径：

```rust
// loom/src/compress/compaction.rs:21-30
fn is_tool_result_message(m: &Message) -> bool {
    match m {
        Message::Tool { .. } => true,           // 路径 1：基于 variant，不受前缀移除影响 ✓
        Message::User(c) => {
            let s = c.as_text();
            s.starts_with("Tool ") && s.contains(" returned: ")  // 路径 2：基于文本模式
        }
        _ => false,
    }
}
```

- **路径 1**（`Message::Tool`）：基于 variant 匹配，移除前缀无影响，无需修改
- **路径 2**（`Message::User`）：匹配旧格式 `Tool xxx returned: ...`。如果旧 checkpoint 中存在这种格式的 User 消息，移除前缀后不再匹配

`observe_node.rs` 当前写入的是 `Message::Tool`（L93），不是 `Message::User`，所以路径 2 只匹配旧数据。

**修改方案**：保留路径 2 不变（向后兼容旧 checkpoint），路径 1 无需修改。

### 修改文件

| 文件 | 变更 |
|------|------|
| `loom/src/agent/react/observe_node.rs` | `Message::Tool.content` 存储 `observation` 而非 `body`（去除前缀） |
| `loom/src/compress/compaction.rs` | 无需修改（见上方分析） |
| `loom/src/compress/prune_node.rs` | 无需修改（无文本模式匹配） |

---

## 任务 4：Item ID 稳定性

### 问题

当前按全局递增序号分配 ID（`item_0`、`item_1`、`item_2`...），存在两个问题：

1. **不稳定**：同一次 `session cat` 调用结果一致，但如果 checkpoint 被清理或新增（如 session resume），所有 ID 整体偏移
2. **不可追溯**：从 ID 无法定位到具体 checkpoint 或消息位置

### 数据源分析

Checkpoint 表的主键为 `(thread_id, checkpoint_ns, checkpoint_id)`，可用标识字段：

| 字段 | 示例值 | 可用性 |
|------|--------|--------|
| `checkpoint_id` | `1ef07a5a-...`（uuid6） | ✅ 每次节点 put 时生成，唯一，与 ReAct 节点一一对应 |
| `metadata_step` | 全部为 `0` | ❌ 不可用 |
| `metadata_created_at` | `1778473732043`（毫秒时间戳） | ✅ 唯一，但语义不如 checkpoint_id 明确 |
| `rowid` | 自增整数 | ✅ 但跨 session 不唯一 |

`checkpoint_id` 是每次 ReAct 节点（think_node / act_node / observe_node）执行 `put` 时由 `uuid6()` 生成的，天然与节点级别对应，是最佳选择。

### 方案：基于 checkpoint_id 前缀

取 uuid6 第一段（8 字符）作为前缀，拼接 checkpoint 内序号：

```rust
struct ItemIdCounter {
    checkpoint_id_short: String,
    seq: u16,
}

impl ItemIdCounter {
    fn new(checkpoint_id: &str) -> Self {
        Self {
            checkpoint_id_short: checkpoint_id.split('-').next().unwrap_or("?").to_string(),
            seq: 0,
        }
    }

    fn next(&mut self) -> String {
        let id = format!("item_{}_{}", self.checkpoint_id_short, self.seq);
        self.seq += 1;
        id
    }
}
```

生成的 ID 示例：

```
item_1ef07a5a_0    ← checkpoint 1 的第 1 个 item
item_1ef07a5a_1    ← checkpoint 1 的第 2 个 item
item_2b3f8c91_0    ← checkpoint 2 的第 1 个 item
item_2b3f8c91_1    ← checkpoint 2 的第 2 个 item
```

### 稳定性保证

| 场景 | 当前（序号） | 改进后（checkpoint_id 前缀） |
|------|------------|----------------------------|
| 同一 session 多次 cat | `item_0` ~ `item_138` ✓ | `item_1ef07a5a_0` ~ ✓ |
| 新增 checkpoint（resume） | 所有 ID 偏移 ❌ | 新 checkpoint 有独立 ID 前缀 ✓ |
| 中间 checkpoint 清理 | 后续 ID 偏移 ❌ | 不影响，每个 checkpoint 独立前缀 ✓ |
| 定位 checkpoint | 无法定位 ❌ | 前缀可直接反查 checkpoint_id ✓ |

### 代码变更

新增 `CheckpointEntry` 结构体，`build_codex_events` 接收 checkpoint_id：

```rust
pub struct CheckpointEntry {
    pub id: String,
    pub state: ReActState,
}

pub fn build_codex_events(
    session_id: &str,
    checkpoints: &[CheckpointEntry],
) -> Vec<CodexEvent>
```

`session.rs` 中加载时同时读取 checkpoint_id：

```rust
let mut stmt = conn.prepare(
    "SELECT checkpoint_id, payload FROM checkpoints
     WHERE thread_id = ?1 ORDER BY metadata_created_at ASC"
)?;

let checkpoints: Vec<CheckpointEntry> = stmt
    .query_map([session_id], |row| {
        let id: String = row.get(0)?;
        let payload: Vec<u8> = row.get(1)?;
        Ok((id, payload))
    })?
    .filter_map(|r| r.ok())
    .filter_map(|(id, payload)| {
        serde_json::from_slice(&payload).ok().map(|state| CheckpointEntry { id, state })
    })
    .collect();
```

### 修改文件

| 文件 | 变更 |
|------|------|
| `cli/src/codex_event_builder.rs` | 新增 `CheckpointEntry` 结构体，`ItemIdCounter` 改用 `checkpoint_id` 前缀 |
| `cli/src/codex_event_builder.rs` | `build_codex_events` 参数改为 `&[CheckpointEntry]` |
| `cli/src/session.rs` | `cat_session()` 读取 `checkpoint_id` 并构建 `CheckpointEntry` |

---

## 任务 5：file_change 类型映射

### 需求

将文件操作工具映射为 Codex `file_change` item 类型。Codex 协议要求：**仅发 `item.completed`，不发 `item.started`**。

### 工具清单（基于实际 TOOL 常量）

| 工具名 | TOOL 常量 | 参数中的路径字段 | Codex kind |
|--------|----------|----------------|------------|
| `write_file` | `TOOL_WRITE_FILE` | `arguments.path` | `add` |
| `edit` | `TOOL_EDIT_FILE` | `arguments.path` | `update` |
| `multiedit` | `TOOL_MULTIEDIT` | `arguments.path` | `update` |
| `apply_patch` | `TOOL_APPLY_PATCH` | 需从 `patchText` 解析 path | 由 hunk 类型决定 |
| `delete_file` | `TOOL_DELETE_FILE` | `arguments.path` | `delete` |
| `move_file` | `TOOL_MOVE_FILE` | `arguments.source` + `arguments.target` | `delete`(source) + `add`(target) |
| `create_dir` | `TOOL_CREATE_DIR` | `arguments.path` | 归为 `mcp_tool_call`（见边界情况） |
| `bash` | `TOOL_BASH` | — | `command_execution`（不变） |
| 其他 | — | — | `mcp_tool_call`（不变） |

### path 提取逻辑

大部分文件工具的参数中有 `path` 字段，可直接提取。特殊情况：

**`move_file`**：参数是 `source` + `target`，生成两条 change：

```json
{
  "changes": [
    { "path": "src/old.rs", "kind": "delete" },
    { "path": "src/new.rs", "kind": "add" }
  ]
}
```

**`apply_patch`**：参数是 `patchText`（一个包含多个 hunk 的字符串），需要解析 patch 文本提取每个 hunk 的 path 和 kind。Hunk 类型：

```rust
// loom/src/tools/file/apply_patch.rs:18
enum Hunk {
    Add { path, contents },    // → kind: "add"
    Delete { path },           // → kind: "delete"
    Update { path, move_path, chunks },  // → kind: "update"
}
```

解析策略：从 `patchText` 中提取 `*** Add File:`, `*** Delete File:`, `*** Update File:` 行，获取 path。

### 方案

```rust
#[derive(Clone, Debug)]
enum ToolClass {
    Command,
    FileChange(FileChangeInfo),
    McpTool,
}

#[derive(Clone, Debug)]
struct FileChangeInfo {
    changes: Vec<FileUpdateChange>,
    status: String,
}

fn classify_tool(name: &str, args: &serde_json::Value, result_ok: bool) -> ToolClass {
    match name {
        "bash" | "powershell" => ToolClass::Command,
        "write_file" => {
            let path = args["path"].as_str().unwrap_or("");
            ToolClass::FileChange(FileChangeInfo {
                changes: vec![FileUpdateChange { path: path.to_string(), kind: "add".to_string() }],
                status: if result_ok { "completed" } else { "failed" }.to_string(),
            })
        }
        "edit" | "multiedit" => {
            let path = args["path"].as_str().unwrap_or("");
            ToolClass::FileChange(FileChangeInfo {
                changes: vec![FileUpdateChange { path: path.to_string(), kind: "update".to_string() }],
                status: if result_ok { "completed" } else { "failed" }.to_string(),
            })
        }
        "delete_file" => {
            let path = args["path"].as_str().unwrap_or("");
            ToolClass::FileChange(FileChangeInfo {
                changes: vec![FileUpdateChange { path: path.to_string(), kind: "delete".to_string() }],
                status: if result_ok { "completed" } else { "failed" }.to_string(),
            })
        }
        "move_file" => {
            let source = args["source"].as_str().unwrap_or("");
            let target = args["target"].as_str().unwrap_or("");
            ToolClass::FileChange(FileChangeInfo {
                changes: vec![
                    FileUpdateChange { path: source.to_string(), kind: "delete".to_string() },
                    FileUpdateChange { path: target.to_string(), kind: "add".to_string() },
                ],
                status: if result_ok { "completed" } else { "failed" }.to_string(),
            })
        }
        "apply_patch" => {
            // 从 patchText 解析 hunk paths
            let patch_text = args["patchText"].as_str().unwrap_or("");
            let changes = parse_patch_hunks(patch_text);
            ToolClass::FileChange(FileChangeInfo {
                changes,
                status: if result_ok { "completed" } else { "failed" }.to_string(),
            })
        }
        _ => ToolClass::McpTool,
    }
}

fn parse_patch_hunks(patch_text: &str) -> Vec<FileUpdateChange> {
    let mut changes = Vec::new();
    for line in patch_text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("*** Add File:") {
            changes.push(FileUpdateChange { path: rest.trim().to_string(), kind: "add".to_string() });
        } else if let Some(rest) = line.strip_prefix("*** Delete File:") {
            changes.push(FileUpdateChange { path: rest.trim().to_string(), kind: "delete".to_string() });
        } else if let Some(rest) = line.strip_prefix("*** Update File:") {
            changes.push(FileUpdateChange { path: rest.trim().to_string(), kind: "update".to_string() });
        }
    }
    changes
}
```

### 事件输出差异

`file_change` 只发 `item.completed`，不发 `item.started`（Codex 协议规定）：

```rust
match classify_tool(&tc.name, &args, !is_error) {
    ToolClass::Command => {
        events.push(CodexEvent::ItemStarted { item: ... });
        // ...
        events.push(CodexEvent::ItemCompleted { item: ... });
    }
    ToolClass::FileChange(info) => {
        let item = file_change_item(&id, info.changes, &info.status);
        events.push(CodexEvent::ItemCompleted { item });
    }
    ToolClass::McpTool => {
        events.push(CodexEvent::ItemStarted { item: ... });
        // ...
        events.push(CodexEvent::ItemCompleted { item: ... });
    }
}
```

### 已知限制与边界情况

#### 1. file_change 失败时无错误详情

Codex 的 `file_change` 协议没有 `error` 字段（与 `mcp_tool_call` 不同），只有 `status: "failed"`。工具执行失败的具体原因（文件不存在、权限不足等）无法在 `file_change` 中表达。

**处理方式**：失败时仍输出 `file_change` + `status: "failed"`，具体错误信息丢失。如果消费端需要错误详情，建议同时输出一个 `mcp_tool_call` item（通过 `arguments` 中的扩展字段传递错误信息），但这会偏离 Codex 协议。

**当前决策**：只输出 `file_change`，接受错误详情丢失。后续如需可扩展。

#### 2. create_dir 的映射争议

Codex `file_change` 的 `kind` 只有 `add`/`delete`/`update`，没有 `mkdir`。将 `create_dir` 映射为 `kind: "add"` 语义不准确（创建的不是文件而是目录）。

**两种方案**：

- A）映射为 `file_change` + `kind: "add"`：与 Codex 的文件新建语义最接近，但可能误导消费端
- B）归为 `mcp_tool_call`：语义准确，但丢失了“这是文件系统变更”的语义

**当前决策**：采用方案 B，`create_dir` 归为 `mcp_tool_call`，不映射为 `file_change`。只有真正修改文件内容的工具才映射为 `file_change`。

#### 3. 逆向可追溯性

`file_change` item 没有 `tool` 字段，消费端无法区分 `write_file` 和 `apply_patch`。Codex 协议设计如此——它关注的是“文件发生了什么变更”，而非“哪个工具引起的变更”。

如果需要追溯原始工具调用，消费端应结合相邻的 `mcp_tool_call` 或 `command_execution` item 中的时间戳/ID 关联。

**无需额外处理**，属于 Codex 协议的设计取舍。

### 修改文件

| 文件 | 变更 |
|------|------|
| `cli/src/codex_event_builder.rs` | 新增 `ToolClass` 枚举、`classify_tool` 函数、`parse_patch_hunks` 函数 |
| `cli/src/codex_event_builder.rs` | 替换 `is_shell_tool` 为 `classify_tool`，增加 `file_change` 路径 |
| `stream-event/src/codex.rs` | 确认 `file_change_item` 函数可用 |

---

## 实施顺序

| 优先级 | 任务 | 预计工作量 | 依赖 |
|--------|------|-----------|------|
| P0 | 任务 1：Turn 粒度拆分 | 2-3h | 无 |
| P0 | 任务 2：降级策略 turn.failed | 0.5h | 任务 1 |
| P1 | 任务 3：工具结果清洗 | 0.5h | 无 |
| P1 | 任务 4：Item ID 稳定性 | 0.5h | 任务 1 |
| P2 | 任务 5：file_change 映射 | 1h | 无 |

建议先完成任务 1（核心），验证通过后依次完成 2-5。

## 相关文档

- [session cat — 会话回放](/docs/deployment/cli-session-cat) — 功能使用指南和待决事项
- [Codex 异常处理](/docs/reference/codex-error-handling) — 错误事件的处理策略
- [Codex 事件协议字段参考](/docs/reference/codex-event-protocol) — 完整的事件类型和字段定义
