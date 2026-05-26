# LlmFactory 与 Background Review 使用 Strong Tier 方案

## 问题

Background review 当前直接使用 CLI 的 `base_url`、`api_key`、`model`，未复用 agent 已有的 tier 模型解析系统，导致：
- `base_url` 为空时报 "builder error"
- review 使用与 agent 相同的模型，而非用更强的模型做审查

## 目标

1. 抽象公共 `LlmFactory`，统一所有 LLM client 创建
2. Background review 固定使用 Strong tier

---

## 1. 配置系统梳理

项目有两套独立的配置，需要区分清楚：

### Config 文件（TOML）

定义 **provider 的连接信息**，由 `env_config` 加载，路径由用户配置：

```toml
[[providers]]
name = "zhipuai"
base_url = "https://open.bigmodel.cn/api/paas/v4"
api_key = "xxx"
provider_type = "openai_compat"

[[providers]]
name = "openai"
base_url = "https://api.openai.com/v1"
api_key = "sk-xxx"
provider_type = "openai"

[[providers]]
name = "deepseek"
base_url = "https://api.deepseek.com"
api_key = "sk-xxx"
provider_type = "openai_compat"
```

→ 加载为 `Vec<ProviderConfig>`，仅含连接参数（base_url, api_key, provider_type）

### Agent Profile（YAML）

定义 **agent 使用哪个模型系列及其变体**：

```yaml
# agents/dev/config.yaml
name: dev
model:
  provider: zhipuai     # 引用 config 文件中的 provider.name
  family: glm           # 模型系列
  version: "5"          # 代际版本
  tier: standard        # 代际内的变体

# agents/explore/config.yaml
name: explore
model:
  provider: zhipuai
  family: glm
  version: "5"
  tier: light
```

→ profile.model.tier 决定 agent 默认使用 Light / Standard / Strong 中的哪一个

### Tier Plans（编译时内置）

定义 **provider + family + version → {tier → model_name}** 的映射：

```toml
# loom/src/tier/plans.toml

[[plan]]
provider_id = "zhipuai"
family = "glm"
version = "5"
[plan.tiers]
strong   = "glm-5.1"
standard = "glm-4.7"
light    = "glm-4.5-air"

[[plan]]
provider_id = "zhipuai"
family = "glm"
version = "4"
[plan.tiers]
strong   = "glm-4-52b"
standard = "glm-4-9b"
light    = "glm-4-air"

[[plan]]
provider_id = "deepseek"
family = "deepseek"
version = "latest"
[plan.tiers]
strong   = "deepseek-r1"
standard = "deepseek-chat"
light    = "deepseek-lite"
```

**注意**：`provider_id` 字段名对应 config 文件中的 `provider.name` 和 profile 中的 `model.provider`，三者必须是同一个值（如 `"zhipuai"`）。

### 三级配置关系

```
Config 文件 (provider连接)
  provider.name = "zhipuai" ───── base_url, api_key, provider_type
         │
         │ 通过 model.provider 关联
         │
Agent Profile (模型选型)
  model.provider = "zhipuai" ───── 引用 provider 连接
  model.family   = "glm"     ───── 定位 tier plan
  model.version  = "5"       ───── 定位 tier plan
  model.tier     = "standard" ──── 选择最终模型
         │
         │ 通过 (provider_id, family, version) 查找
         │
Tier Plans (模型映射)
  {provider_id: "zhipuai", family: "glm", version: "5"}
    └─ tiers: {strong: "glm-5.1", standard: "glm-4.7", light: "glm-4.5-air"}
```

---

## 2. 向后兼容

### 场景 A：旧 profile（只有 tier，无 family/version）

```yaml
# 旧格式
model:
  tier: standard
```

处理方式：
- `family` 和 `version` 为空 → `ModelEntry.family = None, version = None`
- `resolve_tier_from_entry` → family/version 为 None → 无法定位 plan → 返回 None
- review 自动 fallback 到原逻辑（使用 `config.model`），行为不变

### 场景 B：旧 plans.toml（只有 provider_id + tiers）

