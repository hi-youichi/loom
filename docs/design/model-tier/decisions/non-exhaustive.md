# 决策一：`#[non_exhaustive]` 枚举设计

**决策**：❌ 不使用 `#[non_exhaustive]`

> 返回 [README](../README.md)

---

## 背景

当前 `ModelTier` 枚举使用了 `#[non_exhaustive]` 属性：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]  // ← 关键属性
pub enum ModelTier {
    #[default]
    None,
    Light,
    Standard,
    Strong,
}
```

需要决定：model-tier crate 提取后，是否保留此属性？

---

## `#[non_exhaustive]` 的含义

表示**枚举或结构体可以在未来添加新变体/字段，而不破坏现有代码的兼容性**。

**没有 `#[non_exhaustive]`**：如果库未来添加新变体 `Ultra`，使用方的 `match` 会编译失败。

```rust
// 使用方代码
match tier {
    ModelTier::None => ...,
    ModelTier::Light => ...,
    ModelTier::Standard => ...,
    ModelTier::Strong => ...,
    // ❌ 编译失败：error[E0004]: non-exhaustive patterns: `Ultra` not covered
}
```

**有 `#[non_exhaustive]`**：使用方必须添加 `_` 兜底分支，未来新增变体不会破坏编译。

```rust
match tier {
    ModelTier::None => ...,
    ModelTier::Light => ...,
    ModelTier::Standard => ...,
    ModelTier::Strong => ...,
    _ => ...,  // ← 必须添加
}
```

### 对序列化的影响

```rust
// 未知值（如未来版本的 "ultra"）
let tier: ModelTier = serde_json::from_str("\"ultra\"")?;
// ⚠️ 行为取决于实现：有 _ 分支则匹配兜底，否则反序列化失败
```

---

## 方案对比

### 方案 A：保留 `#[non_exhaustive]`（推荐用于通用库）

```rust
#[non_exhaustive]
pub enum ModelTier {
    #[default]
    None,
    Light,
    Standard,
    Strong,
}
```

| 优点 | 缺点 |
|------|------|
| 向后兼容，未来添加变体不破坏用户代码 | 使用方代码稍繁琐（必须 `_` 分支） |
| Rust 库设计最佳实践（如 `std::io::ErrorKind`） | 可能隐藏错误（新 tier 被静默忽略） |
| 支持扩展，无需担心 API 稳定性 | |

**适用**：长期维护的库、可能扩展的枚举、对外发布的 crate

### 方案 B：移除 `#[non_exhaustive]`

```rust
pub enum ModelTier {
    #[default]
    None,
    Light,
    Standard,
    Strong,
}
```

| 优点 | 缺点 |
|------|------|
| 代码简洁，不需要 `_` 分支 | 添加新变体是 breaking change |
| 编译时检查严格，新增变体会导致编译失败 | 违反语义版本，需 major 版本更新 |
| 用户必须处理所有已知变体 | |

**适用**：确定不会扩展的枚举、内部 crate

### 方案 C：条件编译

```rust
#[cfg_attr(feature = "non_exhaustive", non_exhaustive)]
pub enum ModelTier { ... }
```

```toml
[features]
default = ["non_exhaustive"]
non_exhaustive = []
```

| 优点 | 缺点 |
|------|------|
| 灵活选择 | 增加复杂度（feature flag 管理） |
| 支持渐进迁移 | 不一致性（不同用户行为不同） |

**适用**：过渡期的库

---

## 实际案例

### Rust 标准库

```rust
// std::io::ErrorKind 使用了 #[non_exhaustive]
#[non_exhaustive]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    // ...
    _Uncategorized,  // 内部使用，不对外暴露
}

// 使用方必须处理未知错误
match error.kind() {
    ErrorKind::NotFound => ...,
    _ => ...,
}
```

### serde

```rust
// serde 的 Content 没有使用 #[non_exhaustive]
// 因为它是 pub(crate) 内部类型，不需要扩展
pub(crate) enum Content {
    Bool(bool),
    U64(u64),
    I64(i64),
    // ...
}
```

---

## 决策：不使用 `#[non_exhaustive]`

**理由**：

1. **Tier 级别不会改变**：当前 4 个级别（None/Light/Standard/Strong）已经足够，不需要扩展性
2. **使用方代码更简洁**：不需要 `_` 分支，匹配更明确
3. **编译时检查更严格**：如果未来添加新变体，使用方代码会编译失败，可以及时发现问题
4. **减少维护成本**：不需要管理向后兼容性

**最终实现**：

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
