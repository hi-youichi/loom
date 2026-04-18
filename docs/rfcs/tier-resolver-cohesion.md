# RFC: 将 TierResolver 从 agent/react/build/llm.rs 内聚到 model_spec 模块

| 字段 | 值 |
|---|---|
| 状态 | Draft |
| 作者 | - |
| 日期 | 2025-08-19 |
| 影响模块 | `loom/src/llm`, `loom/src/model_spec`, `loom/src/agent/react/build`, `loom/src/cli_run` |

---

## 1. 背景与问题

当前 `TierResolver` trait、`DefaultTierResolver`、`ResolvedTierModel` 定义在 `loom/src/agent/react/build/llm.rs:66-148`。

该文件的职责是 **构建 LLM 客户端**（`build_default_llm` 等），但 tier 解析是一个独立的关注点：

- `DefaultTierResolver` 的核心操作是查询 `ModelRegistry`、读取 `ProviderConfig`——这些都是 `crate::llm` 的类型
- `ResolvedTierModel` 本质上是 `ModelEntry` 的简化投影
- tier 解析与 agent react 构建流程无直接耦合，只是被 `cli_run/agent.rs` 在构建 runner 之前调用

**违反的原则**：单一职责——LLM 构建文件里混入了 model tier 解析策略。

---

## 2. 方案对比

### 方案 A（原版）：整体搬入 `loom/src/model_spec/`

将 `TierResolver` trait + `DefaultTierResolver` + `ResolvedTierModel` 全部放入 `model_spec`。

- **缺点**：`DefaultTierResolver` 依赖 `ModelRegistry`、`ProviderConfig`、`ModelEntry`、`load_provider_configs()`——全部在 `crate::llm`。`model_spec` 反向依赖 `crate::llm`，引入模块间耦合

### 方案 B：整体搬入 `loom/src/llm/tier.rs`

- **优点**：零跨模块依赖
- **缺点**：`TierResolver` trait 与 `ModelTier` 枚举分处不同模块，内聚性一般；trait 仍然绑定 `ReactBuildConfig`

### 方案 C：独立顶层模块 `loom/src/tier/`

- **缺点**：仅 3 个类型单独一个顶层模块，粒度过细

### 方案 A'（改进版）：Trait 与 Impl 分离 ✅ 推荐

用 **依赖倒置** 拆开：trait + 纯领域类型放入 `model_spec`，具体实现留在 `llm`。

- **优点**：最高内聚 + 零循环依赖 + trait 解耦 ReactBuildConfig
- 详见第 3 节

---

## 3. 推荐方案详细设计：方案 A'

### 3.1 核心思路：依赖倒置 + Trait 签名解耦

```
model_spec（定义抽象层）
  ├── ModelTier enum          ← 已有
  ├── pick_best_for_tier()    ← 已有
  ├── ResolvedTierModel       ← 搬入（纯 String 字段，无外部依赖）
  └── TierResolver trait      ← 搬入（签名解耦 ReactBuildConfig）

llm（提供具体实现）
  ├── DefaultTierResolver     ← 留在这里（依赖 ModelRegistry 等）
  └── load_provider_configs() ← 留在这里
```

依赖方向：

```
model_spec (TierResolver trait + ResolvedTierModel + ModelTier)
    ↑                ↑
    │                │
  llm               cli_run
  (DefaultTierResolver 实现)  (适配层：ReactBuildConfig → model_hint)
```

- `model_spec` → 零外部依赖（纯领域类型）
- `llm` → 依赖 `model_spec`（实现 trait，和现有依赖方向一致）
- `cli_run` → 依赖两者（组装调用）
- **无循环依赖，无反向依赖**

### 3.2 Trait 签名变更

当前 trait 绑定了 `ReactBuildConfig`：

```rust
// 现在 (llm.rs:66-73)
#[async_trait]
pub trait TierResolver: Send + Sync {
    async fn resolve_tier(
        &self,
        config: &ReactBuildConfig,
        tier: ModelTier,
    ) -> Option<ResolvedTierModel>;
}
```

`DefaultTierResolver` 实际只读 `config.model`（一个 `Option<String>`）。解耦后：

```rust
// 改进后 (model_spec/tier_resolver.rs)
#[async_trait]
pub trait TierResolver: Send + Sync {
    async fn resolve_tier(
        &self,
        model_hint: Option<&str>,
        tier: ModelTier,
    ) -> Option<ResolvedTierModel>;
}
```