```toml
# 旧格式
[[plan]]
provider_id = "zhipuai-coding-plan"
[plan.tiers]
strong   = "glm-5.1"
standard = "glm-4.7"
light    = "glm-4.5-air"
```

处理方式：
- 旧 plan 缺少 `family` 和 `version` 字段
- `resolve_tier(provider, family, version, tier)` 只按 `(provider_id, family, version)` 精确匹配
- 旧 plan 不会被新接口匹配到，但现有的 `resolve_from_plan` 不受影响
- 逐步将旧 plan 迁移到新格式（补充 family/version 字段）

---

## 3. LlmFactory 设计方案

### 位置

`loom/src/llm/factory.rs`

### 接口

```rust
pub struct LlmFactory {
    providers: Vec<ProviderConfig>,
}

impl LlmFactory {
    /// 从 config 文件加载 providers
    pub fn load() -> Option<Self>;

    /// 解析指定 provider + family + version + tier → 完整模型配置
    ///
    /// # 查找流程
    /// 1. 在 plans.toml 中匹配 (provider, family, version) 的 tier plan
    /// 2. 从 plan.tiers[tier] 拿模型名
    /// 3. 在 providers 中查找 provider，获取 base_url/api_key
    /// 4. 组装 ModelEntry (模型名 + 连接信息 + family/version)
    pub async fn resolve_tier(
        &self,
        provider: &str,
        family: &str,
        version: &str,
        tier: ModelTier,
    ) -> Option<ModelEntry>;

    /// 从 session 的 ModelEntry 换一个 tier 重新解析（复用连接信息）
    pub async fn resolve_tier_from_entry(
        &self,
        entry: &ModelEntry,
        tier: ModelTier,
    ) -> Option<ModelEntry>;
}
```

### resolve_tier 内部实现

```rust
pub async fn resolve_tier(
    &self,
    provider: &str,
    family: &str,
    version: &str,
    tier: ModelTier,
) -> Option<ModelEntry> {
    // 1. 从 tier plans 找到匹配的 (provider, family, version)
    let plans = tier_plans();
    let plan = plans.values().find(|p| {
        p.provider_id == provider
            && p.family.as_deref() == Some(family)
            && p.version.as_deref() == Some(version)
    })?;

    // 2. 获取 tier → model_name 映射
    let model_name = plan.tiers.get(&tier)?;

    // 3. 从 providers 获取连接配置
    let provider_cfg = self.providers.iter().find(|p| p.name == provider)?;

    // 4. 组装 ModelEntry，注入 family 和 version
    let mut entry = ModelEntry::from_provider_config(provider_cfg, model_name);
    entry.family = Some(family.to_string());
    entry.version = Some(version.to_string());
    Some(entry)
}
```

---

## 4. ModelEntry 扩展

```rust
pub struct ModelEntry {
    pub id: String,              // "zhipuai/glm-4.7"
    pub name: String,            // "glm-4.7"
    pub provider: String,        // "zhipuai"
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub family: Option<String>,  // ← 新增
    pub version: Option<String>, // ← 新增
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub tool_choice: Option<ToolChoiceMode>,
}
```

---

## 5. 数据流：Session → Review

```
session 启动
  │
  ├─ profile.model: {provider: "zhipuai", family: "glm", version: "5", tier: standard}
  ├─ factory.resolve_tier("zhipuai", "glm", "5", Standard)
  │    └─ 返回 ModelEntry {
  │         name: "glm-4.7",
  │         base_url: Some("https://open.bigmodel.cn/api/paas/v4"),
  │         api_key: Some("xxx"),
  │         family: Some("glm"),
  │         version: Some("5"),
  │       }
  │
session 结束 (EndTurn)
  │
  ├─ 将 session 的 ModelEntry 传入 BackgroundReviewConfig.session_model
  ├─ review: factory.resolve_tier_from_entry(session_entry, Strong)
  │    └─ 复用 entry.provider → 找到 base_url/api_key
  │    └─ 复用 entry.family + entry.version → 找到 plan
  │    └─ plan.tiers[Strong] → "glm-5.1"
  │    └─ 返回 ModelEntry {name: "glm-5.1", ...}
  │
  └─ review 用返回的 ModelEntry 构建 LLM client
```

