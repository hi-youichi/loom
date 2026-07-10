# Tier → Model 解析 API

> 输入 `ModelTier`，返回具体的模型名称。

> 返回 [README](../README.md)

---

## 概述

本文档描述 `model-tier` crate 的**反向 API**：给定一个 `ModelTier` 和 provider 信息，解析出具体的模型 ID。

现有代码中已有完整实现（`tier_resolve.rs` + `tier_plan/`），本设计将其分为两层：

| 层 | API | 依赖 | 场景 |
|----|-----|------|------|
| 核心层 | `TierPlan` + `tier_plans()` + `resolve_from_plan()` | serde, toml | 同步、纯内存、轻量 |
| 模型层 | `resolve_tier_intelligent()` + `TierResolver` trait | reqwest, tokio | 异步、网络回退、完整 |

---

## 数据流

```
ModelTier (Light/Standard/Strong)
         │
         ▼
┌─────────────────────────────────────────────────┐
│            解析链（优先级从高到低）               │
│                                                 │
│  1. TierPlan (内嵌 TOML)  ← 核心层              │
│     └─ provider/family/version → tier→model     │
│                                                 │
│  2. models.dev Spec       ← 模型层（异步）       │
│     └─ pick_best_for_tier() 按 tier 筛选        │
│                                                 │
│  3. Provider API          ← 模型层（异步）       │
│     └─ fetch_models 列表取第一个                │
│                                                 │
│  4. 全部失败 → None                              │
└─────────────────────────────────────────────────┘
         │
         ▼
ModelEntry { id, base_url, api_key, provider_type, ... }
```

---

## 核心层：TierPlan 同步解析

核心层只依赖 serde + toml，纯内存操作，适合其他项目独立使用。

### TierPlan 结构体

```rust
/// 一个 provider/family/version 的 tier → model 映射。
///
/// 来源：`tier_plan/plans.toml`（内嵌编译）。
#[derive(Debug, Clone)]
pub struct TierPlan {
    pub provider_id: String,
    pub family: Option<String>,
    pub version: Option<String>,
    pub tiers: HashMap<ModelTier, String>,
}
```

**`plans.toml` 格式**：
```toml
[[plan]]
provider_id = "zhipuai"
family = "glm"
version = "5.2"
[plan.tiers]
strong   = "glm-5.2"
standard = "glm-5.2"
light    = "glm-4.7"

[[plan]]
provider_id = "zhipuai"
family = "glm"
version = "5"
[plan.tiers]
strong   = "glm-5.1"
standard = "glm-4.7"
light    = "glm-4.5-air"
```

### tier_plans() — 加载内嵌计划

```rust
use std::sync::OnceLock;

static TIER_PLANS: OnceLock<HashMap<String, TierPlan>> = OnceLock::new();

/// 加载并缓存内嵌的 tier plans。
/// key 格式：`provider_id/family/version`
pub fn tier_plans() -> &'static HashMap<String, TierPlan> {
    TIER_PLANS.get_or_init(|| {
        let raw = include_str!("plans.toml");
        // ... parse + cache
    })
}
```

### resolve_from_plan() — 从计划解析

```rust
/// 从内嵌 tier plans 解析模型。
///
/// 匹配规则：
/// 1. provider_id 前缀匹配（`zhipuai` 匹配 `zhipuai`, `zhipuai-coding-plan`, `zhipuai_plan`）
/// 2. 多个匹配时选最高 version
/// 3. 从选中 plan 的 tiers 中查找 tier 对应的 model_id
pub fn resolve_from_plan(
    provider: &str,
    tier: ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry>
```

**provider_id 匹配逻辑**：
```rust
/// `zhipuai` 匹配 `zhipuai`, `zhipuai-coding-plan`, `zhipuai_plan`
/// 但不匹配 `zhipuaiai`（无分隔符）
fn provider_id_matches(plan_id: &str, config_name: &str) -> bool {
    if plan_id == config_name { return true; }
    let plan_lower = plan_id.to_ascii_lowercase();
    let config_lower = config_name.to_ascii_lowercase();
    config_lower.starts_with(&format!("{plan_lower}-"))
        || config_lower.starts_with(&format!("{plan_lower}_"))
}
```

**版本比较**：
```rust
/// 比较点分版本字符串："5.2" > "5.1" > "5"
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering
```

**使用示例**：
```rust
let plans = tier_plans();
// 直接查 HashMap
let key = "zhipuai/glm/5.2";
if let Some(plan) = plans.get(key) {
    let model_id = plan.tiers.get(&ModelTier::Light); // → "glm-4.7"
}

// 通过 resolve_from_plan（含 provider 匹配 + 版本选择）
let entry = resolve_from_plan("zhipuai-coding-plan", ModelTier::Strong, &providers);
assert_eq!(entry.unwrap().name, "glm-5.2"); // 最高版本
```

---

## 模型层：异步完整解析

模型层在核心层基础上增加 models.dev spec 和 provider API 回退，依赖 reqwest/tokio。

### ResolvedTierModel — 解析结果

```rust
/// tier 解析的最终结果：完整的模型 + provider 信息。
#[derive(Clone)]
pub struct ResolvedTierModel {
    pub model_id: String,         // "provider/model-name"
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
    pub provider_name: Option<String>,
}

impl ResolvedTierModel {
    pub fn from_entry(entry: ModelEntry) -> Self { ... }
}
```

### TierResolver trait — 解析器抽象

```rust
/// 将 tier 解析为具体模型。
///
/// 独立于 `loom-react-config`，调用方需提前加载 providers。
#[async_trait]
pub trait TierResolver: Send + Sync {
    async fn resolve_tier(
        &self,
        model: Option<&str>,        // 显式模型覆盖（如 "openai/gpt-4o"）
        tier: ModelTier,            // 目标 tier
        provider_hint: Option<&str>,// provider 名称提示
        providers: &[ProviderConfig],
    ) -> Option<ResolvedTierModel>;
}
```