变更点：
- `config: &ReactBuildConfig` → `model_hint: Option<&str>`
- trait 不再感知任何 agent/react 类型

### 3.3 ResolvedTierModel 搬移

```rust
// model_spec/tier_resolver.rs
pub struct ResolvedTierModel {
    pub model_id: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub provider_type: Option<String>,
}
```

纯 String 字段，零外部依赖，可以直接放入 `model_spec`。

`from_entry()` 转换函数留在 `llm` 模块（因为依赖 `ModelEntry`）。

### 3.4 DefaultTierResolver 适配

留在 `loom/src/llm/` 下，新建 `loom/src/llm/tier_default.rs`：

```rust
// llm/tier_default.rs
use crate::model_spec::{ModelTier, ResolvedTierModel, TierResolver};
use crate::llm::{ModelEntry, ModelRegistry, ProviderConfig, load_provider_configs};

pub struct DefaultTierResolver;

#[async_trait]
impl TierResolver for DefaultTierResolver {
    async fn resolve_tier(
        &self,
        model_hint: Option<&str>,
        tier: ModelTier,
    ) -> Option<ResolvedTierModel> {
        let providers = load_provider_configs()?;

        match model_hint {
            Some(model_id) => {
                // 原有逻辑：从 provider 查 ModelEntry → 转为 ResolvedTierModel
                if let Some((provider, _model)) = ModelEntry::parse_id(model_id) {
                    if let Some(provider_cfg) = providers.iter().find(|p| p.name == provider) {
                        if provider_cfg.enable_tier_resolution {
                            let entry = ModelRegistry::global()
                                .resolve_tier(provider, tier)
                                .await?;
                            return Some(ResolvedTierModel {
                                model_id: entry.id,
                                base_url: entry.base_url,
                                api_key: entry.api_key,
                                provider_type: entry.provider_type,
                            });
                        }
                    }
                }
                // fallback ...
            }
            None => { /* 原有逻辑 */ }
        }
    }
}
```

### 3.5 受影响文件及改动

#### `loom/src/model_spec/tier_resolver.rs` — 新建

放入 `TierResolver` trait + `ResolvedTierModel`。

#### `loom/src/model_spec/mod.rs`

```diff
+ mod tier_resolver;
+ 
+ pub use tier_resolver::{ResolvedTierModel, TierResolver};
```

#### `loom/src/llm/tier_default.rs` — 新建

`DefaultTierResolver` 实现 + `load_provider_configs()`（从 `agent/react/build/llm.rs` 搬入）。

#### `loom/src/llm/mod.rs`

```diff
+ mod tier_default;
+ 
+ pub use tier_default::DefaultTierResolver;
```

#### `loom/src/agent/react/build/llm.rs`

- **删除**：`ResolvedTierModel`、`load_provider_configs()`、`TierResolver` trait、`DefaultTierResolver`、`resolve_tier_model()`
- **新增 import**：`use crate::model_spec::{ResolvedTierModel, TierResolver};`（函数签名需要）
- 保留 `build_default_llm`、`resolve_title_llm`、`model_entry_from_config`、`parse_provider_model` 等 LLM 构建逻辑

#### `loom/src/agent/react/build/mod.rs`

```diff
- pub use llm::{DefaultTierResolver, ResolvedTierModel, TierResolver};
+ pub use crate::model_spec::{ResolvedTierModel, TierResolver};
+ pub use crate::llm::DefaultTierResolver;
```

#### `loom/src/agent/react/mod.rs`

不变（re-export 来源由 `build/mod.rs` 决定）。

#### `loom/src/cli_run/agent.rs`

```diff
- use crate::agent::react::TierResolver;
+ use crate::model_spec::TierResolver;
```

适配 `resolve_tier_and_build_config_with_resolver`：

