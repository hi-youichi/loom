---
sidebar_position: 1
title: "ADR: ActNode 重构"
description: "ActNode 重构计划"
---

# ADR: ActNode 重构计划

- **状态**：提议中
- **日期**：2025-08-19
- **关联 Issue**：[#32](https://github.com/hi-youichi/loom/issues/32)
- **影响文件**：`loom/src/agent/react/act_node.rs`

## 背景

`ActNode` 实现了 `Node<ReActState>` trait，提供 `run` 和 `run_with_context` 两个方法。两者共享约 80% 的核心逻辑（审批流程、工具调用、输出标准化、错误处理、call_id 回填），但各自独立实现，导致：

1. **维护成本高** — 修复 bug 或添加功能需要同步修改两处
2. **行为不一致** — 序列化方式、ToolCallContext 构造存在差异
3. **测试困难** — 无法集中测试核心逻辑

## 已识别问题

### P0：架构

#### 1. `run` 与 `run_with_context` 代码重复

| 对比项 | `run` (L197-316) | `run_with_context` (L318-619) |
|--------|-------------------|-------------------------------|
| ToolCallContext | 单一实例复用于所有工具 | 每个工具创建新实例 |
| 取消支持 | 无 | `CancellationToken` + `run_cancellable` |
| 流式事件 | 无 | `ToolStart`/`ToolOutput`/`ToolEnd`/`ToolApproval` |
| 审批流 emit | 仅 Interrupt | Interrupt + StreamEvent::ToolApproval |
| 结果 emit | 无 | step_progress 或 ToolEnd |
| 行数 | 120 行 | 300 行 |

重复的核心逻辑：
- 审批三态判断（None → interrupt / Some(false) → reject / Some(true) → proceed）
- `parse_tool_arguments` 调用
- `call_tool_with_context` 调用
- `normalize_tool_output` 调用 + `used_observation_chars` 累加
- `ToolResult` 构建（from normalized + with_call_id + with_name + with_is_error）
- `backfill_tool_result_call_ids` 调用
- 新状态构建（approval_result 消费逻辑）

#### 2. ToolCallContent 序列化不一致

- `run` L260：`content.to_display_string()` — 面向用户的纯文本
- `run_with_context` L509：`serde_json::to_string(&content)` — JSON 序列化（保留 Diff/Terminal 等结构信息）

同一工具、相同输出，因 stream 模式不同导致 normalize 接收不同的输入文本，可能产生不同的截断/标准化结果。

### P1：安全与正确性

#### 3. 错误模板泄露敏感参数

`DEFAULT_EXECUTION_ERROR_TEMPLATE` 直接插入完整 `args.to_string()`：

```rust
// L278, L562
.replace("{tool_kwargs}", &args.to_string())
```

当工具参数包含 API key、token、密码等敏感信息时，错误信息会完整暴露给 LLM。

#### 4. `display_limit` 取值来源不一致

```rust
// L334 — 用 default
let display_limit = NormalizationConfig::default().display_limit;
// L522, L390, L571 — 用 runtime_default
NormalizationConfig::runtime_default()
    .with_used_observation_chars(used_observation_chars)
```

`default()` 与 `runtime_default()` 可能返回不同的 `display_limit`，导致 `ToolOutput` 事件的截断阈值与 normalize 的配置不一致。

### P2：清理

#### 5. `DEFAULT_TOOL_ERROR_TEMPLATE` (L95) — 死代码

已定义但未被任何代码引用。

#### 6. 多余空行 (L101, L139)

## 重构方案

### 方案：提取 `execute_tool_calls` 核心方法

#### 设计

引入一个封装流式/取消上下文的结构体，将核心工具执行循环提取为独立方法：

```rust
struct ExecutionContext<'a> {
    run_ctx: &'a RunContext<ReActState>,
    tool_output_hints: HashMap<String, ToolOutputHint>,
    norm_config: NormalizationConfig,
    tools_mode: bool,
    base_custom_writer: ToolStreamWriter,
}

impl ActNode {
    async fn execute_tool_calls(
        &self,
        state: &ReActState,
        ctx: &mut ExecutionContext<'_>,
    ) -> Result<(Vec<ToolResult>, bool), AgentError> {
        // 统一的工具执行循环
    }
}
```

然后：

```rust
async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
    // 构造最小 RunContext（无 stream/cancel）
    let run_ctx = RunContext::new(RunnableConfig::default());
    let mut ctx = ExecutionContext::new(&run_ctx, self.load_tool_output_hints().await);
    let (tool_results, consumed) = self.execute_tool_calls(&state, &mut ctx).await?;
    Ok((Self::build_new_state(state, tool_results, consumed), Next::Continue))
}

async fn run_with_context(&self, state: ReActState, run_ctx: &RunContext<ReActState>) -> ... {
    let mut ctx = ExecutionContext::new(run_ctx, self.load_tool_output_hints().await);
    let (tool_results, consumed) = self.execute_tool_calls(&state, &mut ctx).await?;
    Ok((Self::build_new_state(state, tool_results, consumed), Next::Continue))
}
```

#### `ExecutionContext` 职责

| 职责 | 方法 |
|------|------|
| 取消检查 | `is_cancelled(&self) -> bool` |
| 发射流事件 | `emit_tool_start(&self, tc: &ToolCall)` |
| 发射流事件 | `emit_tool_end(&self, tc: &ToolCall, result: &str, is_error: bool, raw: Option<&str>)` |
| 发射审批事件 | `emit_approval_required(&self, tc: &ToolCall, args: &Value)` |
| 构建工具写入器 | `create_per_tool_writer(&self, tc: &ToolCall) -> ToolStreamWriter` |
| 构建 ToolCallContext | `build_tool_call_context(&self, state: &ReActState, writer: ToolStreamWriter) -> ToolCallContext` |
| 标准化工具输出 | `normalize_output(&self, name, args, text, is_error, used_chars) -> (NormalizedToolOutput, usize)` |

#### 优势

1. **单一真相源** — 核心逻辑只存在于 `execute_tool_calls` 中
2. **行为一致** — 序列化、ToolCallContext 构造统一
3. **易于测试** — 可以构造各种 `ExecutionContext` 测试不同场景
4. **渐进式** — 可以先提取核心循环，再逐步细化 `ExecutionContext`

### 统一 ToolCallContent 序列化

```rust
// 统一使用 serde_json 序列化（保留结构信息）
fn serialize_tool_content(content: &ToolCallContent) -> String {
    serde_json::to_string(content)
        .unwrap_or_else(|_| content.clone().into_text())
}
```

### 修复错误模板脱敏

```rust
let kwargs_preview = truncate_for_display(&args.to_string(), 200);
let error_text = DEFAULT_EXECUTION_ERROR_TEMPLATE
    .replace("{tool_name}", &tc.name)
    .replace("{tool_kwargs}", &kwargs_preview)
    .replace("{error}", &e.to_string());
```

### 统一 NormalizationConfig

```rust
// ExecutionContext 初始化时创建一次
let norm_config = NormalizationConfig::runtime_default();
let display_limit = norm_config.display_limit;
// 后续所有 normalize 调用复用同一 config
```

## 执行计划

| 阶段 | 任务 | 风险 |
|------|------|------|
| **Phase 1** | 定义 `ExecutionContext` 结构体和方法 | 低 — 纯新增 |
| **Phase 2** | 提取 `execute_tool_calls` 从 `run_with_context` | 中 — 需确保行为不变 |
| **Phase 3** | 让 `run` 委托到 `execute_tool_calls` | 中 — 需验证无 stream 场景兼容 |
| **Phase 4** | 统一序列化、修复 display_limit、脱敏 kwargs | 低 — 局部修改 |
| **Phase 5** | 删除死代码、清理空行 | 低 |
| **Phase 6** | 补充测试 | 低 |

### Phase 1：定义 ExecutionContext

```rust
use std::collections::HashMap;
use crate::state::tool_output_normalizer::{NormalizationConfig, ToolOutputHint};
use crate::stream::{StreamMode, ToolStreamWriter};
use crate::graph::RunContext;

struct ExecutionContext<'a> {
    run_ctx: &'a RunContext<ReActState>,
    tool_output_hints: HashMap<String, ToolOutputHint>,
    norm_config: NormalizationConfig,
    tools_mode: bool,
    base_custom_writer: ToolStreamWriter,
}

impl<'a> ExecutionContext<'a> {
    fn new(
        run_ctx: &'a RunContext<ReActState>,
        tool_output_hints: HashMap<String, ToolOutputHint>,
    ) -> Self {
        let tools_mode = run_ctx.stream_mode.contains(&StreamMode::Tools)
            || run_ctx.stream_mode.contains(&StreamMode::Debug);
        let norm_config = NormalizationConfig::runtime_default();
        let base_custom_writer = if run_ctx.stream_mode.contains(&StreamMode::Custom) || tools_mode {
            if let Some(tx) = &run_ctx.stream_tx {
                let tx = tx.clone();
                ToolStreamWriter::new(move |value| tx.try_send(StreamEvent::Custom(value)).is_ok())
            } else {
                ToolStreamWriter::noop()
            }
        } else {
            ToolStreamWriter::noop()
        };
        Self { run_ctx, tool_output_hints, norm_config, tools_mode, base_custom_writer }
    }

    fn is_cancelled(&self) -> bool {
        self.run_ctx.cancellation.as_ref()
            .is_some_and(|t| t.is_cancelled())
    }

    fn display_limit(&self) -> usize {
        self.norm_config.display_limit
    }
}
```

### Phase 2-3：核心循环提取

将 `run_with_context` 中的 for 循环体提取为 `execute_tool_calls`，将流式 emit 和取消检查委托给 `ExecutionContext`。`run` 构造一个无 stream/cancel 的最小 `RunContext` 后调用同一方法。

### Phase 6：补充测试

```rust
#[tokio::test]
async fn run_executes_single_tool() { ... }
#[tokio::test]
async fn run_handles_tool_error() { ... }
#[tokio::test]
async fn run_interrupts_on_approval_required() { ... }
#[tokio::test]
async fn run_rejection_adds_error_result() { ... }
#[tokio::test]
async fn run_with_context_emits_tool_start_end() { ... }
#[tokio::test]
async fn run_with_context_respects_cancellation() { ... }
#[tokio::test]
async fn run_with_context_handles_length_mismatch() { ... }
```

## 验证标准

- [ ] `run` 和 `run_with_context` 无重复的核心逻辑
- [ ] 所有现有测试通过
- [ ] `ToolCallContent` 序列化方式统一
- [ ] 错误模板中 kwargs 被截断
- [ ] `NormalizationConfig` 只创建一次
- [ ] `DEFAULT_TOOL_ERROR_TEMPLATE` 已删除
- [ ] `run` 和 `run_with_context` 有单元测试覆盖
