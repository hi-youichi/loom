# RFC: 将模型配置从 LLM Client 移入 ReActState

| 字段 | 值 |
|------|-----|
| 状态 | Draft |
| 作者 | loom team |
| 日期 | 2025-08-19 |

---

## 1. 背景与动机

### 1.1 现状

当前模型选择发生在 `ReactRunner` 构造阶段：

```
ReactRunner::new(llm: Box<dyn LlmClient>)
  └─ ThinkNode::new(Arc<dyn LlmClient>)
       └─ self.llm.invoke(&state.messages)
```

`ChatOpenAI` / `ChatOpenAICompat` 在 `create_llm_client()` 时将 `model` 写死为内部 `String` 字段。`ThinkNode`、`TitleNode`、`CompressionGraphNode` 构造时拿到 `Arc<dyn LlmClient>` 后不可变。

`ReActState` 不携带任何模型信息，无法序列化/恢复模型选择。

### 1.2 问题

1. **静态绑定** — 模型在 runner 创建时确定，运行期间无法切换。
2. **状态不完整** — checkpoint 不包含模型信息，跨进程恢复后无法保证使用同一模型。
3. **多 tier 调度不可行** — 无法根据任务复杂度在简单/强力模型之间动态切换。
4. **成本归因困难** — `usage` 字段与具体模型之间的关联需外部维护。

### 1.3 目标

1. `ReActState` 持有当前模型标识（纯数据，可序列化）。
2. ThinkNode 每次执行时根据 state 中的模型标识动态获取 LLM client。
3. 支持运行时切换模型。

4. 不引入显著的运行时开销（client 缓存）。

---

## 2. 核心设计

### 2.1 设计原则

- **State = 纯数据** — `ReActState` 仅持有 `ModelConfig`（模型名 + 参数），不持有 trait object。
- **Provider = 工厂** — 新增 `LlmProvider` trait，持有连接配置（base_url / api_key），根据模型名动态创建 client。
- **Node = 消费者** — ThinkNode 持有 `LlmProvider`，每次执行从 state 读取模型名，通过 provider 获取 client。

### 2.2 新增 `ModelConfig`

`ModelConfig` 支持两种指定模型的方式：**精确模型**（`model_id`）和 **Tier 抽象**（`tier`）。
两者互斥：`model_id` 非空时直接使用；否则由 `LlmProvider` 根据 `tier` 解析出具体模型。
`tier` 为 `ModelTier::None` 时使用 provider 默认模型。

```rust
// loom/src/state/react_state.rs

/// 当前轮次使用的模型配置。纯数据，可序列化。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelConfig {
    /// 精确模型标识，如 "openai/gpt-4o"。非空时直接使用，忽略 tier。
    pub model_id: String,
    /// Tier 抽象（None / Light / Standard / Strong）。当 model_id 为空时，
    /// 由 LlmProvider 根据 tier 从 provider 的模型列表中解析出具体模型。
    #[serde(default)]
    pub tier: ModelTier,
    /// 可选的 temperature 覆盖。
    pub temperature: Option<f32>,
    /// 可选的 tool_choice 覆盖。
    pub tool_choice: Option<crate::llm::ToolChoiceMode>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            tier: ModelTier::None,
            temperature: None,
            tool_choice: None,
        }
    }
}
```

`ModelTier` 在 `model-spec-core` 中新增 `None` 变体：

```rust
// model-spec-core/src/tier.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTier {
    None,       // ← 新增：未指定 tier
    Light,
    Standard,
    Strong,
}
```

**优先级规则**：

| model_id | tier | 行为 |
|----------|------|------|
| 非空 | 任意 | 直接使用 model_id |
| 空 | Light / Standard / Strong | 由 provider 解析 tier 得到具体模型 |
| 空 | None | 使用 provider 默认模型 |

### 2.3 `ReActState` 新增字段

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReActState {
    /// 当前轮次使用的模型配置。
    #[serde(default)]
    pub model_config: ModelConfig,

    // --- 以下字段不变 ---
    pub messages: Vec<Message>,
    pub last_reasoning_content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub turn_count: u32,
    pub approval_result: Option<bool>,
    pub usage: Option<LlmUsage>,
    pub total_usage: Option<LlmUsage>,
    pub message_count_after_last_think: Option<usize>,
    pub think_count: u32,
    pub summary: Option<String>,
    pub should_continue: bool,
}
```

`#[serde(default)]` 保证反序列化旧 checkpoint 时 `model_config` 为默认值（`model_id` 为空、`tier` 为 `None`），使用 provider 默认模型。

### 2.4 新增 `LlmProvider` trait

