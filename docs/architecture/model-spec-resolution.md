# Model Specification Resolution

本文档描述 Loom 如何解析和获取 LLM 模型的规格信息（上下文限制、输出限制等），以及如何配置不同的数据源。

## 概述

Loom 通过统一的 `ModelLimitResolver` trait 支持多种模型规格数据源：

- **models.dev** - 远程 API 获取最新的模型规格
- **本地文件** - 从 JSON 文件读取自定义模型配置
- **配置覆盖** - 通过配置文件直接指定限制
- **组合解析** - 按优先级链式解析

## 架构设计

### 核心组件

```
┌─────────────────────────────────────────────────────────────┐
│                   ModelLimitResolver (Trait)                │
├─────────────────────────────────────────────────────────────┤
│  resolve(provider, model) -> Option<ModelSpec>              │
│  resolve_combined("provider/model") -> Option<ModelSpec>    │
└─────────────────────────────────────────────────────────────┘
                           ▲
        ┌──────────────────┼──────────────────┐
        │                  │                  │
   ┌────┴────┐      ┌─────┴──────┐      ┌────┴─────┐
   │ Cached  │      │ Composite  │      │ Config   │
   │Resolver │      │  Resolver  │      │ Override │
   └────┬────┘      └─────┬──────┘      └──────────┘
        │                 │
        │    ┌────────────┴────────────┐
        │    │                         │
   ┌────┴────┐                 ┌──────┴────────┐
   │Resolver │                 │   LocalFile   │
   │Refresher│                 │   Resolver    │
   └────┬────┘                 └───────────────┘
        │
   ┌────┴────┐
   │ModelsDev│
   │Resolver │◄────┐
   └────┬────┘     │
        │          │
   ┌────┴────┐     │
   │   HTTP  │─────┘
   │ Client  │ (定时刷新)
   └─────────┘
```

### 数据模型

```rust
pub struct ModelSpec {
    pub context_limit: u32,      // 上下文 token 限制（如 128000）
    pub output_limit: u32,       // 输出 token 限制（如 4096）
    pub cache_read: Option<u32>, // 可选：缓存读限制
    pub cache_write: Option<u32>, // 可选：缓存写限制
}
```

## 解析器实现

### 1. ModelsDevResolver

从 `https://models.dev/api.json` 获取模型规格。

**特性：**
- 自动重试机制（最多 3 次，带指数退避）
- 支持自定义 HTTP Client（便于测试）
- 响应缓存（通过 CachedResolver 包装）

**JSON 格式：**
```json
{
  "openai": {
    "models": {
      "gpt-4o": {
        "limit": {
          "context": 128000,
          "output": 4096
        }
      }
    }
  }
}
```

**使用示例：**
```rust
use loom::model_spec::ModelsDevResolver;

let resolver = ModelsDevResolver::new();
let spec = resolver.resolve_combined("openai/gpt-4o").await;
```

### 2. LocalFileResolver

从本地 JSON 文件读取模型规格，格式与 models.dev 兼容。

**配置示例：**
```json
{
  "zai": {
    "models": {
      "glm-5": {
        "limit": {
          "context": 204800,
          "output": 131072
        }
      }
    }
  }
}
```

### 3. CachedResolver

为任意解析器添加内存缓存，避免重复请求。

**特性：**
- 线程安全的 `RwLock<HashMap<String, ModelSpec>>` 缓存
- 支持手动刷新和后台定时刷新（ResolverRefresher）

```rust
use loom::model_spec::{CachedResolver, ModelsDevResolver};

let cached = CachedResolver::new(ModelsDevResolver::new());
// 首次调用会请求 API，后续从缓存返回
```

**手动刷新：**
```rust
// 获取所有模型规格并更新缓存
if let Ok(specs) = resolver.fetch_all().await {
    cached.refresh(specs).await;
}
```

### 4. ResolverRefresher

后台定时刷新缓存，适用于长生命周期服务。

**特性：**
- 使用 `tokio::time::interval` 实现定时任务
- 可配置刷新间隔（如每小时）
- 返回 `JoinHandle` 可用于优雅停止

```rust
use std::sync::Arc;
use std::time::Duration;
use loom::model_spec::{CachedResolver, ModelsDevResolver, ResolverRefresher};

let cached = Arc::new(CachedResolver::new(ModelsDevResolver::new()));
let refresher = ResolverRefresher::new(cached.clone(), Duration::from_secs(3600));

// 启动后台刷新任务
let handle = refresher.spawn();

// 服务停止时中断刷新
handle.abort();
```

### 5. CompositeResolver

按优先级链式组合多个解析器，返回第一个成功的结果。

**典型优先级：**
1. ConfigOverride（配置文件中手动指定）
2. LocalFileResolver（本地自定义配置）
3. CachedResolver<ModelsDevResolver>（远程 API + 缓存）

