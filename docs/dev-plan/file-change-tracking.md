# React Agent 文件修改记录 — 开发方案

## 1. 背景与目标

### 现状
- 文件操作工具（`edit`、`write_file`、`multiedit`、`apply_patch`）已经通过 `ToolCallContent::Diff { path, old_text, new_text }` 返回 diff 信息
- `ActNode` 将 diff 序列化为 JSON 后经 `normalize_tool_output` 归一化，在 `ObserveNode` 中以纯文本 `Tool {name} result: ...` 呈现
- `ReActState` 没有字段累积文件变更；agent 运行结束后无法直接获取"改了哪些文件"
- ACP 层（`stream_bridge.rs`）已能从 `StreamEvent::ToolEnd` 中反序列化 `ToolCallContent::Diff` 并转为 `StreamUpdate::Diff`
- CLI 层（`codex_event_builder.rs`）也能识别 Diff 类型

### 目标
1. **在 `ReActState` 中累积结构化的文件变更记录**，agent 运行结束后可直接查询
2. **通过 StreamEvent 实时推送文件变更**，消费者（CLI / ACP server）无需反序列化猜测
3. **向后兼容**，不影响现有 LLM 上下文注入逻辑（ObserveNode）
4. **支持 checkpoint 持久化**，恢复会话时保留历史变更记录

## 2. 数据模型

### 2.1 新增 `FileChangeRecord` 结构体

```rust
// loom/src/state/react_state.rs

/// 单次文件变更的记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangeRecord {
    /// 变更的文件路径（相对 workspace）
    pub path: String,
    /// 原始内容（None = 新建文件）
    pub old_text: Option<String>,
    /// 修改后内容（None = 删除文件）
    pub new_text: Option<String>,
    /// 触发变更的工具名（edit / write_file / multiedit / apply_patch 等）
    pub tool_name: String,
    /// 发生变更时的 turn 序号
    pub turn: u32,
    /// 调用 ID，用于关联 StreamEvent
    pub call_id: Option<String>,
}
```

### 2.2 `ReActState` 新增字段

```rust
pub struct ReActState {
    // ... existing fields ...
    
    /// 本次运行累积的文件变更记录
    #[serde(default)]
    pub file_changes: Vec<FileChangeRecord>,
}
```

### 2.3 `StreamEvent` 新增 `FileChange` variant

```rust
// loom/src/stream/stream_event.rs

pub enum StreamEvent<S> {
    // ... existing variants ...
    
    /// 文件变更事件（ActNode 在检测到 Diff 时发射）
    FileChange {
        /// 触发的 tool call ID
        call_id: Option<String>,
        /// 工具名
        tool_name: String,
        /// 变更的文件路径
        path: String,
        /// 原始内容
        old_text: Option<String>,
        /// 新内容
        new_text: Option<String>,
    },
}
```

## 3. 核心改动

### 3.1 `ActNode` — 提取 Diff 并发射事件

**文件**: `loom/src/agent/react/act_node.rs`

**改动位置**: `run_with_context` 方法中，tool 成功返回后的 `Ok(content)` 分支（约 line 506-557）

**逻辑**:
```
1. tool 返回 ToolCallContent 后，检查是否为 Diff 类型
2. 如果是 Diff：
   a. 构造 FileChangeRecord，append 到 state.file_changes
   b. 发射 StreamEvent::FileChange
3. 无论是否 Diff，继续走已有的 normalize + push tool_results 逻辑
```

**关键代码位置**: `act_node.rs:506-557`（`Ok(content)` 分支）

需要额外做的事：
- 在 `call_tool_with_context` 返回后、`normalize_tool_output` 之前，检查 `content` 是否为 `Diff`
- 从 `content` 中提取 `path`/`old_text`/`new_text`，构造 `FileChangeRecord`
- 通过 `run_ctx.stream_tx` 发射 `StreamEvent::FileChange`

**同样需要处理 `run` 方法（无 context 版本）**: `act_node.rs:250-272`
- 无 stream channel 时，仅 append `FileChangeRecord` 到 state，不发射事件

### 3.2 `ObserveNode` — 无需改动

`ObserveNode` 只负责消费 `tool_results` 生成 messages，`file_changes` 不参与 LLM 上下文注入。

但考虑一个 *可选优化*：在 `observe` 的 observation text 中附加上简要变更摘要（如 `[File modified: src/main.rs]`），帮助 LLM 理解文件已被修改。这是可选的，可后续迭代。

### 3.3 `ReactRunner` — 无需改动

