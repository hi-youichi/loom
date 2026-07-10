# Model Tier 独立 Crate 设计文档

> 将 model tier 相关逻辑提取为统一的 `model-tier` crate，供其他项目按需使用。

**创建时间**：2025-08-19｜**状态**：已决策

---

## 关键决策

| 决策 | 结果 | 文档 |
|------|------|------|
| `#[non_exhaustive]` | ❌ 不使用 | [详情](./decisions/non-exhaustive.md) |
| 自定义 Tier | ❌ 不支持 | [详情](./decisions/custom-tier.md) |
| 配置化分类规则 | 📋 分阶段 | [详情](./decisions/configurable-rules.md) |

**最终类型定义**：

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

---

## 文档索引

| 文档 | 内容 |
|------|------|
| [Tier → Model 解析 API](./tier-resolution.md) | tier → 具体模型名称的解析链（plan → spec → API）、TierPlan、TierResolver |
| [Crate 提取方案](./extraction-plan.md) | 架构图、模块划分、实施计划（4 阶段 / 5h）、技术细节、风险 |
| [决策一：non_exhaustive](./decisions/non-exhaustive.md) | 是否使用 `#[non_exhaustive]` — 方案对比与决策 |
| [决策二：自定义 Tier](./decisions/custom-tier.md) | 是否支持用户自定义 tier — 3 种方案对比 |
| [决策三：配置化规则](./decisions/configurable-rules.md) | 分类规则可配置化 — 配置文件 / Builder / 策略模式 |

---

## 实施路线图

| 阶段 | 内容 | 时间 | 状态 |
|------|------|------|------|
| 一 | 创建 `model-tier` crate，迁移核心逻辑，重构 `model-spec-core` | 5h | 📋 待实施 |
| 二 | Builder 模式支持自定义分类规则 | 4h | 🔮 可选 |
| 三 | TOML 配置文件 + 策略模式 | 6h | 🔮 可选 |