```rust
pub async fn resolve_tier_and_build_config_with_resolver(
    config: &ReactBuildConfig,
    resolver: &dyn TierResolver,
) -> ReactBuildConfig {
    let Some(tier) = config.model_tier else {
        return config.clone();
    };
    let mut config = config.clone();

    match resolver.resolve_tier(config.model.as_deref(), tier).await {
    //                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //                    适配：ReactBuildConfig → model_hint
        Some(resolved) => {
            config.model = Some(resolved.model_id);
            if config.openai_base_url.is_none() {
                config.openai_base_url = resolved.base_url;
            }
            if config.openai_api_key.is_none() {
                config.openai_api_key = resolved.api_key;
            }
            if config.llm_provider.is_none() && resolved.provider_type.is_some() {
                config.llm_provider = resolved.provider_type;
            }
        }
        None => { /* 不变 */ }
    }
    config
}
```

#### `loom/src/lib.rs`

```diff
-     DefaultTierResolver, ..., TierResolver,
+     // 确认 pub use 路径仍可达（通过 agent::react 或 model_spec re-export）
```

#### `loom/src/tools/invoke_agent.rs` (测试)

```diff
- impl crate::TierResolver for MockTierResolver {
+ impl crate::model_spec::TierResolver for MockTierResolver {
    async fn resolve_tier(
        &self,
-       config: &ReactBuildConfig,
-       tier: ModelTier,
+       model_hint: Option<&str>,
+       tier: ModelTier,
    ) -> Option<ResolvedTierModel> {
-       let model = config.model.as_deref().unwrap();
+       // 直接用 model_hint
```

### 3.6 公开 API 变化

| 之前 | 之后 | 变化 |
|---|---|---|
| `loom::TierResolver` | `loom::TierResolver` | 不变（re-export 路径不变） |
| `loom::DefaultTierResolver` | `loom::DefaultTierResolver` | 不变 |
| `loom::ResolvedTierModel` | `loom::ResolvedTierModel` | 不变 |
| `TierResolver::resolve_tier` 签名 | 第一个参数 `&ReactBuildConfig` → `Option<&str>` | **breaking**（但仅限自定义实现者，内部可控） |

对外公开 API 除 trait 签名外 **零 breaking change**。`TierResolver` 的外部实现者需适配参数变更（当前仅 `MockTierResolver` 一个测试 mock）。

### 3.7 文件结构对比

```
Before:
  model-spec-core/src/tier.rs           ← ModelTier, pick_best_for_tier
  loom/src/model_spec/mod.rs            ← re-export ModelTier
  loom/src/agent/react/build/llm.rs     ← TierResolver + DefaultTierResolver + LLM 构建（混杂）
  loom/src/llm/mod.rs                   ← ModelRegistry, ModelEntry 等

After:
  model-spec-core/src/tier.rs           ← ModelTier, pick_best_for_tier（不变）
  loom/src/model_spec/tier_resolver.rs  ← TierResolver trait + ResolvedTierModel（新）
  loom/src/model_spec/mod.rs            ← 增加 re-export
  loom/src/llm/tier_default.rs          ← DefaultTierResolver 实现（新）
  loom/src/llm/mod.rs                   ← 增加 re-export
  loom/src/agent/react/build/llm.rs     ← 只保留 LLM 构建逻辑（瘦身）
```

---

## 4. 测试计划

1. `cargo clippy -- -D warnings` 通过
2. `MockTierResolver` 测试（`invoke_agent.rs:1035`）适配后通过
3. `resolve_tier_and_build_config` 相关测试通过
4. `llm.rs` 中 `model_entry_from_config` 系列测试不受影响

---

## 5. 风险与缓解

| 风险 | 缓解 |
|---|---|
| `TierResolver` 签名变更影响外部实现者 | 当前仅 `MockTierResolver` 一个 mock，改动可控；如需保持兼容可暂用 `#[deprecated]` 旧签名 |
| re-export 路径遗漏导致下游 break | 对外 `loom::` 路径不变，逐一检查 grep 结果确认 |
| `load_provider_configs()` 放在 `llm` 模块是否合适 | 该函数本质是读取 provider 配置，与 `ProviderConfig` 定义同模块，合理 |

---

## 6. 后续可选优化

- `ResolvedTierModel` 可考虑直接暴露 `ModelEntry` 或用 newtype 包装，减少字段重复
- `load_provider_configs()` 未来可移到独立的 provider 配置模块
- 如需支持 `HybridTierResolver`（见 `docs/gateway_provider_tier_resolution.md`），trait 定义已就绪，只需在 `llm` 模块新增实现
- `resolve_tier_model()` 便捷函数可考虑作为 `TierResolver` 的 default method
