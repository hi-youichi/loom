# Model Tier Crate 提取方案

**创建时间**：2025-08-19

> 决策矩阵与最终类型定义见 [README](./README.md)。

---

## 架构图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           model-tier (统一 crate)                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      核心层（轻量，仅依赖 serde）                    │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌────────┐│   │
│  │  │ ModelTier     │  │ Cost          │  │ tier_of()     │  │ pick_  ││   │
│  │  │ 枚举          │  │ 结构体        │  │ 函数          │  │ best() ││   │
│  │  │               │  │               │  │               │  │ 函数   ││   │
│  │  │ - None        │  │ - input       │  │ - id          │  │        ││   │
│  │  │ - Light       │  │ - output      │  │ - family      │  │        ││   │
│  │  │ - Standard    │  │ - cache_read  │  │ - cost        │  │        ││   │
│  │  │ - Strong      │  │ - cache_write │  │               │  │        ││   │
│  │  └───────────────┘  └───────────────┘  └───────────────┘  └────────┘│   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    模型层（完整功能，依赖 reqwest/tokio）             │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌────────┐│   │
│  │  │ ModelInfo     │  │ ModelRegistry │  │ TierResolver  │  │ Tier   ││   │
│  │  │ 结构体        │  │               │  │               │  │ Plan   ││   │
│  │  │               │  │ - models      │  │ - resolve()   │  │        ││   │
│  │  │ - id          │  │ - get()       │  │ - fetch()     │  │        ││   │
│  │  │ - name        │  │ - list()      │  │               │  │        ││   │
│  │  │ - family      │  │               │  │               │  │        ││   │
│  │  │ - cost        │  │               │  │               │  │        ││   │
│  │  │ - tier()      │  │               │  │               │  │        ││   │
│  │  └───────────────┘  └───────────────┘  └───────────────┘  └────────┘│   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ 依赖
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            其他项目                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐                    │
│  │   项目 A      │  │   项目 B      │  │   项目 C      │                    │
│  │               │  │               │  │               │                    │
│  │ 使用核心层    │  │ 使用核心层    │  │ 使用模型层    │                    │
│  │ (轻量)        │  │ (轻量)        │  │ (完整)        │                    │
│  └───────────────┘  └───────────────┘  └───────────────┘                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ 依赖
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Loom 主项目                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐                    │
│  │   CLI         │  │ Agent Core    │  │  其他模块     │                    │
│  │               │  │               │  │               │                    │
│  │ 使用模型层    │  │ 使用模型层    │  │ 使用模型层    │                    │
│  │ (完整)        │  │ (完整)        │  │ (完整)        │                    │
│  └───────────────┘  └───────────────┘  └───────────────┘                    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

数据流：
```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│  ┌─────────┐      ┌─────────┐      ┌─────────┐      ┌─────────┐            │
│  │ ModelInfo│─────▶│  tier() │─────▶│ tier_of()│─────▶│ModelTier│            │
│  └─────────┘      └─────────┘      └─────────┘      └─────────┘            │
│                                                                             │
│  ┌─────────┐      ┌─────────┐      ┌─────────┐      ┌─────────┐            │
│  │ models  │─────▶│ pick_   │─────▶│ tier_of()│─────▶│ best    │            │
│  │ HashMap │      │ best()  │      │         │      │ model   │            │
│  └─────────┘      └─────────┘      └─────────┘      └─────────┘            │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

依赖关系：
```
其他项目 ──────────────────────▶ model-tier (核心层)
                                     │
                                     │ 仅依赖
                                     ▼
                                   serde

Loom 主项目 ──────────────────────▶ model-tier (完整)
                                     │
                                     │ 依赖
                                     ▼
  async-trait  tokio  reqwest  tracing  toml
