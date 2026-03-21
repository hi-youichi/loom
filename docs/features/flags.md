# Feature Flags

Loom uses feature flags to provide optional functionality, allowing you to include only what you need. This keeps compile times fast and dependencies minimal. Loom 采用 feature flags 机制提供可选功能，让你只引入需要的组件，保持编译快速、依赖精简。

## Why Feature Flags / 为什么使用 Feature Flags

- **Optional dependencies**: Only compile what you need, reducing build time and binary size.
  只编译需要的部分，减少构建时间和二进制体积。
- **Storage backend selection**: Choose the right persistence layer for your use case.
  根据场景选择合适的持久化层。
- **Clean separation**: Features are opt-in, keeping the core lightweight.
  功能按需启用，保持核心轻量。

## Available Feature Flags / 可用的 Feature Flags

| Flag | Description | Dependencies | Use Case |
|------|-------------|--------------|----------|
| `lance` | LanceDB vector search persistent storage / LanceDB 向量搜索持久化存储 | `lancedb`, `arrow-array`, `arrow-schema` | Long-term memory with vector search capabilities / 带向量搜索的长期记忆 |

### `lance` - LanceDB Vector Store

Enables **LanceStore** for vector search in long-term memory. When disabled, vector store backends are not compiled. Other backends (e.g., `SqliteStore`, `InMemoryStore`) do not require this feature.

启用 **LanceStore** 用于长期记忆的向量搜索。禁用时，向量存储后端不会被编译。其他后端（如 `SqliteStore`、`InMemoryStore`）不需要此 feature。

**Dependencies introduced / 引入的依赖**:
- `lancedb` - LanceDB Rust SDK
- `arrow-array` - Apache Arrow array types
- `arrow-schema` - Apache Arrow schema definitions

## Usage in Cargo.toml / 在 Cargo.toml 中使用

Enable features in your `Cargo.toml`:

```toml
[dependencies]
loom = { version = "0.1.3", features = ["lance"] }
```

For multiple features (future-proofing):

```toml
[dependencies]
loom = { version = "0.1.3", features = ["lance", "future-feature"] }
```

## Default Features / 默认 Features

**Loom has no default features.** All features are opt-in, meaning you get a minimal build by default.

**Loom 没有默认 feature。** 所有功能都是按需启用，默认情况下获得最小化构建。

```toml
# This gives you the core Loom without any optional backends
# 这将获得核心 Loom，不包含任何可选后端
[dependencies]
loom = "0.1.3"
```

## Feature Combinations / Feature 组合

Features can be combined when multiple are available. Currently only `lance` exists, but the pattern supports future additions:

```toml
[dependencies]
loom = { version = "0.1.3", features = ["lance"] }
```

When more features are added, you can enable multiple:

```toml
# Example for future reference
[dependencies]
loom = { version = "0.1.3", features = ["lance", "redis", "postgres"] }
```

## When to Enable `lance` / 何时启用 `lance`

Enable the `lance` feature when you need:

以下场景需要启用 `lance` feature：

1. **Vector search for long-term memory** / **长期记忆的向量搜索**
   - Your agent needs to search through stored memories semantically
   - 你的 agent 需要语义搜索存储的记忆

2. **Persistent storage with semantic search** / **带语义搜索的持久化存储**
   - You want memories to persist across sessions with search capabilities
   - 你希望记忆在会话间持久化，并具备搜索能力

3. **Production agents with memory persistence** / **带记忆持久化的生产级 agent**
   - Building agents that need to recall and search past interactions
   - 构建需要回忆和搜索过去交互的 agent

4. **High-performance vector operations** / **高性能向量操作**
   - LanceDB provides efficient vector indexing and search
   - LanceDB 提供高效的向量索引和搜索

### When NOT to enable `lance` / 何时不启用 `lance`

- Simple stateless agents / 简单的无状态 agent
- Short-lived conversational bots / 短期对话机器人
- When `SqliteStore` or `InMemoryStore` meets your needs / 当 `SqliteStore` 或 `InMemoryStore` 满足需求时
- Minimizing dependencies in constrained environments / 在受限环境中最小化依赖

## Related Documentation / 相关文档

- [Configuration System](../guides/configuration.md) - Configuring Loom features at runtime
- [Core Concepts](../architecture/core-concepts.md) - Understanding Loom's architecture
