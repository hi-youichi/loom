# LLM Tool 开发方案

## 开发步骤总览

1. 新增 `LlmCallTool`（`loom/src/tools/llm_call.rs`）
2. 新增 `LlmToolsSource`（`loom/src/tool_source/llm_tool_source.rs`）
3. 扩展 `ToolCallContext`（`loom/src/tool_source/context.rs`）
4. 扩展 `ReactBuildConfig`（`loom/src/agent/react/config.rs`）
5. 注册到 `build_tool_source`（`loom/src/agent/react/build/tool_source.rs`）
6. 编写单元测试
7. 编写集成测试

---

## 步骤 1: 新增 `LlmCallTool`

**文件**: `loom/src/tools/llm_call.rs`（新建）
**模块注册**: `loom/src/tools/mod.rs` 添加 `pub mod llm_call;`

### 结构体

```rust
pub struct LlmCallTool {
    provider: Arc<dyn LlmProvider>,
    default_max_tokens: u32,
    max_depth: u32,
}
```

### Tool trait 实现

**`name()`**: 返回 `"llm_call"`

**`spec()`**:
```json
{
  "type": "object",
  "properties": {
    "prompt": { "type": "string", "description": "The content to send to the LLM" },
    "system_prompt": { "type": "string", "description": "Optional system prompt" },
    "model": { "type": "string", "description": "Optional model name, defaults to provider's default model" },
    "temperature": { "type": "number", "description": "Sampling temperature 0.0-2.0, default 0.0" },
    "max_tokens": { "type": "integer", "description": "Max tokens to generate, defaults to config default_max_tokens" },
    "response_format": { "type": "string", "enum": ["text", "json"], "description": "Response format, default text" }
  },
  "required": ["prompt"]
}
```

**`call()`** 核心逻辑:
1. 从 `args` 提取参数，`prompt` 必填
2. 从 `ctx` 读取 `depth`，检查 `depth >= self.max_depth` 则返回错误文本
3. 构造 `Message` 列表:
   - 如果有 `system_prompt`，添加 `Message::system(system_prompt)`
   - 添加 `Message::user(prompt)`
4. 解析 model：为空则用 `self.provider.default_model()`
5. 调用 `self.provider.create_client(model)?.invoke(messages)`
6. 从 response 提取 `usage`，通过 `ctx.usage_callback` 回写
7. 返回 `ToolCallContent::Text(response.content)`

**错误处理**:
- `AgentError` → 返回 `ToolCallContent::Text(format!("LLM call failed: {}", e))`，不抛 `ToolSourceError`
- `prompt` 缺失 → `ToolSourceError::InvalidInput`

---

## 步骤 2: 新增 `LlmToolsSource`

**文件**: `loom/src/tool_source/llm_tool_source.rs`（新建）
**模式**: 与 `BashToolsSource` / `WebToolsSource` 一致

### 结构

```rust
pub struct LlmToolsSource {
    _source: AggregateToolSource,
}
```

### 构造方法

```rust
impl LlmToolsSource {
    pub async fn new(
        provider: Arc<dyn LlmProvider>,
        config: LlmToolConfig,
    ) -> AggregateToolSource {
        let source = AggregateToolSource::new();
        source.register_async(Box::new(
            LlmCallTool::new(provider, config.default_max_tokens, config.max_depth)
        )).await;
        source
    }
}
```

同时实现 `ToolSource` trait（委托给内部 `AggregateToolSource`），与 `WebToolsSource` 模式完全一致。

### 配置结构

```rust
pub struct LlmToolConfig {
    pub enabled: bool,
    pub max_depth: u32,
    pub default_max_tokens: u32,
}

impl Default for LlmToolConfig {
    fn default() -> Self {
        Self { enabled: false, max_depth: 2, default_max_tokens: 1024 }
    }
}
```

---

## 步骤 3: 扩展 `ToolCallContext`

**文件**: `loom/src/tool_source/context.rs`

新增字段:

```rust
pub struct ToolCallContext {
    // ... 现有字段 ...

    /// Optional callback to report LLM usage from tools like `llm_call`.
    /// ActNode injects this; the tool calls it after each LLM invocation.
    pub usage_callback: Option<Arc<dyn Fn(crate::llm::LlmUsage) + Send + Sync>>,
}
```

需要更新的位置:
- `ToolCallContext::new()` — 添加 `usage_callback: None`
- `ToolCallContext::with_stream_writer()` — 添加 `usage_callback: None`
- `Debug` impl — 添加 `.field("usage_callback", &self.usage_callback.as_ref().map(|_| "..."))`

---

## 步骤 4: 扩展 `ReactBuildConfig`

**文件**: `loom/src/agent/react/config.rs`

### 新增字段

```rust
pub struct ReactBuildConfig {
    // ... 现有字段 ...

    /// Configuration for the built-in `llm_call` tool.
    pub llm_tool_config: Option<crate::tool_source::LlmToolConfig>,
}
```

### `from_env()` 读取环境变量

```rust
llm_tool_config: if std::env::var("LOOM_LLM_TOOL_ENABLED")
    .unwrap_or_default()
    .parse::<bool>()
    .unwrap_or(false)
{
    Some(LlmToolConfig {
        enabled: true,
        max_depth: std::env::var("LOOM_LLM_TOOL_MAX_DEPTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2),
        default_max_tokens: std::env::var("LOOM_LLM_TOOL_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024),
    })
} else {
    None
},
```

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `LOOM_LLM_TOOL_ENABLED` | `false` | 是否启用 llm_call 工具 |
| `LOOM_LLM_TOOL_MAX_DEPTH` | `2` | 最大嵌套深度 |
| `LOOM_LLM_TOOL_MAX_TOKENS` | `1024` | 默认 max_tokens |