```

**设计原则**：
- 单一 crate，统一管理
- 分层设计，按需使用
- 核心层可独立，模型层可选
- 其他项目可只依赖核心层（轻量）

---

## 一、背景分析

### 1.1 当前结构

```
foundation/model-spec-core/
├── src/
│   ├── tier.rs            # 核心 tier 逻辑（~260 行）
│   ├── cost.rs            # Cost 结构体（~45 行）
│   ├── model.rs           # Model 结构体（使用 tier）
│   ├── tier_resolve.rs    # 异步解析逻辑（~497 行）
│   ├── tier_plan/         # Tier 计划配置
│   ├── tier_error.rs      # 错误类型
│   └── model_registry.rs  # 模型注册表
└── Cargo.toml
```

### 1.2 依赖分析

**核心 tier 逻辑依赖**（可独立）：
- `ModelTier` 枚举：仅依赖 `serde`
- `tier_of()` 函数：仅依赖 `serde`
- `pick_best_for_tier()` 函数：仅依赖 `serde`
- `Cost` 结构体：仅依赖 `serde`

**重量级依赖**（不可独立）：
- `tier_resolve.rs`：依赖 `async-trait`, `tokio`, `reqwest`, `tracing`
- `tier_plan/`：依赖 `toml`, `thiserror`
- `model_registry.rs`：依赖 `reqwest`, `tokio`

### 1.3 使用情况统计

**直接使用 ModelTier**（12 处）：
- `apps/cli/`：profile_convert 模块（3 处）
- `agent/agent-core/`：state、profile、react 等模块（9 处）

**使用 tier 解析函数**：
- `pick_best_for_tier()`：2 处（tier_resolve.rs, lib.rs）
- `tier_of()`：1 处（model.rs 内部）

---

## 二、方案设计

### 2.1 新 Crate 结构

```
foundation/model-tier/
├── Cargo.toml
├── src/
│   ├── lib.rs             # 公共 API 导出
│   ├── tier.rs            # ModelTier 枚举定义（固定 4 个级别）
│   ├── cost.rs            # Cost 结构体（最小版本）
│   ├── classifier.rs      # tier_of() 分类逻辑
│   ├── picker.rs          # pick_best_for_tier() 匹配逻辑
│   ├── tier_plan/
│   │   ├── mod.rs         # TierPlan 结构体 + tier_plans()
│   │   └── plans.toml     # 内嵌 tier → model 映射数据
│   └── resolve.rs         # resolve_from_plan() 同步解析
└── tests/
    └── integration.rs     # 集成测试
```

**注意**：异步解析链（`resolve_tier_intelligent`、`TierResolver` trait、`resolve_from_spec`）保留在 `model-spec-core` 模型层中，详见 [Tier → Model 解析 API](./tier-resolution.md)。

### 2.2 ModelInfo 设计（model-spec-core 中）

```rust
/// 模型完整信息（保留在 model-spec-core 中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    // 基础信息
    pub id: String,                    // "openai/gpt-4o-mini"
    pub name: String,                  // "GPT-4o Mini"
    pub description: Option<String>,
    pub family: Option<String>,        // "gpt-4o-mini"

    // 能力标志
    pub attachment: bool,
    pub reasoning: bool,
    pub reasoning_options: Vec<String>,
    pub tool_call: bool,
    pub temperature: bool,

    // 时间信息
    pub knowledge: Option<String>,
    pub release_date: Option<String>,
    pub last_updated: Option<String>,

    // 模态
    pub modalities: Modalities,

    // 开放权重
    pub open_weights: bool,

    // 限制
    pub limit: Limit,

    // 成本
    pub cost: Cost,

    // Tier（自动计算）
    #[serde(skip)]
    tier: ModelTier,
}

/// 模态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Modalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

/// 限制信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limit {
    pub context: u64,
    pub output: u64,
}

/// 成本信息（$/M tokens）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}
```

**ModelInfo 方法**：
```rust
impl ModelInfo {
    /// 获取模型的 tier（自动计算）
    pub fn tier(&self) -> ModelTier {
        if self.tier != ModelTier::None {
            return self.tier;
        }
        self.tier = tier_of(&self.id, self.family.as_deref(), Some(&self.cost));
        self.tier
    }

