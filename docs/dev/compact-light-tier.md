# TitleNode 使用 Light Tier 模型

**Status**: Draft
**Date**: 2025-08-19

---

## 1. 现状

`TitleNode` 在 `runner.rs:87` 构造时复用主模型 `retry_llm`：

```rust
let title_node = TitleNode::new(Arc::clone(&retry_llm));
```

摘要生成是一个简单的短文本任务（50 字以内一句话），用主模型浪费成本。

---

## 2. 方案

### 2.1 改动范围

只改 **1 个文件**：`loom/src/agent/react/runner/runner.rs`

在 `new()` 中，为 `TitleNode` 传入一个独立的 Light tier LLM client，其余不变。

### 2.2 具体变更

**文件**: `loom/src/agent/react/runner/runner.rs`

`new()` 签名新增 `title_llm: Option<Arc<dyn LlmClient>>`：

```rust
pub fn new(
    llm: Box<dyn LlmClient>,
    // ... 现有参数不变 ...
    title_llm: Option<Arc<dyn LlmClient>>,  // 新增
) -> Result<Self, CompilationError> {
```

构造 TitleNode 时：

```rust
let title_node = TitleNode::new(
    title_llm.unwrap_or_else(|| Arc::clone(&retry_llm))
);
```

`CompletionCheckNode` 保持使用 `retry_llm` 不变。

### 2.3 调用方传入 Light tier client

涉及两个调用方：

**1. `loom/src/agent/react/build/mod.rs`**

`build_react_runner()` 已持有 providers 和 model_id 信息。在调用 `ReactRunner::new()` 前，解析 Light tier client：

```rust
let title_llm = resolve_title_llm(&config).await;

let runner = ReactRunner::new(
    llm,
    // ... 现有参数 ...
    Some(title_llm),
)?;
```

提取工具函数（放在 `build/llm.rs`）：

```rust
pub(crate) async fn resolve_title_llm(
    config: &ReactBuildConfig,
) -> Option<Arc<dyn LlmClient>> {
    let providers = config.providers.as_ref()?;
    let model_id = config.model.as_deref()?;
    let entry = ModelRegistry::global()
        .resolve_tier_for_model(model_id, ModelTier::Light, providers)
        .await?;
    create_llm_client(&entry).ok().map(|c| Arc::from(c))
}
```

**2. `loom/src/cli_run/agent.rs`**

同理，在构建 runner 前解析 Light tier client 并传入。

### 2.4 Fallback 行为

- `resolve_tier_for_model` 返回 `None`（无 Light tier 模型）→ `None` → runner 中 fallback 到 `retry_llm`
- `create_llm_client` 失败 → `None` → fallback 到 `retry_llm`
- 调用方不传 providers → `None` → fallback 到 `retry_llm`

无需额外 fallback 逻辑，完全向后兼容。

---

## 3. 文件变更清单

| 文件 | 变更 |
|------|------|
| `loom/src/agent/react/runner/runner.rs` | `new()` 签名加 `title_llm` 参数 |
| `loom/src/agent/react/build/llm.rs` | 新增 `resolve_title_llm()` |
| `loom/src/agent/react/build/mod.rs` | 调用 `resolve_title_llm`，传入 `new()` |
| `loom/src/cli_run/agent.rs` | 同上 |

`TitleNode` 本身 **不需要修改**，它已通过 `Arc<dyn LlmClient>` 抽象。

---

## 4. 测试

1. `resolve_title_llm` 单测 — 有/无 Light tier 模型、create 失败
2. `ReactRunner::new` 传入 `Some(light_client)` — 验证 TitleNode 持有的是 light client
3. `ReactRunner::new` 传入 `None` — 保持现有行为