```rust
// loom/src/llm/mod.rs

/// 模型无关的 provider 连接池：持有 base_url / api_key / provider_type，
/// 可按不同 model name 动态创建 LlmClient 实例，也支持从 tier 解析具体模型。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 用给定的 model name 创建一个 LLM client。
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError>;

    /// 当前 provider 的默认模型标识。
    fn default_model(&self) -> &str;

    /// Provider 名称（如 "openai", "bigmodel"）。
    fn provider_name(&self) -> &str;

    /// 根据 tier（None / Light / Standard / Strong）从 provider 的模型列表中解析出具体模型名。
    /// `ModelTier::None` 时应返回 default_model()。
    /// 返回解析后的 model_id 字符串。
    async fn resolve_tier(&self, tier: ModelTier) -> Result<String, AgentError>;
}
```

### 2.5 Provider 实现

#### OpenAI Provider

```rust
// loom/src/llm/openai_provider.rs

pub struct OpenAIProvider {
    config: OpenAIConfig,
    provider_type: String,
    default_model: String,
    provider_name: String,
    providers: Vec<ProviderConfig>,
}

impl OpenAIProvider {
    pub fn from_entry(entry: &ModelEntry, providers: Vec<ProviderConfig>) -> Self {
        let mut config = OpenAIConfig::new();
        if let Some(ref api_key) = entry.api_key {
            config = config.with_api_key(api_key);
        }
        if let Some(ref base_url) = entry.base_url {
            config = config.with_api_base(base_url.trim_end_matches('/'));
        }
        Self {
            config,
            provider_type: entry.provider_type.clone().unwrap_or_default(),
            default_model: entry.name.clone(),
            provider_name: entry.provider.clone(),
            providers,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError> {
        let client = ChatOpenAI::with_config(self.config.clone(), model);
        Ok(Box::new(client))
    }

    fn default_model(&self) -> &str { &self.default_model }
    fn provider_name(&self) -> &str { &self.provider_name }

    async fn resolve_tier(&self, tier: ModelTier) -> Result<String, AgentError> {
        if tier == ModelTier::None {
            return Ok(self.default_model().to_string());
        }
        let entry = ModelRegistry::global()
            .resolve_tier_intelligent(&self.provider_name, tier, &self.providers)
            .await
            .ok_or_else(|| AgentError::ExecutionFailed(
                format!("no model found for tier {:?} on provider '{}'", tier, self.provider_name)
            ))?;
        Ok(entry.id)
    }
}
```

#### OpenAI Compatible Provider

```rust
// loom/src/llm/openai_compat_provider.rs

pub struct OpenAICompatProvider {
    base_url: String,
    api_key: String,
    provider_type: String,
    default_model: String,
    provider_name: String,
    providers: Vec<ProviderConfig>,
}

#[async_trait]
impl LlmProvider for OpenAICompatProvider {
    fn create_client(&self, model: &str) -> Result<Box<dyn LlmClient>, AgentError> {
        let client = ChatOpenAICompat::with_config(
            self.base_url.clone(),
            self.api_key.clone(),
            model,
        );
        Ok(Box::new(client))
    }

    fn default_model(&self) -> &str { &self.default_model }
    fn provider_name(&self) -> &str { &self.provider_name }

    async fn resolve_tier(&self, tier: ModelTier) -> Result<String, AgentError> {
        if tier == ModelTier::None {
            return Ok(self.default_model().to_string());
        }
        let entry = ModelRegistry::global()
            .resolve_tier_intelligent(&self.provider_name, tier, &self.providers)
            .await
            .ok_or_else(|| AgentError::ExecutionFailed(
                format!("no model found for tier {:?} on provider '{}'", tier, self.provider_name)
            ))?;
        Ok(entry.id)
    }
}
```

#### 通用构造函数

```rust
// loom/src/llm/mod.rs

/// 从 ModelEntry 创建对应的 LlmProvider。
/// providers 参数用于 tier 解析（从 ModelRegistry 解析模型列表）。
pub fn create_llm_provider(
    entry: &ModelEntry,
    providers: Vec<ProviderConfig>,
) -> Result<Arc<dyn LlmProvider>, AgentError> {
    let provider_type = entry.provider_type.as_deref().unwrap_or_else(|| {
        if entry.provider.eq_ignore_ascii_case("openai") { "openai" } else { "openai_compat" }
    });

    match provider_type {
        "openai" => {
            let provider = OpenAIProvider::from_entry(entry, providers);
            Ok(Arc::new(provider))
        }
        _ => {
            let api_key = entry.api_key.clone().ok_or_else(|| { /* ... */ })?;
            let base_url = entry.base_url.clone().ok_or_else(|| { /* ... */ })?;
            let provider = OpenAICompatProvider {
                base_url, api_key,
                provider_type: provider_type.to_string(),
                default_model: entry.name.clone(),
                provider_name: entry.provider.clone(),
                providers,
            };
            Ok(Arc::new(provider))
        }
    }
}
```

