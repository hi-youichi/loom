# 决策三：配置化分类规则

**决策**：📋 分阶段实施（当前硬编码 → Builder → 配置文件）

> 返回 [README](../README.md)

---

## 背景

当前 `tier_of()` 的分类规则是**硬编码**的：

```rust
pub fn tier_of(id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
    // 硬编码规则 1：family 后缀
    if let Some(family) = family {
        let f = family.to_lowercase();
        if f.ends_with("flash") || f.ends_with("haiku") || f.ends_with("mini") {
            return ModelTier::Light;
        }
        if f.ends_with("opus") || f.ends_with("ultra") {
            return ModelTier::Strong;
        }
    }

    // 硬编码规则 2：id 关键词
    let id_lower = id.to_lowercase();
    if id_lower.contains("flash") || id_lower.contains("mini") {
        return ModelTier::Light;
    }

    // 硬编码规则 3：成本阈值
    if let Some(cost) = cost {
        if cost.input < 0.5 {
            return ModelTier::Light;
        }
        if cost.input > 15.0 {
            return ModelTier::Strong;
        }
    }

    // 默认
    ModelTier::Standard
}
```

**问题**：
- 维护成本高：新模型发布时需要修改代码
- 灵活性差：不同项目可能有不同的分类标准
- 测试困难：难以测试不同的分类策略

---

## 方案对比

### 方案 A：配置文件（TOML/YAML）

**配置文件** (`tier-rules.toml`)：
```toml
[light]
family_suffixes = ["flash", "flashx", "haiku", "mini", "air", "airx"]
id_keywords = ["flash", "flashx", "air", "airx", "mini"]
max_input_cost = 0.5

[standard]
# 默认 tier，无需配置

[strong]
family_suffixes = ["opus", "ultra", "long"]
id_keywords = ["long"]
min_input_cost = 15.0
special_prefixes = ["glm-5"]
```

**实现**：
```rust
#[derive(Debug, Deserialize)]
pub struct TierRules {
    pub light: Option<TierRule>,
    pub standard: Option<TierRule>,
    pub strong: Option<TierRule>,
}

#[derive(Debug, Deserialize)]
pub struct TierRule {
    #[serde(default)]
    pub family_suffixes: Vec<String>,
    #[serde(default)]
    pub id_keywords: Vec<String>,
    pub min_input_cost: Option<f64>,
    pub max_input_cost: Option<f64>,
    #[serde(default)]
    pub special_prefixes: Vec<String>,
}

impl TierRules {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn default_rules() -> Self {
        Self {
            light: Some(TierRule {
                family_suffixes: vec![
                    "flash".into(), "flashx".into(),
                    "haiku".into(), "mini".into(),
                    "air".into(), "airx".into(),
                ],
                id_keywords: vec![
                    "flash".into(), "flashx".into(),
                    "air".into(), "airx".into(),
                    "mini".into(),
                ],
                min_input_cost: None,
                max_input_cost: Some(0.5),
                special_prefixes: Vec::new(),
            }),
            standard: None,
            strong: Some(TierRule {
                family_suffixes: vec!["opus".into(), "ultra".into(), "long".into()],
                id_keywords: vec!["long".into()],
                min_input_cost: Some(15.0),
                max_input_cost: None,
                special_prefixes: vec!["glm-5".into()],
            }),
        }
    }

    pub fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
        if let Some(ref strong) = self.strong {
            if strong.matches(id, family, cost) { return ModelTier::Strong; }
        }
        if let Some(ref light) = self.light {
            if light.matches(id, family, cost) { return ModelTier::Light; }
        }
        ModelTier::Standard
    }
}

impl TierRule {
    pub fn matches(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> bool {
        let id_lower = id.to_lowercase();

        if let Some(family) = family {
            let family_lower = family.to_lowercase();
            if self.family_suffixes.iter().any(|s| family_lower.ends_with(&s.to_lowercase())) {
                return true;
            }
        }

        if self.id_keywords.iter().any(|k| id_lower.contains(&k.to_lowercase())) {
            return true;
        }

        if self.special_prefixes.iter().any(|p| id_lower.starts_with(&p.to_lowercase())) {
            return true;
        }

        if let Some(cost) = cost {
            if cost.input > 0.0 {
                if let Some(max) = self.max_input_cost {
                    if cost.input < max { return true; }
                }
                if let Some(min) = self.min_input_cost {
                    if cost.input > min { return true; }
                }
            }
        }

        false
    }
}
```

