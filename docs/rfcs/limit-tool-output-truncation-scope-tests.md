# 测试用例：限定工具输出截断范围

## 改动影响

仅修改 `loom/src/state/tool_output_normalizer.rs` 的 `determine_strategy()` 函数。
测试添加在同一文件的 `mod tests` 中。

## 现有需修改的测试

| 测试 | 原断言 | 改动后行为 | 需要修改 |
|------|--------|-----------|---------|
| `test_normalize_get_recent_messages_summary` | `SummaryOnly` | `Inline` | 是 |
| `test_normalize_error_gets_head_tail` | `HeadTail`（unknown_tool + error） | `Inline` | 是 |

## 新增测试用例

### A. 白名单工具 — 行为不变

#### A1. bash 小输出 → Inline

```
tool: "bash", output: 100 chars (< inline_limit)
expect: strategy == Inline, truncated == false, observation_text == 原文
```

#### A2. bash 中等输出 → HeadTail

```
tool: "bash", output: 5000 chars (> inline_limit, < file_ref_threshold)
expect: strategy == HeadTail, truncated == true, observation_text contains "Head:"
```

#### A3. bash 超大输出 → FileRefWithExcerpt

```
tool: "bash", output: 20000 chars (> file_ref_threshold)
expect: strategy == FileRefWithExcerpt, truncated == true
```

#### A4. powershell 与 bash 等价

```
tool: "powershell", output: 5000 chars
expect: strategy == HeadTail (同 A2)
```

#### A5. web_fetcher 小输出 → Inline

```
tool: "web_fetcher", output: 100 chars
expect: strategy == Inline
```

#### A6. web_fetcher 大输出 → FileRefWithExcerpt

```
tool: "web_fetcher", output: 20000 chars (> inline_limit)
expect: strategy == FileRefWithExcerpt
```

#### A7. web_search 小输出 → Inline

```
tool: "web_search", output: 100 chars
expect: strategy == Inline
```

#### A8. web_search 大输出 → FileRefWithExcerpt

```
tool: "web_search", output: 20000 chars
expect: strategy == FileRefWithExcerpt (同 web_fetcher 分支)
```

### B. 非白名单工具 — 始终 Inline

#### B1. read 大输出 → Inline

```
tool: "read", output: 20000 chars
expect: strategy == Inline, truncated == false, observation_text == 原文完整
```

#### B2. grep 大输出 → Inline

```
tool: "grep", output: 20000 chars
expect: strategy == Inline, truncated == false
```

#### B3. glob 大输出 → Inline

```
tool: "glob", output: 20000 chars
expect: strategy == Inline, truncated == false
```

#### B4. invoke_agent 大输出 → Inline

```
tool: "invoke_agent", output: 20000 chars
expect: strategy == Inline, truncated == false
```

#### B5. get_recent_messages 大输出 → Inline

```
tool: "get_recent_messages", output: 5000 chars (> inline_limit/2)
expect: strategy == Inline, truncated == false
```

#### B6. unknown_tool + error → Inline

```
tool: "unknown_tool", output: 5000 chars, is_error: true
expect: strategy == Inline, truncated == false
```

#### B7. unknown_tool 小输出 → Inline

```
tool: "any_random_tool", output: 100 chars
expect: strategy == Inline, truncated == false
```

### C. ToolOutputHint 被忽略（非白名单）

#### C1. 非 白名单工具配 preferred_strategy → 忽略

```
tool: "read", output: 20000 chars
hint: ToolOutputHint { preferred_strategy: Some(HeadTail), .. }
expect: strategy == Inline (hint 被忽略)
```

#### C2. 非白名单工具配 safe_inline_chars → 忽略

```
tool: "read", output: 500 chars
hint: ToolOutputHint { safe_inline_chars: Some(100), .. }
expect: strategy == Inline (即使 output > 100，仍 Inline)
```

#### C3. 非白名单工具配 prefer_head_tail → 忽略

```
tool: "read", output: 20000 chars
hint: ToolOutputHint { prefer_head_tail: true, .. }
expect: strategy == Inline
```

### D. ToolOutputHint 仍生效（白名单工具）

#### D1. bash 配 safe_inline_chars → 按_hint 生效

```
tool: "bash", output: 500 chars
hint: ToolOutputHint { safe_inline_chars: Some(100), .. }
expect: strategy != Inline (500 > 100，触发截断)
```

#### D2. web_fetcher 配 preferred_strategy → 按 hint 生效

```
tool: "web_fetcher", output: 5000 chars
hint: ToolOutputHint { preferred_strategy: Some(SummaryOnly), .. }
expect: strategy == SummaryOnly
```

### E. 预算机制

#### E1. 白名单工具预算耗尽 → 降级

```
tool: "bash", output: 5000 chars, used_observation_chars: 7900 (剩余 100)
expect: strategy 降级为 SummaryOnly 或 FileRef
```

#### E2. 非白名单工具预算耗尽 → 仍 Inline

```
tool: "read", output: 20000 chars, used_observation_chars: 7900 (剩余 100)
expect: strategy == Inline (预算对非白名单无效)
```

### F. 边界

#### F1. 白名单工具输出恰好 inline_limit → Inline

```
tool: "bash", output: exactly 4000 chars
expect: strategy == Inline
```

#### F2. 白名单工具输出 inline_limit + 1 → 截断

```
tool: "bash", output: 4001 chars
expect: strategy == HeadTail
```

#### F3. 非白名单工具空输出 → Inline

```
tool: "read", output: ""
expect: strategy == Inline, observation_text == ""
```

## 测试用例汇总

| 分类 | 数量 | 说明 |
|------|------|------|
| A: 白名单工具行为不变 | 8 | bash/powershell/web_fetcher/web_search 各场景 |
| B: 非白名单工具始终 Inline | 7 | read/grep/glob/invoke_agent/get_recent_messages/unknown |
| C: 非 白名单 hint 被忽略 | 3 | preferred_strategy/safe_inline_chars/prefer_head_tail |
| D: 白名单 hint 仍生效 | 2 | bash + web_fetcher hint |
| E: 预算机制 | 2 | 白名单降级 + 非白名单忽略 |
| F: 边界 | 3 | 恰好/超1/空输出 |
| **总计** | **25** | |