### 2.6 ThinkNode 改造

```rust
// loom/src/agent/react/think_node.rs

pub struct ThinkNode {
    provider: Arc<dyn LlmProvider>,
    /// client 缓存：model_id → Arc<dyn LlmClient>，避免重复创建。
    client_cache: Arc<RwLock<HashMap<String, Arc<dyn LlmClient>>>>,
}

impl ThinkNode {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            client_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn resolve_client(&self, model_config: &ModelConfig) -> Result<Arc<dyn LlmClient>, AgentError> {
        // 优先级：model_id > tier 解析 > provider 默认
        let model = if !model_config.model_id.is_empty() {
            model_config.model_id.clone()
        } else if model_config.tier != ModelTier::None {
            self.provider.resolve_tier(model_config.tier).await?
        } else {
            self.provider.default_model().to_string()
        };

        // 读缓存
        {
            let cache = self.client_cache.read().await;
            if let Some(client) = cache.get(&model) {
                return Ok(Arc::clone(client));
            }
        }

        // 创建并缓存
        let mut client = self.provider.create_client(&model)?;
        // 应用 state 级别的 temperature / tool_choice 覆盖
        // （需要 client 支持 with_temperature / with_tool_choice builder 方法）
        let client = Arc::from(client);
        {
            let mut cache = self.client_cache.write().await;
            cache.entry(model).or_insert_with(|| Arc::clone(&client));
        }
        Ok(client)
    }
}
```

**Node::run**：

```rust
#[async_trait]
impl Node<ReActState> for ThinkNode {
    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        // resolve_client 内部按 model_id > tier > default 优先级解析
        let llm = self.resolve_client(&state.model_config).await?;
        let response = llm.invoke(&state.messages).await?;
        let new_state = state.apply_think(
            response.content,
            response.reasoning_content,
            response.tool_calls,
            response.usage,
        );
        Ok((new_state, Next::Continue))
    }
}
```

`run_with_context` 的改动模式相同，在调用 `invoke_think_llm` 之前先 resolve client。

### 2.7 ReactRunner 构造函数变化

```rust
impl ReactRunner {
    /// 新构造函数：接收 LlmProvider。
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tool_source: Box<dyn ToolSource>,
        checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
        store: Option<Arc<dyn Store>>,
        runnable_config: Option<RunnableConfig>,
        system_prompt: String,
        title_llm: Option<Arc<dyn LlmProvider>>,    // ← 改为 provider
        compaction_config: Option<CompactionConfig>,
        user_message_store: Option<Arc<dyn UserMessageStore>>,
        model_config: Option<ModelConfig>,           // ← 新增：默认模型配置
        verbose: bool,
        cancellation: Option<RunCancellation>,
    ) -> Result<Self, CompilationError> {
        let think = ThinkNode::new(Arc::clone(&provider));
        let title_node = TitleNode::new(title_llm.unwrap_or_else(|| Arc::clone(&provider)));
        let compression_graph = build_graph(compaction_config, Arc::clone(&provider), None)?;
        // ...
    }
}
```

### 2.8 AgentOptions 更新

```rust
pub struct AgentOptions {
    /// LLM provider。替代原有的 llm 字段。
    pub provider: Option<Arc<dyn LlmProvider>>,
    /// 默认模型配置。
    pub model_config: Option<ModelConfig>,
    // ... 其他字段不变
}
```

---

## 3. 影响范围

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `llm/mod.rs` | 新增 trait | `LlmProvider` |
| `llm/openai_provider.rs` | 新增文件 | `OpenAIProvider` |
| `llm/openai_compat_provider.rs` | 新增文件 | `OpenAICompatProvider` |
| `llm/model_registry.rs` | 修改 | 新增 `create_llm_provider()` |
| `state/react_state.rs` | 修改 | 新增 `ModelConfig` + `ReActState.model_config` |
| `agent/react/think_node.rs` | 修改 | `Arc<dyn LlmClient>` → `Arc<dyn LlmProvider>` + cache |
| `agent/react/title_node.rs` | 修改 | 同上 |
| `compress/mod.rs` | 修改 | `CompressionGraphNode` 适配 |
| `agent/react/runner/runner.rs` | 修改 | `ReactRunner::new` 参数变更 |
| `agent/react/runner/options.rs` | 修改 | `AgentOptions.llm` → `AgentOptions.provider` |
| `agent/react/build/llm.rs` | 修改 | 构建 `LlmProvider` 替代 `LlmClient` |
| `agent/got/execute_engine.rs` | 修改 | GoT 适配 |
| `agent/dup/adapter_nodes.rs` | 修改 | DUP 适配 |
| `cli_run/agent.rs` | 修改 | CLI 构建 runner 使用新 API |
| 测试文件（约 15 个） | 修改 | `ThinkNode::new(llm)` → `ThinkNode::new(provider)` |