**DefaultTierResolver 决策逻辑**：

```
model 参数
├─ Some(model_id)
│   ├─ provider 启用 tier_resolution?
│   │   └─ Yes → resolve_tier_intelligent(provider, tier, providers)
│   │   └─ No  → resolve_for_model(model_id, tier, providers)
│   └─ 无匹配 provider → resolve_for_model(model_id, tier, providers)
│
└─ None
    ├─ Some(provider_hint)
    │   ├─ provider 启用 tier_resolution?
    │   │   └─ Yes → resolve_tier_intelligent(provider, tier, providers)
    │   │   └─ No  → None
    │   └─ 无匹配 → resolve_tier_intelligent(provider, tier, providers)
    │
    └─ None
        └─ 遍历所有启用了 tier_resolution 的 provider
            └─ resolve_tier_intelligent(p, tier, providers) → 第一个成功的
```

### resolve_tier_intelligent() — 策略链

```rust
/// 使用所有可用策略解析 tier（plan → spec → provider API）。
///
/// 按优先级依次尝试，第一个成功的结果即返回。
pub async fn resolve_tier_intelligent(
    provider: &str,
    tier: ModelTier,
    providers: &[ProviderConfig],
) -> Option<ModelEntry>
```

**策略 1 — resolve_from_plan()**（同步，核心层）：
- 从 `plans.toml` 内嵌数据查找
- provider_id 前缀匹配 + 最高 version 选择
- 无网络请求

**策略 2 — resolve_from_spec()**（异步）：
- 从 `ModelRegistry::global()` 获取 models.dev 数据
- 调用 `pick_best_for_tier()` 按 tier 筛选 + 按 release_date 排序
- 需要 reqwest

**策略 3 — resolve_from_provider_api()**（异步）：
- 从 provider 的 `/v1/models` API 获取模型列表
- 取列表中第一个模型
- 需要 `fetch_models = true` 配置

**使用示例**：
```rust
// 完整异步解析
let resolved = resolve_tier_intelligent("zhipuai", ModelTier::Strong, &providers).await;
// → ResolvedTierModel { model_id: "zhipuai/glm-5.2", base_url: "...", ... }

// 通过 trait
let resolver = DefaultTierResolver;
let resolved = resolver.resolve_tier(None, ModelTier::Light, Some("zhipuai"), &providers).await;

// 便捷函数
let resolved = resolve_tier(None, ModelTier::Light, Some("zhipuai"), &providers).await;
```

---

## pick_best_for_tier() — 从模型列表选择

辅助函数：从已知的模型 HashMap 中，按 tier 筛选并选出最新发布的模型。

```rust
/// 从模型映射中选出匹配 tier 的最佳模型。
///
/// 筛选：`tier_of()` 分类 → tier 匹配
/// 排序：release_date 降序（最新优先）
/// 返回 None 如果无匹配或 tier == None
pub fn pick_best_for_tier<'a>(
    models: &'a HashMap<String, Model>,
    tier: ModelTier,
) -> Option<(&'a String, &'a Model)>
```

---

## Crate 分层归属

```
model-tier crate
├── 核心层 (仅 serde + toml)
│   ├── ModelTier 枚举
│   ├── Cost 结构体
│   ├── tier_of() 函数           ← model → tier（正向）
│   ├── pick_best_for_tier() 函数 ← tier → model（反向，从已知列表）
│   ├── TierPlan 结构体           ← tier → model（反向，从计划）
│   └── tier_plans() / resolve_from_plan()
│
└── 模型层 (reqwest + tokio)
    ├── TierResolver trait
    ├── DefaultTierResolver
    ├── resolve_tier_intelligent()
    ├── resolve_from_spec()
    └── resolve_from_provider_api()
```

### API 导出

**核心层**：
```rust
pub use tier_plan::{TierPlan, tier_plans, resolve_from_plan};
```

**模型层**：
```rust
pub use tier_resolve::{
    ResolvedTierModel,
    TierResolver, DefaultTierResolver,
    resolve_tier,
    resolve_tier_intelligent,
    resolve_from_spec,
};
```

---

## 现有代码参考

| 组件 | 源码位置 |
|------|----------|
| `TierPlan` 结构体 | `foundation/model-spec-core/src/tier_plan/mod.rs:10` |
| `tier_plans()` 加载 | `foundation/model-spec-core/src/tier_plan/mod.rs:46` |
| `plans.toml` 数据 | `foundation/model-spec-core/src/tier_plan/plans.toml` |
| `resolve_from_plan()` | `foundation/model-spec-core/src/tier_resolve.rs:210` |
| `resolve_from_spec()` | `foundation/model-spec-core/src/tier_resolve.rs:163` |
| `resolve_from_provider_api()` | `foundation/model-spec-core/src/tier_resolve.rs:183` |
| `resolve_tier_intelligent()` | `foundation/model-spec-core/src/tier_resolve.rs:272` |
| `TierResolver` trait | `foundation/model-spec-core/src/tier_resolve.rs:49` |
| `DefaultTierResolver` | `foundation/model-spec-core/src/tier_resolve.rs:66` |
| `ResolvedTierModel` | `foundation/model-spec-core/src/tier_resolve.rs:19` |
| `pick_best_for_tier()` | `foundation/model-spec-core/src/tier.rs:51` |
| `provider_id_matches()` | `foundation/model-spec-core/src/tier_resolve.rs:245` |
| `compare_versions()` | `foundation/model-spec-core/src/tier_resolve.rs:256` |