**使用**：
```rust
// 使用默认规则
let rules = TierRules::default_rules();
let tier = rules.classify("gpt-4o-mini", Some("gpt-4o-mini"), None);

// 从文件加载
let rules = TierRules::from_file("tier-rules.toml")?;
```

| 优点 | 缺点 |
|------|------|
| 无需重新编译：修改配置即可更新规则 | 引入文件 I/O 依赖 |
| 用户友好：非程序员也可以编辑 | 需要处理配置格式错误 |
| 版本控制友好 | 性能略差（运行时解析） |
| 多环境支持 | 调试困难（规则逻辑分散在配置中） |

---

### 方案 B：Builder 模式（代码配置）

```rust
/// 规则特征
pub trait Rule: Send + Sync + std::fmt::Debug {
    fn matches(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> bool;
    fn description(&self) -> String;
}

/// Family 后缀规则
#[derive(Debug)]
pub struct FamilySuffixRule { suffix: String, tier: ModelTier }

impl Rule for FamilySuffixRule {
    fn matches(&self, _: &str, family: Option<&str>, _: Option<&Cost>) -> bool {
        family.map_or(false, |f| f.to_lowercase().ends_with(&self.suffix.to_lowercase()))
    }
    fn description(&self) -> String {
        format!("family suffix '{}' → {:?}", self.suffix, self.tier)
    }
}

/// ID 关键词规则
#[derive(Debug)]
pub struct IdKeywordRule { keyword: String, tier: ModelTier }

impl Rule for IdKeywordRule {
    fn matches(&self, id: &str, _: Option<&str>, _: Option<&Cost>) -> bool {
        id.to_lowercase().contains(&self.keyword.to_lowercase())
    }
    fn description(&self) -> String {
        format!("id keyword '{}' → {:?}", self.keyword, self.tier)
    }
}

/// 成本阈值规则
#[derive(Debug)]
pub struct CostThresholdRule { min: Option<f64>, max: Option<f64>, tier: ModelTier }

impl Rule for CostThresholdRule {
    fn matches(&self, _: &str, _: Option<&str>, cost: Option<&Cost>) -> bool {
        if let Some(cost) = cost {
            if cost.input > 0.0 {
                let min_match = self.min.map_or(true, |min| cost.input > min);
                let max_match = self.max.map_or(true, |max| cost.input < max);
                return min_match && max_match;
            }
        }
        false
    }
    fn description(&self) -> String {
        match (self.min, self.max) {
            (Some(min), Some(max)) => format!("cost ${}-${}/M → {:?}", min, max, self.tier),
            (Some(min), None) => format!("cost >${}/M → {:?}", min, self.tier),
            (None, Some(max)) => format!("cost <${}/M → {:?}", max, self.tier),
            (None, None) => format!("any cost → {:?}", self.tier),
        }
    }
}

/// 特殊前缀规则
#[derive(Debug)]
pub struct SpecialPrefixRule { prefix: String, tier: ModelTier }

impl Rule for SpecialPrefixRule {
    fn matches(&self, id: &str, _: Option<&str>, _: Option<&Cost>) -> bool {
        id.to_lowercase().starts_with(&self.prefix.to_lowercase())
    }
    fn description(&self) -> String {
        format!("prefix '{}' → {:?}", self.prefix, self.tier)
    }
}

/// Tier 分类器
pub struct TierClassifier {
    light_rules: Vec<Arc<dyn Rule>>,
    strong_rules: Vec<Arc<dyn Rule>>,
}

impl TierClassifier {
    pub fn with_defaults() -> Self {
        TierClassifierBuilder::new()
            // Light 规则
            .add_family_suffix("flash", ModelTier::Light)
            .add_family_suffix("flashx", ModelTier::Light)
            .add_family_suffix("haiku", ModelTier::Light)
            .add_family_suffix("mini", ModelTier::Light)
            .add_family_suffix("air", ModelTier::Light)
            .add_family_suffix("airx", ModelTier::Light)
            .add_id_keyword("flash", ModelTier::Light)
            .add_id_keyword("flashx", ModelTier::Light)
            .add_id_keyword("air", ModelTier::Light)
            .add_id_keyword("airx", ModelTier::Light)
            .add_id_keyword("mini", ModelTier::Light)
            .add_cost_threshold(None, Some(0.5), ModelTier::Light)
            // Strong 规则
            .add_family_suffix("opus", ModelTier::Strong)
            .add_family_suffix("ultra", ModelTier::Strong)
            .add_family_suffix("long", ModelTier::Strong)
            .add_id_keyword("long", ModelTier::Strong)
            .add_special_prefix("glm-5", ModelTier::Strong)
            .add_cost_threshold(Some(15.0), None, ModelTier::Strong)
            .build()
    }

    pub fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
        for rule in &self.strong_rules {
            if rule.matches(id, family, cost) { return ModelTier::Strong; }
        }
        for rule in &self.light_rules {
            if rule.matches(id, family, cost) { return ModelTier::Light; }
        }
        ModelTier::Standard
    }

    pub fn rules(&self) -> Vec<&dyn Rule> {
        let mut rules: Vec<&dyn Rule> = Vec::new();
        rules.extend(self.strong_rules.iter().map(|r| r.as_ref()));
        rules.extend(self.light_rules.iter().map(|r| r.as_ref()));
        rules
    }
}

/// Tier 分类器构建器
pub struct TierClassifierBuilder {
    light_rules: Vec<Arc<dyn Rule>>,
    strong_rules: Vec<Arc<dyn Rule>>,
}

impl TierClassifierBuilder {
    pub fn new() -> Self {
        Self { light_rules: Vec::new(), strong_rules: Vec::new() }
    }

    pub fn add_family_suffix(mut self, suffix: impl Into<String>, tier: ModelTier) -> Self {
        let rule = Arc::new(FamilySuffixRule::new(suffix, tier));
        match tier {
            ModelTier::Light => self.light_rules.push(rule),
            ModelTier::Strong => self.strong_rules.push(rule),
            _ => {}
        }
        self
    }

    pub fn add_id_keyword(mut self, keyword: impl Into<String>, tier: ModelTier) -> Self {
        let rule = Arc::new(IdKeywordRule::new(keyword, tier));
        match tier {
            ModelTier::Light => self.light_rules.push(rule),
            ModelTier::Strong => self.strong_rules.push(rule),
            _ => {}
        }
        self
    }

    pub fn add_cost_threshold(mut self, min: Option<f64>, max: Option<f64>, tier: ModelTier) -> Self {
        let rule = Arc::new(CostThresholdRule::new(min, max, tier));
        match tier {
            ModelTier::Light => self.light_rules.push(rule),
            ModelTier::Strong => self.strong_rules.push(rule),
            _ => {}
        }
        self
    }

    pub fn add_special_prefix(mut self, prefix: impl Into<String>, tier: ModelTier) -> Self {
        let rule = Arc::new(SpecialPrefixRule::new(prefix, tier));
        match tier {
            ModelTier::Light => self.light_rules.push(rule),
            ModelTier::Strong => self.strong_rules.push(rule),
            _ => {}
        }
        self
    }

    pub fn add_rule(mut self, rule: impl Rule + 'static, tier: ModelTier) -> Self {
        let rule = Arc::new(rule);
        match tier {
            ModelTier::Light => self.light_rules.push(rule),
            ModelTier::Strong => self.strong_rules.push(rule),
            _ => {}
        }
        self
    }

    pub fn build(self) -> TierClassifier {
        TierClassifier {
            light_rules: self.light_rules,
            strong_rules: self.strong_rules,
        }
    }
}
```

