# Tool Output Normalization — 设计文档

## 概述

工具返回结果可能非常庞大（如 `bash` 输出数千行日志、`web_fetcher` 返回完整网页）。如果将原始输出直接注入 LLM 上下文，会导致 token 消耗暴增甚至超出模型上下文窗口。

**Tool Output Normalization** 在 `ActNode` 调用工具后、`ObserveNode` 注入上下文前，统一处理工具输出，控制上下文大小。

## 数据流

```
Tool 调用
  │
  ▼
ActNode (act_node.rs)
  │  调用 normalize_tool_output()
  ▼
normalize_tool_output() (tool_output_normalizer.rs)
  │  选择策略 → 截断/持久化 → 生成 NormalizedToolOutput
  ▼
ToolResult (react_state.rs)
  │  From<NormalizedToolOutput> 转换
  ▼
ObserveNode (observe_node.rs)
  │  使用 tr.observation() 注入 LLM 上下文
  ▼
CLI / ACP / Telegram
  │  使用 tr.display() 显示给用户
  ▼
```

## 核心模块

### `loom/src/state/tool_output_normalizer.rs`

#### `NormalizationConfig` (line 61)

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `inline_limit` | 4,000 | 内联结果字符上限 |
| `display_limit` | 1,000 | 显示文本字符上限 |
| `excerpt_limit` | 1,200 | 文件引用摘要字符上限 |
| `head_tail_limit` | 600 | HeadTail 头/尾各保留字符数 |
| `file_ref_threshold` | 10,000 | 超过此值切换为文件持久化 |
| `observation_budget` | 8,000 | 单轮所有工具输出观察总预算 |
| `used_observation_chars` | 0 | 已消耗的观察字符数 |
| `enable_persistence` | false | 是否将大输出持久化到磁盘 |
| `storage_dir` | None | 持久化文件存放目录 |

运行时默认 `NormalizationConfig::runtime_default()` 启用持久化，目录为 `default_tool_output_dir()`。

#### `ToolOutputStrategy` (line 16)

| 策略 | 行为 | 适用场景 |
|------|------|----------|
| `Inline` | 完整保留原始输出 | 小结果（≤ inline_limit） |
| `HeadTail` | 保留头部和尾部，中间用 `...` 连接 | bash/powershell 日志输出 |
| `SummaryOnly` | 仅保留摘要描述，不含内联内容 | get_recent_messages 等结构化数据 |
| `FileRef` | 持久化到文件，仅返回文件路径引用 | 超大输出且无预算 |
| `FileRefWithExcerpt` | 持久化到文件 + 返回小段摘要 | 超大输出但仍有预算展示摘要 |

#### `NormalizedToolOutput` (line 122)

| 字段 | 类型 | 说明 |
|------|------|------|
| `raw_content` | `Option<String>` | 原始输出（过大时为 None） |
| `observation_text` | `String` | 注入 LLM 下一轮的文本 |
| `display_text` | `String` | 显示给用户的文本 |
| `storage_ref` | `Option<ToolStorageRef>` | 文件持久化引用 |
| `strategy` | `ToolOutputStrategy` | 使用的策略 |
| `raw_chars` | `usize` | 原始输出字符数 |
| `observation_chars` | `usize` | 观察文本字符数（用于预算跟踪） |
| `truncated` | `bool` | 是否被截断 |

### `normalize_tool_output()` (line 159)

入口函数，流程：

1. 计算剩余观察预算 `remaining_budget`
2. 调用 `determine_strategy()` 选择策略
3. 如需持久化（FileRef/FileRefWithExcerpt），调用 `persist_output()` 写入文件
4. 根据策略构建 `NormalizedToolOutput`：
   - **Inline**: `build_inline_output()` — observation = 完整文本, display = 截断到 display_limit
   - **HeadTail**: `build_head_tail_output()` — 将文本分为头/尾各 head_tail_limit 字符
   - **SummaryOnly**: `build_summary_output()` — 仅保留摘要描述 + 存储路径提示
   - **FileRef/FileRefWithExcerpt**: `build_file_ref_output()` — 文件路径 + 可选摘要
5. 调用 `apply_observation_budget()` 在总预算内二次裁剪

### `determine_strategy()` (line 348)

策略选择优先级：

1. **ToolOutputHint**（工具级自定义配置）
   - 有 `safe_inline_chars` 且输出 ≤ 该值 → Inline
   - 有 `preferred_strategy` → 使用指定策略（受预算约束降级）
   - `prefer_head_tail` → 优先 HeadTail
2. **工具名特定策略**（无 hint 时）：
   - `bash` / `powershell`: ≤4k Inline → ≤10k HeadTail → FileRefWithExcerpt
   - `web_fetcher` / `mcp_call_tool`: ≤4k Inline → FileRefWithExcerpt
   - `get_recent_messages`: ≤2k Inline → SummaryOnly
   - `invoke_agent`: ≤4k Inline → ≤10k HeadTail → FileRefWithExcerpt
   - 其他: ≤4k Inline → ≤10k (错误:HeadTail, 正常:SummaryOnly) → FileRefWithExcerpt
3. **预算约束降级**：
   - `remaining_budget == 0` → FileRef 或 SummaryOnly
   - `raw_chars > remaining_budget` → 按工具名降级

