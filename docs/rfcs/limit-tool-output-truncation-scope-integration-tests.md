# 集成测试用例：限定工具输出截断范围

## 概述

集成测试验证 `ActNode` → `normalize_tool_output()` → `ToolResult` 端到端流程。
与单元测试的区别：单元测试直接调用 `normalize_tool_output()`，集成测试通过 `ActNode.run()` 驱动完整 pipeline。

## 涉及文件

| 文件 | 说明 |
|------|------|
| `loom/tests/tool_normalize.rs` | API 级集成测试（直接调用公共 API） |
| `loom/tests/react_nodes.rs` | 节点级集成测试（通过 ActNode 驱动） |

## 一、`tool_normalize.rs` — API 级集成测试

### 需修改的现有测试

#### 1. `get_recent_messages_large_output_uses_summary_only`

**当前断言**: `strategy == SummaryOnly, truncated == true`
**改动后**: `get_recent_messages` 不在白名单，应返回 Inline

```rust
// 改为:
fn get_recent_messages_large_output_uses_inline() {
    // ... 同样的 200 行大输出
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert!(result.raw_content.is_some());
    assert_eq!(result.observation_text, text);
}
```

#### 2. `tool_output_hint_can_override_default_strategy`

**当前行为**: `custom_tool` 配 hint 后走 HeadTail
**改动后**: `custom_tool` 不在白名单，hint 被忽略，应返回 Inline

```rust
// 改为:
fn tool_output_hint_is_ignored_for_non_whitelisted_tool() {
    // custom_tool 不在白名单，即使配了 hint 也应 Inline
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}
```

### 新增测试用例

#### I-1. 白名单工具 web_search 大输出 → FileRefWithExcerpt

```rust
#[test]
fn web_search_large_output_uses_file_ref_with_excerpt() {
    with_temp_loom_home(|| {
        let text = make_long_string(20_000);
        let result = normalize_tool_output(
            "web_search",
            &json!({"query": "test"}),
            &text,
            false,
            None,
            NormalizationConfig::runtime_default(),
        );
        assert_eq!(result.strategy, ToolOutputStrategy::FileRefWithExcerpt);
        assert!(result.truncated);
        assert!(result.storage_ref.is_some());
    });
}
```

#### I-2. 白名单工具 web_search 小输出 → Inline

```rust
#[test]
fn web_search_small_output_uses_inline() {
    let text = "search result: 3 items found";
    let result = normalize_tool_output(
        "web_search",
        &json!({"query": "test"}),
        text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}
```

#### I-3. 非白名单工具 read 大输出 → Inline

```rust
#[test]
fn read_large_output_uses_inline() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "read",
        &json!({"path": "/tmp/big.txt"}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}
```

#### I-4. 非白名单工具 grep 大输出 → Inline

```rust
#[test]
fn grep_large_output_uses_inline() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "grep",
        &json!({"pattern": "test"}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}
```

#### I-5. 非白名单工具 invoke_agent 大输出 → Inline

```rust
#[test]
fn invoke_agent_large_output_uses_inline() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "invoke_agent",
        &json!({"agent": "explore", "task": "..."}),
        &text,
        false,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}
```

#### I-6. 非白名单工具 error → Inline（不再 HeadTail）

```rust
#[test]
fn unknown_tool_error_output_uses_inline() {
    let text = make_long_string(5_000);
    let result = normalize_tool_output(
        "unknown_tool",
        &json!({}),
        &text,
        true,
        None,
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}
```

#### I-7. 非白名单工具 + hint → hint 被忽略

```rust
#[test]
fn non_whitelisted_tool_ignores_safe_inline_chars_hint() {
    let text = make_long_string(500);
    let hint = ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).safe_inline_chars(100);
    let result = normalize_tool_output(
        "read",
        &json!({}),
        &text,
        false,
        Some(&hint),
        NormalizationConfig::default(),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
}
```

#### I-8. 白名单工具 + hint → hint 仍生效

