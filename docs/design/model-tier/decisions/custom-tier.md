# 决策二：支持自定义 Tier 级别

**决策**：❌ 不支持自定义 Tier

> 返回 [README](../README.md)

---

## 背景

当前 `ModelTier` 是一个固定枚举（None/Light/Standard/Strong），但其他项目可能需要：

- **更细粒度的分级**：`Economy`, `Budget`, `Mid`, `Premium`, `Ultra`
- **特殊标记**：`Beta`, `Preview`, `Deprecated`
- **项目特定**：`Internal`, `External`, `Sandbox`

是否应该支持用户定义额外的 tier 级别？

---

## 方案对比

### 方案 A：固定枚举 + Custom(u8)（推荐用于通用库）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTier {
    #[default]
    None,
    Light,
    Standard,
    Strong,

    /// 用户自定义 tier（用于扩展）
    #[serde(rename = "custom:{0}")]
    Custom(u8),
}

impl ModelTier {
    // 预定义的自定义 tier 常量
    pub const ECONOMY: ModelTier = ModelTier::Custom(10);
    pub const PREMIUM: ModelTier = ModelTier::Custom(20);
    pub const ULTRA: ModelTier = ModelTier::Custom(30);
    pub const BETA: ModelTier = ModelTier::Custom(40);
    pub const PREVIEW: ModelTier = ModelTier::Custom(50);

    pub fn custom_value(&self) -> Option<u8> {
        match self {
            ModelTier::Custom(v) => Some(*v),
            _ => None,
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, ModelTier::Custom(_))
    }
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTier::None => write!(f, "none"),
            ModelTier::Light => write!(f, "light"),
            ModelTier::Standard => write!(f, "standard"),
            ModelTier::Strong => write!(f, "strong"),
            ModelTier::Custom(v) => write!(f, "custom:{}", v),
        }
    }
}
```

**使用示例**：
```rust
// 使用预定义的自定义 tier
let tier = ModelTier::ECONOMY;
assert_eq!(tier.to_string(), "custom:10");

// 使用任意自定义 tier
let tier = ModelTier::Custom(15);

// 序列化
let json = serde_json::to_string(&ModelTier::ECONOMY)?;
// → "\"custom:10\""

// 反序列化
let tier: ModelTier = serde_json::from_str("\"custom:10\"")?;
assert_eq!(tier, ModelTier::ECONOMY);
```

| 优点 | 缺点 |
|------|------|
| 向后兼容：现有代码无需修改 | `Custom(u8)` 的含义不够清晰 |
| 类型安全：编译时检查 | 匹配复杂：需要处理所有 Custom 变体 |
| 扩展性：支持无限自定义 tier | 命名冲突：不同项目可能使用相同值 |
| 栈上分配，比较高效 | |

### 方案 B：字符串枚举 + 常量

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelTier(String);

impl ModelTier {
    pub const NONE: &'static str = "none";
    pub const LIGHT: &'static str = "light";
    pub const STANDARD: &'static str = "standard";
    pub const STRONG: &'static str = "strong";

    pub fn new(name: impl Into<String>) -> Self { Self(name.into()) }
    pub fn custom(name: impl Into<String>) -> Self { Self(name.into()) }
    pub fn as_str(&self) -> &str { &self.0 }

    pub fn is_custom(&self) -> bool {
        !matches!(self.0.as_str(), "none" | "light" | "standard" | "strong")
    }
}

impl Default for ModelTier {
    fn default() -> Self { Self::new(Self::NONE) }
}
```

**使用示例**：
```rust
// 使用自定义 tier
let tier = ModelTier::custom("premium");
assert_eq!(tier.as_str(), "premium");
assert!(tier.is_custom());

// 从字符串转换
let tier: ModelTier = "ultra".into();
```

| 优点 | 缺点 |
|------|------|
| 完全灵活：支持任意名称 | 失去编译时检查 |
| 语义清晰：名称本身就是含义 | 不是真正的枚举，无法使用 `match` |
| 易于扩展 | 类型不安全 |

### 方案 C：泛型 Tier\<T\>

```rust
/// Tier 类型特征
pub trait TierType: Clone + Eq + Hash + Serialize + DeserializeOwned + Send + Sync {
    fn as_str(&self) -> &str;
    fn from_str(s: &str) -> Option<Self>;
}

/// 泛型 Model Tier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelTier<T: TierType = DefaultTier> {
    value: T,
}

// 默认 tier 类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DefaultTier {
    None,
    Light,
    Standard,
    Strong,
}

impl TierType for DefaultTier { ... }

// 扩展 tier 类型示例
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExtendedTier {
    Economy,
    Budget,
    Standard,
    Premium,
    Ultra,
}

impl TierType for ExtendedTier { ... }
```

| 优点 | 缺点 |
|------|------|
| 完全类型安全：编译时检查 | 增加复杂度：需要定义 trait |
| 最大灵活性：支持任意 tier 类型 | 泛型传播：使用泛型类型增加代码复杂度 |
| 零运行时开销：泛型单态化 | 不同类型的 tier 不能比较 |
| | 学习成本：用户需要理解泛型和 trait |

---

## 实际应用场景

### 场景 1：Loom 主项目

```rust
// 使用默认 tier
let tier = ModelTier::Standard;
```

### 场景 2：企业级项目（需要更细粒度）

```rust
// 方案 A：使用预定义的自定义 tier
let tier = ModelTier::ECONOMY;
let tier = ModelTier::PREMIUM;

// 或使用任意值
let tier = ModelTier::Custom(25);
```

### 场景 3：研究项目（需要实验性 tier）

```rust
// 方案 B：使用字符串方式
let tier = ModelTier::custom("experimental-v2");
```

---

## 决策：不支持自定义 Tier

**理由**：

1. **保持简单，避免过度设计**：当前 4 个级别已满足需求，不需要额外的复杂度
2. **避免滥用**：`Custom(u8)` 可能导致语义混乱（不同项目对相同值有不同理解）
3. **保持一致性**：所有使用 model-tier 的项目共享相同的级别定义，避免碎片化
4. **未来扩展性**：如果需要更多级别，可以在未来版本中通过修改源码正式添加新变体

**使用建议**：
```rust
// 使用固定的 4 个级别
let tier = ModelTier::Light;
let tier = ModelTier::Standard;
let tier = ModelTier::Strong;

// 如果需要更多级别，等待未来版本
// 或者通过配置化分类规则实现（见下文）
```