    /// 设置 tier（用于手动覆盖）
    pub fn with_tier(mut self, tier: ModelTier) -> Self {
        self.tier = tier;
        self
    }

    /// 创建最小的 ModelInfo（仅必需字段）
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            family: None,
            attachment: false,
            reasoning: false,
            reasoning_options: Vec::new(),
            tool_call: false,
            temperature: true,
            knowledge: None,
            release_date: None,
            last_updated: None,
            modalities: Modalities {
                input: vec!["text".to_string()],
                output: vec!["text".to_string()],
            },
            open_weights: false,
            limit: Limit { context: 128000, output: 4096 },
            cost: Cost { input: 0.0, output: 0.0, cache_read: None, cache_write: None },
            tier: ModelTier::None,
        }
    }
}
```

### 2.3 模块职责划分

**tier.rs** — 类型定义：
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    #[default]
    None,
    Light,
    Standard,
    Strong,
}
```

**cost.rs** — 成本数据：
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub input: f64,
    pub output: f64,
}
```

**classifier.rs** — 分类逻辑：
```rust
pub fn tier_of(id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
    // 分类逻辑
}
```

**picker.rs** — 匹配逻辑：
```rust
pub fn pick_best_for_tier<'a>(
    models: &'a HashMap<String, ModelInfo>,
    tier: ModelTier,
) -> Option<(&'a String, &'a ModelInfo)> {
    // 匹配逻辑
}
```

### 2.4 API 设计

**核心 API**（必须）：
```rust
pub use tier::ModelTier;
pub use cost::Cost;
pub use classifier::tier_of;
pub use picker::pick_best_for_tier;
pub use tier::ModelTierVariant;  // 用于 CLI 参数解析
```

**扩展 API**（可选，详见 [配置化规则决策](./decisions/configurable-rules.md)）：
```rust
pub trait TierClassifier: Send + Sync {
    fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier;
}

pub struct DefaultClassifier;
impl TierClassifier for DefaultClassifier { ... }
```

### 2.5 依赖配置

```toml
[package]
name = "model-tier"
version = "0.1.0"
edition = "2021"
description = "Model tier classification and selection for LLM models"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = { version = "1.0", optional = true }

[dev-dependencies]
serde_json = "1.0"

[features]
default = ["std"]
std = []
json = ["dep:serde_json"]
```

---

## 三、实施计划

### 3.1 阶段一：创建新 Crate（预计 2 小时）

- [ ] 创建目录结构（`mkdir -p foundation/model-tier/{src,tests}`）
- [ ] 创建 `Cargo.toml`（包名、版本、最小依赖 serde、features）
- [ ] 迁移 `tier.rs`（ModelTier 枚举 + Display + variants()）
- [ ] 迁移 `cost.rs`（Cost 结构体 + 核心方法）
- [ ] 迁移 `classifier.rs`（tier_of()，不依赖 Model）
- [ ] 迁移 `picker.rs`（pick_best_for_tier()，泛型化 Model）
- [ ] 创建 `lib.rs`（导出公共 API + 模块文档）
- [ ] 迁移测试（单元测试 + 集成测试）

### 3.2 阶段二：重构 model-spec-core（预计 1.5 小时）

- [ ] 添加 `model-tier` 依赖
- [ ] 删除重复代码（ModelTier、Cost、tier_of、pick_best_for_tier）
- [ ] Re-export 公共 API 保持向后兼容：
  ```rust
  // foundation/model-spec-core/src/lib.rs
  pub use model_tier::{ModelTier, Cost, tier_of, pick_best_for_tier};
  ```
- [ ] 更新内部引用（model.rs、tier_resolve.rs）
- [ ] 处理 breaking changes（确保 `use model_spec_core::ModelTier` 仍工作）

### 3.3 阶段三：验证和测试（预计 1 小时）

```bash
cd foundation/model-tier && cargo test
cd foundation/model-spec-core && cargo test
cargo test --workspace
cargo clippy --workspace
```

### 3.4 阶段四：文档和示例（预计 30 分钟）

- [ ] 编写 model-tier README.md
- [ ] 更新 Loom ARCHITECTURE.md
- [ ] 创建使用示例

---

## 四、技术细节

### 4.1 泛型化 Model 类型

**问题**：`pick_best_for_tier()` 当前依赖 `Model` 结构体，其他项目可能有不同的 Model 类型。

**方案 A — 使用 trait**（推荐）：
```rust
pub trait TierQueryable {
    fn tier(&self) -> ModelTier;
    fn release_date(&self) -> Option<&str>;
}