### `Debug` impl

添加 `.field("llm_tool_config", &self.llm_tool_config)`

---

## 步骤 5: 注册到 `build_tool_source`

**文件**: `loom/src/agent/react/build/tool_source.rs`

在 `build_tool_source()` 函数中，**在注册 `InvokeAgentTool` 之前**，添加:

```rust
if let Some(ref llm_cfg) = config.llm_tool_config {
    if llm_cfg.enabled {
        // 复用当前 provider，需要从 build 上下文传入
        let provider = build_default_provider(config, /* tool_source ref */).await?;
        let llm_tools = LlmToolsSource::new(Arc::from(provider), llm_cfg.clone()).await;
        // 将 llm_tools 中的工具注册到 aggregate
        aggregate.register_async(Box::new(
            LlmCallTool::new(Arc::from(provider), llm_cfg.default_max_tokens, llm_cfg.max_depth)
        )).await;
    }
}
```

**问题**: `build_tool_source` 函数中 provider 尚未构建（provider 在 `build_react_run_context` 中构建）。
需要考虑两种方案:

### 方案 A: 延迟到 ActNode 注入（推荐）

不在 `build_tool_source` 中注册，而是在 `build_react_run_context` 构建 provider 之后，
通过 `extra_tools` 机制注入 `LlmCallTool`:

```rust
// 在 build_react_run_context 中，build_tool_source 之后
if let Some(ref llm_cfg) = config.llm_tool_config {
    if llm_cfg.enabled {
        let provider = /* 已构建的 provider */;
        let llm_tool = Arc::new(LlmCallTool::new(provider, llm_cfg.default_max_tokens, llm_cfg.max_depth));
        config.extra_tools = Some(Arc::new(vec![llm_tool]));
    }
}
```

这与现有的 `extra_tools` 注入路径一致（`tool_source.rs:436-440`），无需修改 `build_tool_source` 函数。

### 方案 B: 修改 `build_tool_source` 签名

传入 provider 参数。但会改变现有接口，侵入性大。

**选择方案 A**。

---

## 步骤 6: 单元测试

### `loom/src/tools/llm_call.rs` 测试

```rust
#[cfg(test)]
mod tests {
    // 1. test_spec_has_correct_name_and_schema — 验证 tool name 和 input_schema
    // 2. test_call_requires_prompt — 缺少 prompt 返回 InvalidInput
    // 3. test_call_returns_text_content — mock provider 返回文本
    // 4. test_call_depth_exceeded_returns_error_text — depth >= max_depth 返回错误文本
    // 5. test_call_uses_default_model_when_empty — model 为空时用 default
    // 6. test_call_reports_usage_via_callback — 验证 usage_callback 被调用
    // 7. test_call_failure_returns_error_text — provider 报错时返回文本而非抛异常
}
```

需要使用 `MockLlm`（已有 `loom/src/llm/mock.rs`）构造 mock provider。

### `loom/src/tool_source/llm_tool_source.rs` 测试

```rust
#[cfg(test)]
mod tests {
    // 1. new_registers_llm_call_tool — 验证 list_tools 包含 llm_call
    // 2. trait_methods_delegate_to_aggregate — 同 WebToolsSource 测试模式
}
```

### `loom/src/tool_source/context.rs` 测试

```rust
// 1. test_usage_callback_default_none — 新字段默认 None
// 2. test_usage_callback_invoked — 设置 callback 后可调用
```

---

## 步骤 7: 集成测试

### 验证方式

1. 设置 `LOOM_LLM_TOOL_ENABLED=true`，启动 agent
2. agent 的 tool list 中应包含 `llm_call`
3. agent 可以调用 `llm_call` 执行子任务（如总结、分类、翻译）
4. 递归调用被正确拦截（depth 检查）
5. Token 用量汇总到总统计中

---

## 文件变更清单

| 文件 | 变更 |
|------|------|
| `loom/src/tools/llm_call.rs` | **新建** — LlmCallTool 实现 |
| `loom/src/tools/mod.rs` | 添加 `pub mod llm_call;` 和 `pub use llm_call::LlmCallTool;` |
| `loom/src/tool_source/llm_tool_source.rs` | **新建** — LlmToolsSource |
| `loom/src/tool_source/mod.rs` | 添加 `mod llm_tool_source;` 和 `pub use` |
| `loom/src/tool_source/context.rs` | 新增 `usage_callback` 字段 |
| `loom/src/agent/react/config.rs` | 新增 `llm_tool_config` 字段，`from_env()` 读取环境变量 |
| `loom/src/agent/react/build/mod.rs` | 构建后注入 LlmCallTool 到 extra_tools |

---

## 风险与注意事项

1. **循环调用**: Agent 用 `llm_call` 让子 LLM 再次触发 `llm_call`。通过 `depth` 机制防护，但需确保 `FilteredToolSource` 不影响深度传递
2. **Token 成本**: `llm_call` 的 token 消耗会叠加，需确保 usage_callback 正确累加到父 agent 的 `total_usage`
3. **Provider 共享**: `LlmCallTool` 持有 `Arc<dyn LlmProvider>`，与父 agent 共享同一个 provider 实例，共享 API key 和连接配置
4. **模型参数传递**: `LlmClient::invoke()` 接收 `&[Message]`，`temperature`/`max_tokens`/`response_format` 需要在 tool 内部设置到 client 上。需确认现有 `create_client` 是否支持 per-call 参数，如果不支持，需要在 `LlmCallTool` 内部直接构造 OpenAI 请求