```rust
#[test]
fn whitelisted_tool_respects_safe_inline_chars_hint() {
    let text = make_long_string(500);
    let hint = ToolOutputHint::preferred(ToolOutputStrategy::HeadTail).safe_inline_chars(100);
    let result = normalize_tool_output(
        "bash",
        &json!({"command": "test"}),
        &text,
        false,
        Some(&hint),
        NormalizationConfig::default(),
    );
    assert_ne!(result.strategy, ToolOutputStrategy::Inline);
    assert!(result.truncated);
}
```

#### I-9. 非白名单工具预算耗尽 → 仍 Inline

```rust
#[test]
fn non_whitelisted_tool_ignores_observation_budget() {
    let text = make_long_string(20_000);
    let result = normalize_tool_output(
        "read",
        &json!({}),
        &text,
        false,
        None,
        NormalizationConfig::default().with_used_observation_chars(7_950),
    );
    assert_eq!(result.strategy, ToolOutputStrategy::Inline);
    assert!(!result.truncated);
    assert_eq!(result.observation_text, text);
}
```

#### I-10. 白名单工具预算耗尽 → 降级

```rust
#[test]
fn whitelisted_tool_degrades_when_budget_spent() {
    let text = make_long_string(4_500);
    let result = normalize_tool_output(
        "bash",
        &json!({"command": "test"}),
        &text,
        false,
        None,
        NormalizationConfig::default().with_used_observation_chars(7_950),
    );
    assert!(matches!(
        result.strategy,
        ToolOutputStrategy::SummaryOnly | ToolOutputStrategy::FileRefWithExcerpt
    ));
}
```

## 二、`react_nodes.rs` — ActNode 级集成测试

### 需修改的现有测试

#### 1. `act_node_uses_tool_spec_output_hint`

**当前行为**: `hinted_tool` 配了 `SummaryOnly` hint → 输出被截断
**改动后**: `hinted_tool` 不在白名单，hint 被忽略 → Inline

```rust
// 改名 + 改断言:
async fn act_node_non_whitelisted_tool_ignores_output_hint() {
    // ... 同样的 HintingToolSource + 大输出
    assert_eq!(
        out.tool_results[0].strategy,
        Some(ToolOutputStrategy::Inline)
    );
    assert!(!out.tool_results[0].truncated);
}
```

### 新增测试用例

#### N-1. ActNode 对白名单工具 bash 执行截断

需要一个返回大输出的 MockToolSource：

```rust
struct LargeOutputToolSource {
    tool_name: String,
    result: String,
}

#[async_trait]
impl ToolSource for LargeOutputToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        Ok(vec![ToolSpec {
            name: self.tool_name.clone(),
            description: Some("returns large output".to_string()),
            input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            output_hint: None,
        }])
    }
    async fn call_tool(&self, _name: &str, _arguments: Value) -> Result<ToolCallContent, ToolSourceError> {
        Ok(ToolCallContent::text(self.result.clone()))
    }
}

#[tokio::test]
async fn act_node_bash_large_output_is_truncated() {
    let large = "line\n".repeat(2_500); // ~10,000 chars
    let node = ActNode::new(Box::new(LargeOutputToolSource {
        tool_name: "bash".into(),
        result: large.clone(),
    }));
    let state = ReActState {
        messages: vec![],
        tool_calls: vec![ToolCall {
            name: "bash".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        }],
        tool_results: vec![],
        ..Default::default()
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.tool_results.len(), 1);
    assert_eq!(out.tool_results[0].strategy, Some(ToolOutputStrategy::HeadTail));
    assert!(out.tool_results[0].truncated);
    assert!(out.tool_results[0].content.contains("Head:"));
}
```

#### N-2. ActNode 对非白名单工具 read 保留完整输出

