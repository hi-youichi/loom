# LLM Tool 设计文档

## 概述

新增 `LlmToolSource`，为 Agent 提供一个 `llm_call` 工具，允许 Agent 在执行过程中调用 LLM 完成子任务。

## 设计决策

| 决策点 | 结论 |
|--------|------|
| 工具粒度 | 单工具 `llm_call` |
| 模型选择 | Agent 自行通过 `model` 参数指定 |
| 上下文传递 | 无状态，Agent 自行将所需上下文组装到 prompt 中 |
| 递归防护 | 配置 `max_depth`，通过 `ToolCallContext` 传递当前深度 |
| Token 追踪 | 汇总到父 agent 统计中 |

## 工具 Schema

**工具名**: `llm_call`

**输入参数**:

```json
{
  "prompt": "必填，发送给 LLM 的内容",
  "system_prompt": "可选，系统提示词",
  "model": "可选，指定模型名称，默认使用 provider 的 default_model",
  "temperature": "可选，0.0-2.0，默认 0.0",
  "max_tokens": "可选，最大生成 token 数，默认使用配置中的 default_max_tokens",
  "response_format": "可选，\"text\" 或 \"json\"，默认 \"text\""
}
```

**输出**: `ToolCallContent::Text`

## 工具行为

- 内部使用非流式调用 `LlmClient::invoke()`，等待完整响应后返回
- LLM 调用失败（网络、rate limit、token 超限）返回错误文本，不中断 agent 流程，让 Agent 自行决定重试或换方案
- `model` 参数为空时使用 `LlmProvider::default_model()`，不做硬校验（支持自定义/fine-tune 模型名）

## 核心依赖

- `LlmProvider` trait（`loom/src/llm/mod.rs`）：工厂接口，`create_client(model) -> Box<dyn LlmClient>`
- `LlmClient` trait：`invoke(&[Message]) -> Result<LlmResponse>` 非流式调用已就绪
- `LlmResponse.usage: Option<LlmUsage>`：包含 `prompt_tokens`、`completion_tokens`、`total_tokens`
- `LlmUsage::accumulate()`：已有累加方法

## Token 用量追踪

`ToolCallContent` 不携带 usage，通过 `ToolCallContext` 回写：

- 在 `ToolCallContext` 中新增 `usage_callback: Option<Arc<dyn Fn(LlmUsage) + Send + Sync>>`
- `LlmToolSource` 调用 `invoke()` 后，从 `LlmResponse.usage` 中提取 usage，调用 `usage_callback` 回写
- ActNode 注入 callback，内部累加到 agent 的 `total_usage`

选择回写方案而非扩展 `ToolCallContent`，因为：
- 不侵入现有 `ToolCallContent` enum 和序列化逻辑
- 与现有 `ToolCallContext` 的注入模式一致（类似 `depth` 字段）

## 递归防护

通过配置控制最大递归深度：

```toml
[tools.llm]
enabled = true
max_depth = 2
default_max_tokens = 1024
```

- `max_depth`: `llm_call` 被嵌套调用的最大深度，默认 2
- 复用 `ToolCallContext` 现有的 `depth` 字段机制（与 `InvokeAgentTool` 一致）
- 超过 `max_depth` 时返回错误文本拒绝调用

## Model 处理

- `model` 为空：使用 `LlmProvider::default_model()`
- `model` 有值：直接传给 `create_client(model)`，不做 `ModelRegistry` 校验
- 理由：支持自定义模型名、fine-tune 模型，不限制灵活性
- 调用失败时（provider 不认识该 model）返回错误文本，由 Agent 处理

## 实现位置

- 新增文件: `loom/src/tool_source/llm_tool_source.rs`
- 模块注册: `loom/src/tool_source/mod.rs`
- `ToolCallContext` 扩展: `loom/src/tool_source/context.rs`（新增 `usage_callback` 字段）