pub fn pick_best_for_tier<'a, T: TierQueryable>(
    models: &'a HashMap<String, T>,
    tier: ModelTier,
) -> Option<(&'a String, &'a T)> { ... }
```

**方案 B — 使用泛型 + 闭包**：
```rust
pub fn pick_best_for_tier<'a, V>(
    models: &'a HashMap<String, V>,
    tier: ModelTier,
    tier_fn: impl Fn(&V) -> ModelTier,
    date_fn: impl Fn(&V) -> Option<&str>,
) -> Option<(&'a String, &'a V)> { ... }
```

**方案 C — 保留 ModelInfo 内置类型**（最简单，推荐）：
```rust
pub struct ModelInfo {
    pub id: String,
    pub family: Option<String>,
    pub cost: Option<Cost>,
    pub release_date: Option<String>,
}

impl ModelInfo {
    pub fn tier(&self) -> ModelTier {
        tier_of(&self.id, self.family.as_deref(), self.cost.as_ref())
    }
}
```

### 4.2 处理 breaking changes

1. 在 model-spec-core 中 re-export 所有 model-tier 的公共类型
2. 保持函数签名完全相同
3. 使用 `#[deprecated]` 标记旧路径（可选）

```rust
// foundation/model-spec-core/src/lib.rs
pub use model_tier::ModelTier;
pub use model_tier::Cost;
pub use model_tier::tier_of;
pub use model_tier::pick_best_for_tier;
```

### 4.3 处理 serde 依赖

```toml
# 根目录 Cargo.toml
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }

# foundation/model-tier/Cargo.toml
[dependencies]
serde = { workspace = true }
```

### 4.4 处理 Cost 结构体的扩展字段

```rust
impl Cost {
    /// 创建只包含 input/output 的最小 Cost
    pub fn new(input: f64, output: f64) -> Self {
        Self { input, output, cache_read: None, cache_write: None, reasoning: None }
    }

    /// 创建完整的 Cost
    pub fn full(
        input: f64, output: f64,
        cache_read: Option<f64>, cache_write: Option<f64>,
        reasoning: Option<f64>,
    ) -> Self {
        Self { input, output, cache_read, cache_write, reasoning }
    }
}
```

---

## 五、风险和缓解措施

| 风险 | 缓解措施 |
|------|----------|
| API 签名变化导致编译错误 | 保持签名不变 + re-export 向后兼容 |
| serde 版本冲突 | 使用 workspace 依赖统一版本 |
| 测试覆盖不足 | 完整迁移测试 + 添加集成测试 |
| 泛型/trait 导致性能下降 | `#[inline]` 标注 + 基准测试对比 |

---

## 六、验收标准

### 功能验收

- [ ] model-tier crate 可独立编译
- [ ] 所有现有测试通过
- [ ] API 文档完整
- [ ] model-spec-core 保持向后兼容

### 性能验收

- [ ] tier 分类性能无退化（< 1%）
- [ ] 编译时间无显著增加（< 10%）
- [ ] 二进制大小无显著增加

### 质量验收

- [ ] `cargo clippy` 无警告
- [ ] 代码覆盖率 > 90%
- [ ] 所有公共 API 有文档注释
- [ ] 无 `unsafe` 代码

---

## 时间估算

| 阶段 | 任务 | 预计时间 |
|------|------|----------|
| 一 | 创建新 Crate | 2h |
| 二 | 重构 model-spec-core | 1.5h |
| 三 | 验证和测试 | 1h |
| 四 | 文档和示例 | 0.5h |
| **总计** | | **5h** |