```rust
#[tokio::test]
async fn act_node_read_large_output_is_not_truncated() {
    let large = "line\n".repeat(2_500); // ~10,000 chars
    let node = ActNode::new(Box::new(LargeOutputToolSource {
        tool_name: "read".into(),
        result: large.clone(),
    }));
    let state = ReActState {
        messages: vec![],
        tool_calls: vec![ToolCall {
            name: "read".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        }],
        tool_results: vec![],
        ..Default::default()
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.tool_results.len(), 1);
    assert_eq!(out.tool_results[0].strategy, Some(ToolOutputStrategy::Inline));
    assert!(!out.tool_results[0].truncated);
    assert_eq!(out.tool_results[0].content, large);
}
```

#### N-3. ActNode 对非白名单工具 + hint 忽略 hint

```rust
#[tokio::test]
async fn act_node_non_whitelisted_tool_ignores_hint() {
    // 使用现有 HintingToolSource（hinted_tool 配了 SummaryOnly hint）
    // 但加大输出
    let large_result = (0..400)
        .map(|i| format!("line {} {}", i, "x".repeat(20)))
        .collect::<Vec<_>>()
        .join("\n");
    let node = ActNode::new(Box::new(HintingToolSource {
        result: large_result.clone(),
    }));
    let state = ReActState {
        messages: vec![],
        tool_calls: vec![ToolCall {
            name: "hinted_tool".into(),
            arguments: "{}".into(),
            id: Some("hint-1".into()),
        }],
        tool_results: vec![],
        ..Default::default()
    };
    let (out, _) = node.run(state).await.unwrap();
    assert_eq!(out.tool_results[0].strategy, Some(ToolOutputStrategy::Inline));
    assert!(!out.tool_results[0].truncated);
    assert_eq!(out.tool_results[0].content, large_result);
}
```

#### N-4. ActNode 多工具混合：白名单截断 + 非白名单保留

```rust
#[tokio::test]
async fn act_node_mixed_tools_whitelist_and_others() {
    // 需要一个支持多工具的 MockToolSource
    // bash (白名单) + read (非白名单) 同时调用
    // bash 结果被截断，read 结果完整保留
}
```

## 三、测试用例汇总

### `tool_normalize.rs`（API 级）

| 编号 | 测试名 | 分类 | 新增/修改 |
|------|--------|------|----------|
| I-1 | web_search_large_output_uses_file_ref_with_excerpt | 白名单 | 新增 |
| I-2 | web_search_small_output_uses_inline | 白名单 | 新增 |
| I-3 | read_large_output_uses_inline | 非白名单 | 新增 |
| I-4 | grep_large_output_uses_inline | 非白名单 | 新增 |
| I-5 | invoke_agent_large_output_uses_inline | 非白名单 | 新增 |
| I-6 | unknown_tool_error_output_uses_inline | 非白名单 | 新增 |
| I-7 | non_whitelisted_tool_ignores_safe_inline_chars_hint | hint 忽略 | 新增 |
| I-8 | whitelisted_tool_respects_safe_inline_chars_hint | hint 生效 | 新增 |
| I-9 | non_whitelisted_tool_ignores_observation_budget | 预算 | 新增 |
| I-10 | whitelisted_tool_degrades_when_budget_spent | 预算 | 新增 |
| - | get_recent_messages_large_output_uses_summary_only | 非白名单 | **修改** → Inline |
| - | tool_output_hint_can_override_default_strategy | hint 忽略 | **修改** → Inline |

### `react_nodes.rs`（ActNode 级）

| 编号 | 测试名 | 分类 | 新增/修改 |
|------|--------|------|----------|
| N-1 | act_node_bash_large_output_is_truncated | 白名单 | 新增 |
| N-2 | act_node_read_large_output_is_not_truncated | 非白名单 | 新增 |
| N-3 | act_node_non_whitelisted_tool_ignores_hint | hint 忽略 | 新增 |
| N-4 | act_node_mixed_tools_whitelist_and_others | 混合 | 新增 |
| - | act_node_uses_tool_spec_output_hint | hint 忽略 | **修改** → Inline |