**使用**：
```rust
// 使用默认规则
let classifier = TierClassifier::with_defaults();
let tier = classifier.classify("gpt-4o-mini", Some("gpt-4o-mini"), None);

// 自定义规则
let classifier = TierClassifierBuilder::new()
    .add_family_suffix("cheap", ModelTier::Light)
    .add_family_suffix("powerful", ModelTier::Strong)
    .build();

// 调试规则
for rule in classifier.rules() {
    println!("{}", rule.description());
}
```

| 优点 | 缺点 |
|------|------|
| 编译时类型安全 | 修改需要重新编译 |
| 零运行时开销（无文件 I/O） | 代码更复杂（多类型 + trait） |
| 易于测试 | 用户需要 Rust 知识 |
| IDE 支持 + 调试友好 | |

---

### 方案 C：策略模式 + 默认实现

```rust
/// Tier 分类策略特征
pub trait TierStrategy: Send + Sync + std::fmt::Debug {
    fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier;
    fn description(&self) -> String;
}

/// 默认策略（当前硬编码逻辑）
#[derive(Debug, Default)]
pub struct DefaultStrategy;

impl TierStrategy for DefaultStrategy {
    fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
        // 当前 tier_of() 的逻辑
        if let Some(family) = family {
            let f = family.to_lowercase();
            if f.ends_with("flash") || f.ends_with("flashx")
                || f.ends_with("haiku") || f.ends_with("mini")
                || f.ends_with("air") || f.ends_with("airx")
            { return ModelTier::Light; }
            if f.ends_with("opus") || f.ends_with("ultra")
                || f.contains("o1-pro") || f.ends_with("long")
            { return ModelTier::Strong; }
        }

        let id_lower = id.to_lowercase();
        let parts: Vec<&str> = id_lower.split('-').collect();
        if parts.iter().any(|p| matches!(*p, "flash" | "flashx" | "air" | "airx"))
            || parts.last() == Some(&"mini")
        { return ModelTier::Light; }
        if parts.contains(&"long") { return ModelTier::Strong; }
        if id_lower.starts_with("glm-5") { return ModelTier::Strong; }

        if let Some(cost) = cost {
            if cost.input > 0.0 && cost.input < 0.5 { return ModelTier::Light; }
            if cost.input > 15.0 { return ModelTier::Strong; }
        }

        ModelTier::Standard
    }

    fn description(&self) -> String { "Default strategy (hardcoded rules)".into() }
}

/// Builder 策略（使用 Builder 模式构建的规则）
#[derive(Debug)]
pub struct BuilderStrategy { classifier: TierClassifier }

impl TierStrategy for BuilderStrategy {
    fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
        self.classifier.classify(id, family, cost)
    }
    fn description(&self) -> String { "Builder strategy (configurable rules)".into() }
}

/// 配置文件策略
#[derive(Debug)]
pub struct ConfigStrategy { rules: TierRules }

impl TierStrategy for ConfigStrategy {
    fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
        self.rules.classify(id, family, cost)
    }
    fn description(&self) -> String { "Config strategy (file-based rules)".into() }
}

/// Tier 分类器（使用策略模式）
#[derive(Debug)]
pub struct TierClassifierWithStrategy {
    strategy: Arc<dyn TierStrategy>,
}

impl TierClassifierWithStrategy {
    pub fn new(strategy: impl TierStrategy + 'static) -> Self {
        Self { strategy: Arc::new(strategy) }
    }
    pub fn with_default() -> Self { Self::new(DefaultStrategy::new()) }
    pub fn with_builder() -> Self { Self::new(BuilderStrategy::with_defaults()) }
    pub fn with_config(path: impl AsRef<std::path::Path>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::new(ConfigStrategy::from_file(path)?))
    }
    pub fn classify(&self, id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
        self.strategy.classify(id, family, cost)
    }
    pub fn with_strategy(mut self, strategy: impl TierStrategy + 'static) -> Self {
        self.strategy = Arc::new(strategy);
        self
    }
}
```

