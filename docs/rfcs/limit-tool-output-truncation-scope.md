# RFC: 限定工具输出截断范围

## 状态

已确认

## 动机

当前 `determine_strategy()` 对所有工具都执行截断策略选择。对于 `read`、`grep`、`glob` 等工具，用户明确要求返回完整内容，截断反而损失关键信息。只有少数工具（bash、web_fetcher、web_search）的输出天然冗长且不可控，需要截断。

## 方案

### 改动范围

仅修改 `loom/src/state/tool_output_normalizer.rs` 中的 `determine_strategy()` 函数。

### 设计

新增白名单常量，`determine_strategy()` 入口处短路返回：

```rust
/// 需要截断的工具白名单。不在列表中的工具始终 Inline（不截断）。
const TRUNCATABLE_TOOLS: &[&str] = &["bash", "powershell", "web_fetcher", "web_search"];

fn determine_strategy(
    tool_name: &str,
    raw_chars: usize,
    is_error: bool,
    output_hint: Option<&ToolOutputHint>,
    remaining_budget: usize,
    config: &NormalizationConfig,
) -> ToolOutputStrategy {
    if !TRUNCATABLE_TOOLS.contains(&tool_name) {
        return ToolOutputStrategy::Inline;
    }

    // ... 原有逻辑不变
}
```

### 行为变更对照

| 工具 | 改动前 | 改动后 |
|------|--------|--------|
| `bash` / `powershell` | ≤4k Inline → HeadTail → FileRefWithExcerpt | **不变** |
| `web_fetcher` | ≤4k Inline → FileRefWithExcerpt | **不变** |
| `web_search` | 走默认 `_` 分支（SummaryOnly/HeadTail） | 走 `web_fetcher` 同等逻辑 |
| `get_recent_messages` | ≤2k Inline → SummaryOnly | **Inline** |
| `invoke_agent` | ≤4k Inline → HeadTail → FileRefWithExcerpt | **Inline** |
| `read` / `grep` / `glob` / 其他 | 走默认 `_` 分支 | **Inline** |

### 对预算机制的影响

`observation_budget`（单轮 8,000 字符）对非白名单工具不再生效：

- 白名单工具：预算耗尽后降级为 SummaryOnly / FileRef
- 非白名单工具：始终 Inline，不受预算约束

`apply_observation_budget()` 对 Inline 策略无操作（已在 `build_inline_output` 中设 `truncated: false`），无需额外改动。

### 对 ToolOutputHint 的影响

非白名单工具的 `ToolOutputHint` 配置（preferred_strategy、safe_inline_chars 等）被忽略，始终返回 Inline。白名单工具仍受 ToolOutputHint 影响。

### 对文件持久化的影响

非白名单工具不再触发 `persist_output()`，大输出直接进入上下文。`enable_persistence` 配置对这些工具无效。

## 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| `read` 返回超大文件撑大上下文 | 高 token 消耗 | 将 `read` 加入 `TRUNCATABLE_TOOLS` |
| `invoke_agent` 子代理返回长结果 | 上下文溢出 | 将 `invoke_agent` 加入白名单 |
| 新工具输出不可控 | 默认不截断可能爆炸 | 按需加入白名单 |

## 相关文件

| 文件 | 说明 |
|------|------|
| `loom/src/state/tool_output_normalizer.rs` | 唯一改动文件 |
| `docs/design/tool-output-normalization.md` | 现有设计文档 |

## 决策记录

1. **`powershell` 与 `bash` 同等对待** — 已确认，两者均在白名单中
2. **`invoke_agent` 不截断** — 子代理结果完整保留，不再 HeadTail/SummaryOnly
3. **不保留 ToolOutputHint 对非白名单工具的覆盖能力** — 非白名单工具无视 hint，始终 Inline