### `truncate_text()` (line 488)

底层截断函数，所有截断操作的最终执行者：

```rust
fn truncate_text(text: &str, max_chars: usize) -> String {
    // max_chars == 0 → 返回空串
    // chars().count() ≤ max_chars → 原样返回
    // 否则取 max_chars 个 Unicode 字符 + "..."
}
```

- 基于 Unicode 字符边界（`chars()`），不按字节截断
- 截断后追加 `...`，总长度可能略超 `max_chars`

## 数据结构

### `ToolResult` — `loom/src/state/react_state.rs:89`

`NormalizedToolOutput` 通过 `From` trait (line 193) 转换为 `ToolResult`：

| ToolResult 字段 | 来源 |
|----------------|------|
| `content` | `normalized.observation_text`（向后兼容） |
| `raw_content` | `normalized.raw_content` |
| `observation_text` | `normalized.observation_text` |
| `display_text` | `normalized.display_text` |
| `storage_ref` | `normalized.storage_ref` |
| `strategy` | `normalized.strategy` |
| `raw_chars` | `normalized.raw_chars` |
| `observation_chars` | `normalized.observation_chars` |
| `truncated` | `normalized.truncated` |

关键方法：
- `tr.observation()` → 注入 LLM 的文本（优先 observation_text，fallback content）
- `tr.display()` → 显示给用户的文本（优先 display_text → observation_text → content）
- `tr.raw()` → 原始输出（如果未被丢弃）

## 各层截断函数汇总

### Agent 层 — `loom/src/agent/react/act_node.rs`

| 函数 | 位置 | 用途 | 截断方式 |
|------|------|------|----------|
| `truncate_for_log()` | :46 | 日志预览 | 字节长度，追加 `...` |
| `truncate_for_display()` | :54 | 流式事件显示 | Unicode 字符数，`max==0` 返回空串 |

### CLI 层

| 文件 | 函数 | 位置 | 用途 | 配置 |
|------|------|------|------|------|
| `cli/src/display_limits.rs` | `truncate_message()` | :11 | 用户消息/回复截断 | env: `HELVE_MAX_MESSAGE_LEN`(默认200), `HELVE_MAX_REPLY_LEN`(默认0=不截断) |
| `cli/src/output.rs` | `emit_text_reply()` | :152 | 回复输出截断 | `max_reply_len==0` 不截断 |
| `cli/src/run/display.rs` | `truncate_display()` | :14 | 状态/消息 stderr 调试显示 | 与 `truncate_message` 逻辑相同 |

### ACP/Terminal 层

| 文件 | 函数/逻辑 | 位置 | 用途 |
|------|----------|------|------|
| `loom-acp/src/terminal.rs` | `output_byte_limit` 滚动窗口 | :202 | 终端输出字节限制，超出时丢弃头部，标记 `truncated=true` |
| `loom-acp/src/stream_bridge.rs` | `truncate_path()` | :808 | 路径显示截断（保留尾部，max 60 chars） |

### Telegram Bot 层

| 文件 | 函数 | 位置 | 用途 |
|------|------|------|------|
| `telegram-bot/src/utils.rs` | `sanitize_for_display()` | :148 | 清理换行/制表符 + 可选截断 |

## Observation Budget 机制

单轮多个工具调用共享 8,000 字符的观察预算：

```
Turn Start (budget = 8000)
  │
  ├── Tool 1: bash → 3000 chars → remaining = 5000
  ├── Tool 2: bash → 5000 chars → remaining = 0
  └── Tool 3: bash → 策略降级为 SummaryOnly/FileRef
```

- `ActNode` 通过 `used_observation_chars` 在同一轮次中累计已消耗字符
- 预算耗尽后，`determine_strategy()` 自动降级为 FileRef 或 SummaryOnly
- `apply_observation_budget()` 在最终输出上再次确保不超预算

## 文件持久化

当策略为 FileRef 或 FileRefWithExcerpt 时：

1. 原始输出写入 `storage_dir` 下的文件
2. 文件名格式：`{tool_name}_{counter}_{timestamp}.txt`
3. `ToolStorageRef` 包含文件路径和元数据
4. `ObserveNode` 在注入上下文时附带 "Full output saved to: {path}" 提示
5. LLM 可通过 `read` 工具读取完整输出

## 相关文件索引

| 文件 | 说明 |
|------|------|
| `loom/src/state/tool_output_normalizer.rs` | 核心归一化逻辑 |
| `loom/src/state/react_state.rs` | ToolResult 数据结构 |
| `loom/src/agent/react/act_node.rs` | 工具调用 + 归一化入口 |
| `loom/src/agent/react/observe_node.rs` | 观察节点（消费归一化结果） |
| `loom/src/lib.rs` | 公共导出（normalize_tool_output 等） |
| `cli/src/display_limits.rs` | CLI 消息截断 |
| `cli/src/output.rs` | CLI 回复输出 |
| `cli/src/run/display.rs` | CLI 调试显示 |
| `loom-acp/src/terminal.rs` | 终端输出缓冲截断 |
| `loom-acp/src/stream_bridge.rs` | 路径截断 |
| `telegram-bot/src/utils.rs` | Telegram 显示截断 |