---

## 6. BackgroundReviewConfig 改动

```rust
pub struct BackgroundReviewConfig {
    pub enabled: bool,
    /// session 已解析的模型配置（含 provider, family, version, base_url, api_key）
    pub session_model: Option<ModelEntry>,
    /// fallback — 原字段保留
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    ..Default::default()
}
```

### build_background_config_from_opts

```rust
pub fn build_background_config_from_opts(
    opts: &loom::RunOptions,
    session_model: Option<ModelEntry>,  // ← 从 session 传入
) -> BackgroundReviewConfig {
    BackgroundReviewConfig {
        enabled: true,
        session_model,
        base_url: opts.base_url.clone().unwrap_or_default(),
        api_key: opts.api_key.clone().unwrap_or_default(),
        model: opts.model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string()),
        ..Default::default()
    }
}
```

---

## 7. run_background_review_workflow

```rust
async fn run_background_review_workflow(config, session_content, session_id) {
    // enabled/len check ...

    // 优先使用 session 模型配置解析 Strong tier
    if let Some(ref session_entry) = config.session_model {
        if let Some(factory) = LlmFactory::load() {
            if let Some(strong_entry) = factory
                .resolve_tier_from_entry(session_entry, ModelTier::Strong)
                .await
            {
                // 使用 create_llm_client（根据 provider_type 自动选择 ChatOpenAI / ChatOpenAICompat）
                let llm = create_llm_client(&strong_entry, None)
                    .map_err(|e| e.to_string())?
                    .with_tools(review_tool_specs());
                // ... 用 llm 做 review ...
                return Ok(...);
            }
        }
    }

    // fallback: 原逻辑（无 session_model 或 tier 解析失败时）
    let llm = build_review_agent_client(&config.base_url, &config.api_key, &config.model);
    // ...
}
```

**改进点**：不再直接 hardcode `ChatOpenAICompat::with_config`，使用 `create_llm_client(&strong_entry, None)` 根据 provider_type 自动选择正确的客户端类型。

---

## 8. 实施步骤

### Step 1：扩展 TierPlan 数据结构

`loom/src/tier/plan.rs`：

```rust
pub struct TierPlan {
    pub provider_id: String,
    pub family: Option<String>,   // ← 新增
    pub version: Option<String>,  // ← 新增
    pub tiers: HashMap<ModelTier, String>,
}

struct TierPlanRaw {
    provider_id: String,
    family: Option<String>,   // ← 新增
    version: Option<String>,  // ← 新增
    tiers: HashMap<ModelTier, String>,
}
```

修改 `plans.toml`，为现有 plan 补充 family/version（现有一个 `zhipuai-coding-plan` 改为新格式）。

### Step 2：扩展 ModelEntry

添加 `family: Option<String>` 和 `version: Option<String>` 字段及 `Default::default()`。

### Step 3：创建 LlmFactory

`loom/src/llm/factory.rs`，实现 `load`、`resolve_tier`、`resolve_tier_from_entry`。

### Step 4：改造 BackgroundReviewConfig

加 `session_model: Option<ModelEntry>`，修改 `build_background_config_from_opts` 签名。

### Step 5：改造 run_agent → background review 调用点

`cli/src/run/agent.rs:337`，传入 session 的 `ModelEntry`。

### Step 6：改造 run_background_review_workflow

先尝试 factory resolve，失败则 fallback。用 `create_llm_client` 替代硬编码 `ChatOpenAICompat`。

### Step 7（可选）：迁移 title_generator

改用 `LlmFactory`。

---

## 9. 边界情况

| 场景 | 行为 |
|------|------|
| 无 config 文件 | factory.load() = None，fallback |
| 有 config，tier 解析成功 | 使用 Strong 模型 |
| 有 config，但 plan 不匹配 (family/version 对应不上) | fallback |
| session_model.family / version 为 None（旧 profile） | resolve_tier_from_entry 返回 None，fallback |
| session_model 为 None | 直接走 fallback |
| provider_type = "openai" (非 compat) | create_llm_client 自动创建 ChatOpenAI |