**使用**：
```rust
// 默认策略
let classifier = TierClassifierWithStrategy::with_default();

// Builder 策略
let classifier = TierClassifierWithStrategy::with_builder();

// 配置文件策略
let classifier = TierClassifierWithStrategy::with_config("tier-rules.toml")?;

// 自定义策略
struct MyStrategy;
impl TierStrategy for MyStrategy {
    fn classify(&self, id: &str, _: Option<&str>, _: Option<&Cost>) -> ModelTier {
        if id.starts_with("test-") { ModelTier::Light } else { ModelTier::Standard }
    }
    fn description(&self) -> String { "Custom test strategy".into() }
}
let classifier = TierClassifierWithStrategy::new(MyStrategy);

// 动态切换策略
let classifier = TierClassifierWithStrategy::with_default().with_strategy(MyStrategy);
```

| 优点 | 缺点 |
|------|------|
| 最大灵活性：支持任意分类逻辑 | 增加复杂度（多 trait + 实现） |
| 符合开闭原则 | 动态分发轻微性能损失 |
| 易于测试（mock 策略） | 学习成本 |
| 运行时切换 + 组合性好 | |

---

## 性能对比

| 方案 | 初始化时间 | 分类时间 | 内存占用 | 代码复杂度 |
|------|-----------|----------|----------|-----------|
| 硬编码（当前） | 0 ns | ~50 ns | 0 | ⭐ |
| 配置文件 | ~100 μs | ~60 ns | ~1 KB | ⭐⭐ |
| Builder 模式 | ~10 μs | ~55 ns | ~500 B | ⭐⭐⭐ |
| 策略模式 | ~10 μs | ~60 ns | ~1 KB | ⭐⭐⭐⭐ |