```rust
use loom::model_spec::{
    CompositeResolver, ConfigOverride, LocalFileResolver, 
    CachedResolver, ModelsDevResolver
};
use std::sync::Arc;

let composite = CompositeResolver::new(vec![
    Arc::new(ConfigOverride::new(100000)),
    Arc::new(LocalFileResolver::new("/path/to/models.json")),
    Arc::new(CachedResolver::new(ModelsDevResolver::new())),
]);
```

## 调用链路

### CLI 层（信息展示）

```
cli/src/run/agent.rs:166
    └─ print_model_info()
         └─ ModelsDevResolver::new()
              └─ resolve_combined("openai/gpt-4o")
                   ├─ Success: 打印 "model: openai/gpt-4o (128K context)"
                   └─ Failed:  打印 "model: openai/gpt-4o (context: unknown)"
```

**注意：** CLI 层仅用于展示，不影响实际运行。

### Runner 构建层（关键路径）

```
loom/src/agent/react/build/mod.rs:154
    └─ build_react_runner()
         └─ resolve_compaction_config()
              ├─ config.compaction_config 已设置？直接返回
              └─ 模型格式为 "provider/model"？
                   ├─ Yes: ModelsDevResolver::new()
                          └─ resolve_combined(model)
                               ├─ Success: CompactionConfig::with_max_context_tokens(limit)
                               └─ Failed: CompactionConfig::default()
                   └─ No: CompactionConfig::default()
```

**关键行为：**
- 只有模型 ID 包含 `/` 时才尝试从 models.dev 解析
- 解析失败时使用默认配置（不阻塞启动）
- 解析结果用于上下文压缩/裁剪的 token 限制

### LLM 客户端构建层

```
loom/src/agent/react/build/llm.rs
    └─ build_default_llm()
         ├─ model_entry_from_config() ──► 从 env/配置提取模型信息
         │                                    ├─ OPENAI_API_KEY
         │                                    ├─ OPENAI_BASE_URL
         │                                    └─ model name
         │
         └─ 根据 provider_type 构建对应客户端:
              ├─ "openai" ────────► ChatOpenAI::new()
              ├─ "openai_compat" ─► ChatOpenAICompat::new() [Zhipu/Kimi/DeepSeek/Ollama]
              └─ "bigmodel" ──────► ChatOpenAICompat::new() (Zhipu 默认)
```

**说明：** 
- LLM 客户端负责实际的模型调用（`invoke`, `invoke_stream`）
- ModelSpec Resolver 负责获取模型的 context/output 限制（用于压缩配置）
- 两者独立工作：可以成功构建 LLM 客户端但无法获取 ModelSpec（此时使用默认压缩配置）

## 配置指南

### 使用环境变量

```bash
# 自定义 models.dev API URL（默认: https://models.dev/api.json）
export MODELS_DEV_URL="https://custom.example.com/models.json"
```

### 使用本地配置文件

创建 `~/.config/loom/models.json`：

```json
{
  "my-provider": {
    "models": {
      "custom-model": {
        "limit": {
          "context": 32000,
          "output": 4096
        }
      }
    }
  }
}
```

### 在代码中配置

```rust
// 通过 CompactionConfig 直接指定
let config = CompactionConfig::with_max_context_tokens(128000)
    .with_max_messages(50);

// 传递给 ReactRunner
let runner = ReactRunner::new(
    llm,
    tool_source,
    checkpointer,
    store,
    runnable_config,
    system_prompt,
    approval_policy,
    Some(config),  // 使用自定义配置
    None,
    None,
    verbose,
    None,
)?;
```

## 错误处理

### 常见失败场景

| 场景 | 日志级别 | 说明 |
|------|---------|------|
| 模型 ID 无 provider 前缀 | debug | 如使用 "gpt-4o" 而非 "openai/gpt-4o" |
| 模型不在 models.dev 数据库 | debug | 新模型或小众提供商 |
| 网络请求失败 | debug | API 不可达或超时 |
| JSON 解析失败 | warning | 响应格式异常 |

### 降级策略

所有解析失败都优雅降级到 `CompactionConfig::default()`，不会阻塞 Agent 启动。

## 扩展开发

### 实现自定义 Resolver

```rust
use async_trait::async_trait;
use loom::model_spec::{ModelLimitResolver, ModelSpec};

pub struct CustomResolver;

#[async_trait]
impl ModelLimitResolver for CustomResolver {
    async fn resolve(&self, provider_id: &str, model_id: &str) -> Option<ModelSpec> {
        // 自定义解析逻辑
        Some(ModelSpec::new(128000, 4096))
    }
}
```

### 集成到 CompositeResolver

```rust
let composite = CompositeResolver::new(vec![
    Arc::new(CustomResolver),
    Arc::new(ModelsDevResolver::new()),
]);
```