---

## 4. 迁移计划

### Phase 1 — 基础设施

1. 新增 `LlmProvider` trait 和 `OpenAIProvider`、`OpenAICompatProvider` 实现。
2. 新增 `ModelConfig` 结构体。
3. `ReActState` 增加 `model_config` 字段（`#[serde(default)]`）。
4. 新增 `create_llm_provider()` 函数。
5. 删除 `create_llm_client()` 函数。
6. 单元测试覆盖新增类型。

**验收标准**：`cargo clippy -- -D warnings` 和 `cargo test` 通过。

### Phase 2 — Node 层改造

1. `ThinkNode` 改为持有 `Arc<dyn LlmProvider>` + `RwLock<HashMap>` client cache。
2. `TitleNode` 同步改造。
3. `CompressionGraphNode` 同步改造。
4. 新增测试：验证不同 `model_config` 能路由到不同 client。

**验收标准**：`cargo clippy -- -D warnings` 和 `cargo test` 通过。

### Phase 3 — Runner / Options API

1. `ReactRunner::new` 参数从 `Box<dyn LlmClient>` 改为 `Arc<dyn LlmProvider>`。
2. `AgentOptions.llm` → `AgentOptions.provider` + `AgentOptions.model_config`。
3. 更新 `resolve_run_agent_options`。
4. 更新 `build/llm.rs` 构建 provider。

**验收标准**：`cargo clippy -- -D warnings` 和 `cargo test` 通过。

### Phase 4 — 上层全量迁移

1. 更新 `cli_run/agent.rs`。
2. 更新 `agent/got/execute_engine.rs`。
3. 更新 `agent/dup/adapter_nodes.rs`。
4. 更新所有 example 和 test 文件。
5. 删除所有旧 API（`create_llm_client`、`Box<dyn LlmClient>` 参数等）。

**验收标准**：全量 `cargo clippy -- -D warnings` 和 `cargo test` 通过。

---

## 5. 为什么不直接在 State 里放 `Arc<dyn LlmClient>`

| 方案 | 可序列化 | 可切换模型 | 支持 Tier | 职责清晰 | 推荐度 |
|------|---------|-----------|----------|---------|--------|
| State 放 `Arc<dyn LlmClient>` | ✗ | ✗ | ✗ | ✗ | ✗ |
| State 放 `ModelConfig`（含 tier），Node 持有 `LlmProvider` | ✓ | ✓ | ✓ | ✓ | ✓ |

- `ReActState` 需要 `Serialize + Deserialize`，`dyn LlmClient` 不可序列化。
- State 应是纯数据，不应持有行为（trait object）。
- Checkpoint 恢复时无法序列化 client，但可以序列化 `ModelConfig`（含 tier）再由 provider 重建。
- Tier 是抽象描述（Light/Standard/Strong），由 provider 在运行时解析为具体模型，适合放在 state 中作为意图声明。

---

## 6. 改造后解锁的能力

1. **动态模型切换** — 自定义节点修改 `state.model_config.model_id`，下轮 Think 自动使用新模型。
2. **Tier 感知调度** — 节点根据任务复杂度设置 `state.model_config.tier = ModelTier::Light`，provider 自动解析最优模型。
3. **子 Agent 继承** — `invoke_agent` 工具的 explore profile 设置 `tier: Light`，子 runner 的 `ModelConfig` 自动携带 tier，无需在构建时硬编码模型。
4. **A/B 测试** — 同一 session 不同轮次使用不同模型或 tier。
5. **成本归因** — `ModelConfig.model_id` + `usage` 精确统计每模型开销。
6. **Session 恢复** — checkpoint 包含 `ModelConfig`（含 tier），跨进程恢复后可重新解析 tier 到具体模型。
7. **多 provider 路由** — 后续可扩展 `LlmProvider` 支持多 provider 映射，tier 在多 provider 间选最优。

---

## 7. 风险与缓解

| 风险 | 缓解 |
|------|------|
| client 缓存增加内存占用 | 设置缓存上限，LRU 淘汰 |
| `create_client` 每次创建新实例的开销 | 缓存命中时零开销 |
| `ModelConfig.temperature` 需要运行时覆盖 client 配置 | client 支持 `with_temperature` builder |
| 并发读写 `client_cache` | `RwLock` 保护读多写少场景 |
| tier 解析需要异步网络调用（查询 ModelRegistry） | `resolve_tier` 结果也纳入 client 缓存，避免重复解析 |
| tier 解析失败（provider 无匹配模型） | 降级到 `default_model()`，日志 warn |