runner 编排 graph 节点，`file_changes` 作为 `ReActState` 的一部分自动在节点间传递。

### 3.4 `ToolResult` — 可选扩展

**文件**: `loom/src/state/react_state.rs`

可以考虑在 `ToolResult` 中添加 `diff_path: Option<String>` 字段，方便后续消费者直接从 `ToolResult` 判断是否涉及文件变更。但这不是必须的，因为 `ToolCallContent::Diff` 已经在 ActNode 层被处理。

## 4. 下游适配

### 4.1 ACP Stream Bridge

**文件**: `loom-acp/src/stream_bridge.rs`

当前通过反序列化 `raw_result` 来检测 Diff（line 258-280）。新增 `StreamEvent::FileChange` 后：
- 优先匹配 `StreamEvent::FileChange`，直接映射为 `StreamUpdate::Diff`
- 保留原有反序列化逻辑作为 fallback（兼容未升级的 tool source）

### 4.2 CLI Event Builder

**文件**: `cli/src/codex_event_builder.rs`

当前 `tool_call_display_text` 已处理 `ToolCallContent::Diff`（line 29-33）。新增处理 `StreamEvent::FileChange` 的分支，可简化 diff 检测逻辑。

### 4.3 CLI `on_event_react`

**文件**: `cli/src/run/agent.rs`（`on_event_react` 函数）

在 event handler 中新增 `StreamEvent::FileChange` 分支，用于：
- 控制台输出文件变更摘要（如 `📝 Modified: src/main.rs`）
- 累积变更列表供最终输出

## 5. Checkpoint 兼容性

`file_changes` 字段标记了 `#[serde(default)]`，所以：
- 旧 checkpoint（无此字段）反序列化时自动为 `vec![]`
- 新 checkpoint 包含完整变更记录
- 恢复会话时自动继承历史记录

如果需要"每次新消息清空 file_changes"的语义（仅记录当前 turn 的变更），可在 `build_react_initial_state` 中根据是否为新会话决定是否清空。建议先实现"全程累积"，后续按需调整。

## 6. 实施步骤

| 步骤 | 内容 | 涉及文件 |
|------|------|----------|
| **Step 1** | 定义 `FileChangeRecord` 结构体 + `ReActState.file_changes` 字段 | `loom/src/state/react_state.rs` |
| **Step 2** | `StreamEvent` 新增 `FileChange` variant | `loom/src/stream/stream_event.rs` |
| **Step 3** | `ActNode.run_with_context` 中提取 Diff 并发射事件 + 累积 state | `loom/src/agent/react/act_node.rs` |
| **Step 4** | `ActNode.run`（无 context 版本）中提取 Diff 并累积 state | `loom/src/agent/react/act_node.rs` |
| **Step 5** | 导出 `FileChangeRecord` 到 `loom/src/lib.rs` | `loom/src/lib.rs` |
| **Step 6** | ACP stream bridge 适配 `StreamEvent::FileChange` | `loom-acp/src/stream_bridge.rs` |
| **Step 7** | CLI event builder + on_event_react 适配 | `cli/src/codex_event_builder.rs`, `cli/src/run/agent.rs` |
| **Step 8** | 单元测试 | 新增 `tests` 模块 |

## 7. 风险与注意事项

1. **内存开销**: `file_changes` 存储完整 old/new text，长文件多次编辑会占用内存。可设置单条上限（如 100KB），超限时仅记录 path + summary。
2. **序列化开销**: checkpoint 包含 `file_changes` 后体积增大。可选策略：checkpoint 仅保留最近 N 条，或只存摘要。
3. **并发安全**: `ActNode` 是单线程顺序执行 tool calls，不存在并发写入 `file_changes` 的问题。
4. **ToolSource 兼容**: 并非所有 ToolSource 返回 Diff。MCP 工具、自定义工具通常返回 `Text`。方案对这些工具无影响，`file_changes` 仅记录实际产生 Diff 的调用。

## 8. 验收标准

- [ ] React agent 运行后，`ReActState.file_changes` 包含所有文件变更记录
- [ ] 每次 `edit`/`write_file`/`multiedit`/`apply_patch` 调用产生一条 `FileChangeRecord`
- [ ] `StreamEvent::FileChange` 在 tool 执行完成后实时推送
- [ ] ACP server 能正确接收并转发 `StreamUpdate::Diff`
- [ ] CLI 控制台显示文件变更摘要
- [ ] Checkpoint 恢复后保留历史变更记录
- [ ] 非 Diff 类型的 tool 调用不受影响
