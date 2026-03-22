# Dependencies

Loom follows a minimalist dependency philosophy, introducing only necessary libraries to keep the project lightweight and maintainable. Optional dependencies are managed through feature flags, ensuring you only include what you need.

Loom 遵循最小化依赖哲学，只引入必需的库，保持项目轻量和可维护。可选依赖通过 feature flags 管理，确保只包含所需组件。

## Core Dependencies

### Asynchronous Runtime & Concurrency

| Dependency | Version | Purpose |
|------------|---------|---------|
| tokio | 1.0 | Async runtime with full features |
| async-trait | 0.1 | Async trait support |
| futures | 0.3 | Future combinators and utilities |
| tokio-stream | 0.1 | Stream utilities with sync support |
| tokio-util | - | Additional tokio utilities |
| futures-util | - | Future utility combinators |
| crossbeam-channel | - | Multi-producer multi-consumer channels |

### LLM Integration

| Dependency | Version | Purpose |
|------------|---------|---------|
| async-openai | 0.32 | OpenAI API client with chat-completion and embedding support |

### Data Serialization

| Dependency | Version | Purpose |
|------------|---------|---------|
| serde | 1.0 | Serialization framework (with derive feature) |
| serde_json | 1.0 | JSON serialization/deserialization |
| serde_yaml | 0.9 | YAML serialization/deserialization |

### Data Storage

| Dependency | Version | Purpose |
|------------|---------|---------|
| rusqlite | 0.31 | SQLite database with bundled feature |
| sqlite-vec | 0.1 | Vector storage extension for SQLite |

### MCP Protocol

| Dependency | Version | Purpose |
|------------|---------|---------|
| mcp_client | git | MCP protocol client implementation |
| mcp_core | git | MCP protocol core types and utilities |

### HTTP & Networking

| Dependency | Version | Purpose |
|------------|---------|---------|
| reqwest | 0.12 | HTTP client with JSON support |

### File System & Search

| Dependency | Version | Purpose |
|------------|---------|---------|
| glob | 0.3 | Pattern-based file matching |
| walkdir | 2 | Directory traversal |
| grep-regex | 0.1 | Regex-based search |
| grep-searcher | 0.1 | Search execution engine |
| ignore | 0.4 | gitignore-style pattern matching |
| regex | 1 | Regular expression engine |

### Utilities

| Dependency | Version | Purpose |
|------------|---------|---------|
| thiserror | 1.0 | Derive macro for error types |
| clap | 4 | Command-line argument parser with derive and env features |
| dotenv | 0.15 | Environment variable loader |
| dashmap | - | Concurrent HashMap |
| once_cell | - | Lazy static initialization |
| tracing | - | Structured logging and diagnostics |
| chrono | - | Date and time utilities |

## Optional Dependencies

Optional dependencies are managed through feature flags, allowing you to include additional functionality only when needed.

### LanceDB Support (`lance` feature)

```toml
[features]
lance = ["lancedb", "arrow-array", "arrow-schema"]
```

| Dependency | Version | Purpose |
|------------|---------|---------|
| lancedb | 0.23 | LanceDB vector database client (optional) |
| arrow-array | 56.2 | Apache Arrow array types (optional) |
| arrow-schema | 56.2 | Apache Arrow schema types (optional) |

To enable LanceDB support:

```toml
[dependencies]
loom = { version = "0.1", features = ["lance"] }
```

## Workspace Dependency Sharing

Loom uses Cargo workspace to share dependencies across multiple crates. Common dependencies are defined in the root `Cargo.toml` under `[workspace.dependencies]`, ensuring version consistency and reducing duplication.

Loom 使用 Cargo workspace 在多个 crate 之间共享依赖。通用依赖在根 `Cargo.toml` 的 `[workspace.dependencies]` 中定义，确保版本一致性并减少重复。

### Workspace Dependencies

```toml
[workspace.dependencies]
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
thiserror = "1.0"
dotenv = "0.15"
futures = "0.3"
clap = { version = "4", features = ["derive", "env"] }
tokio-stream = { version = "0.1", features = ["sync"] }
```

### Benefits of Workspace Dependencies

- **Version Consistency**: All crates use the same dependency version
- **Reduced Duplication**: Dependencies are resolved once for the entire workspace
- **Simplified Updates**: Update a dependency version in one place
- **Faster Builds**: Shared dependencies are compiled once

## Dependency Version Strategy

Loom follows these principles for dependency management:

### Version Management

- Use `[workspace.dependencies]` for unified version control
- Specify explicit versions for all dependencies
- Use version ranges cautiously, preferring specific versions for stability

### Version Specification

```toml
# Recommended: Specific version
tokio = "1.0"

# Avoid: Broad ranges (unless necessary)
# tokio = ">=1.0, <2.0"
```

### Feature Flags

Control which features are enabled to minimize dependencies:

```toml
# Good: Only enable needed features
tokio = { version = "1.0", features = ["rt-multi-thread", "macros"] }

# Avoid: Using all features when not needed
# tokio = { version = "1.0", features = ["full"] }
```

## Minimizing Dependencies

Loom employs several strategies to keep dependencies minimal:

### Disable Default Features

Many crates include default features you may not need. Disable them and only enable what you use:

```toml
[dependencies]
# Disable all default features
serde = { version = "1.0", default-features = false, features = ["derive"] }

# Only enable specific features
tokio = { version = "1.0", default-features = false, features = ["rt-multi-thread", "macros"] }
```

### Feature-Specific Dependencies

Use feature flags to make heavy dependencies optional:

```toml
[features]
# Optional LanceDB support
lance = ["lancedb", "arrow-array", "arrow-schema"]

[dependencies]
lancedb = { version = "0.23", optional = true }
arrow-array = { version = "56.2", optional = true }
arrow-schema = { version = "56.2", optional = true }
```

### Dependency Audit

Regularly audit dependencies to remove unused ones:

```bash
# Install cargo-udeps
cargo install cargo-udeps

# Check for unused dependencies
cargo +nightly udeps
```

### Best Practices

1. **Use workspace dependencies** for shared libraries
2. **Disable default features** when possible
3. **Enable only needed features** for each dependency
4. **Make optional functionality** behind feature flags
5. **Regularly audit** and remove unused dependencies
6. **Prefer pure-Rust implementations** when performance is acceptable
7. **Avoid duplicate functionality** - choose one library per use case

## Summary

Loom's dependency strategy focuses on:

- **Minimalism**: Only include what's necessary
- **Flexibility**: Optional features through feature flags
- **Consistency**: Workspace-managed versions
- **Efficiency**: Disabled default features and selective feature enabling

This approach keeps Loom lightweight, fast to compile, and easy to maintain while providing all the functionality needed for AI-powered code assistance.