---

## 决策：分阶段实施

### 第一阶段（当前）：保持硬编码

**理由**：简单、高效、易维护，当前分类规则相对稳定。

```rust
pub fn tier_of(id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
    // 保持当前硬编码逻辑
}
```

### 第二阶段（可选）：引入 Builder 模式

**时机**：需要支持不同项目的不同分类规则 / 分类规则频繁变更 / 需要更好的测试覆盖。

```rust
// 提供 Builder API
let classifier = TierClassifierBuilder::new()
    .add_family_suffix("flash", ModelTier::Light)
    .build();

// 同时保留默认实现
pub fn tier_of(id: &str, family: Option<&str>, cost: Option<&Cost>) -> ModelTier {
    TierClassifier::with_defaults().classify(id, family, cost)
}
```

### 第三阶段（未来）：支持配置文件

**时机**：需要非程序员修改规则 / 需要多环境配置 / 需要运行时更新。

```rust
let classifier = TierClassifierWithStrategy::with_config("tier-rules.toml")?;
```

---

## 实施计划

### 第一阶段（2h）

1. 保持现有硬编码逻辑，不修改 `tier_of()` 函数
2. 添加文档说明分类规则，列出支持的模型

### 第二阶段（4h，可选）

1. 定义 `Rule` trait，实现具体规则类型（FamilySuffixRule, IdKeywordRule, CostThresholdRule, SpecialPrefixRule）
2. 创建 `TierClassifierBuilder`
3. 使用 Builder 重构默认规则，保持 API 兼容
4. 单元测试每个规则 + 集成测试分类器

### 第三阶段（6h，可选）

1. 定义 TOML 格式，实现解析器，添加验证逻辑
2. 定义 `TierStrategy` trait，实现多种策略
3. 创建 `TierClassifierWithStrategy`
4. 编写使用指南，提供示例配置