## 性能优化

### 缓存策略建议

| 场景 | 推荐方案 |
|------|---------|
| 短生命周期应用 | 无需缓存，直接请求 |
| 长生命周期服务 | CachedResolver + ResolverRefresher（后台刷新） |
| 离线环境 | LocalFileResolver |
| 混合环境 | CompositeResolver[Local, CachedRemote] |

### 当前实现限制

- CLI 和 Runner 构建各自独立创建 Resolver，**不共享缓存**
- 每个进程启动时会发起一次 HTTP 请求（如果没有缓存命中）
- 建议在生产环境使用 `ResolverRefresher` 定期刷新缓存

## TODO / 未来改进

### [ ] 实现文件持久化缓存 (PersistentCacheResolver)

**问题：** 当前 `CachedResolver` 仅支持内存缓存，进程退出后数据丢失。每次启动都需要重新请求 models.dev API。

**方案：** 实现一个持久化缓存 Resolver，将 models.dev 数据写入本地文件：

```rust
pub struct PersistentCacheResolver<R> {
    inner: R,
    cache_file: PathBuf,
    // 内存缓存 + 文件持久化
}
```

**行为：**
1. 启动时优先从本地缓存文件读取
2. 缓存过期或不存在时，请求远程 API
3. 成功后更新内存缓存并写入文件
4. 可配置缓存 TTL（如 24 小时）

**收益：**
- 支持离线使用
- 减少启动时的网络依赖
- CLI 和 Runner 可共享同一份本地缓存

**临时替代方案：**
```bash
# 手动下载到本地文件
mkdir -p ~/.config/loom
curl https://models.dev/api.json > ~/.config/loom/models.json
```
然后在代码中使用 `LocalFileResolver`。

## 代码文件索引

### Model Spec 模块

| 文件路径 | 作用 | 关键组件 |
|---------|------|---------|
| `loom/src/model_spec/mod.rs` | 模块入口，导出所有公开类型 | 模块文档、re-exports |
| `loom/src/model_spec/spec.rs` | 数据模型定义 | `ModelSpec` struct |
| `loom/src/model_spec/resolver.rs` | Resolver trait 定义 | `ModelLimitResolver` trait |
| `loom/src/model_spec/models_dev.rs` | models.dev 远程 API 实现 | `ModelsDevResolver`, `HttpClient`, `ReqwestHttpClient`, `DEFAULT_MODELS_DEV_URL` |
| `loom/src/model_spec/local_file.rs` | 本地 JSON 文件解析 | `LocalFileResolver` |
| `loom/src/model_spec/cached.rs` | 内存缓存包装器 | `CachedResolver<R>` |
| `loom/src/model_spec/refresher.rs` | 后台定时刷新 | `ResolverRefresher` |
| `loom/src/model_spec/composite.rs` | 链式优先级解析 | `CompositeResolver` |
| `loom/src/model_spec/config_override.rs` | 配置直接覆盖 | `ConfigOverride` |
| `loom/src/agent/react/build/mod.rs` | Runner 构建时解析配置 | `resolve_compaction_config()`, `build_react_runner()` |
| `cli/src/run/agent.rs` | CLI 启动时打印模型信息 | `print_model_info()` |

### LLM 模块

| 文件路径 | 作用 | 关键组件 |
|---------|------|---------|
| `loom/src/llm/mod.rs` | LLM 核心 trait 与数据类型 | `LlmClient` trait, `LlmResponse`, `ModelInfo`, `ModelCapabilities`, `ToolChoiceMode` |
| `loom/src/llm/model_registry.rs` | 全局模型注册表 | `ModelRegistry` (带 5 分钟 TTL 缓存) |
| `loom/src/llm/model_cache.rs` | 模型列表缓存与获取 | `ModelCache`, `ModelFetcher` |
| `loom/src/llm/openai/mod.rs` | OpenAI 官方客户端 | `ChatOpenAI` (基于 async_openai) |
| `loom/src/llm/openai/models.rs` | OpenAI 模型列表查询 | `list_models()` |
| `loom/src/llm/openai_compat.rs` | OpenAI 兼容网关客户端 | `ChatOpenAICompat` (支持 Zhipu/Kimi/DeepSeek/Ollama) |
| `loom/src/llm/mock.rs` | 测试用 Mock | `MockLlm` |
| `loom/src/agent/react/build/llm.rs` | LLM 客户端构建 | `build_default_llm()`, `model_entry_from_config()` |
| `loom/src/lib.rs` | 库入口 | pub use model_spec::*, pub use llm::* |

## 相关文档

- [Compression & Memory Management](./compression.md) - 上下文压缩配置
- [Configuration](../guides/configuration.md) - HelveConfig 和 profiles
- [LLM Integration](../guides/llm-integration.md) - LLM 提供商配置
