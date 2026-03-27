# LLM 模型列表查询功能

本文档描述了如何在 Loom 中查询配置的 Provider 可用模型列表。

## 概述

Loom 支持通过 `/v1/models` endpoint 查询每个配置的 Provider 的可用模型列表。此功能通过以下组件实现：

- **`LlmClient::list_models()`** — LLM 客户端 trait 中的方法
- **`ModelCache`** — 带 TTL 的内存缓存
- **`loom models` CLI 命令** — 列出所有或特定 Provider 的模型

## 配置示例

在 `~/.loom/config.toml` 中配置多个 Provider：

```toml
[[providers]]
name = "openai"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
type = "openai"

[[providers]]
name = "bigmodel"
api_key = "xxx.yyy"
base_url = "https://open.bigmodel.cn/api/paas/v4"
type = "bigmodel"

[[providers]]
name = "local-ollama"
base_url = "http://localhost:11434/v1"
type = "openai"  # Ollama 兼容 OpenAI API

[default]
provider = "openai"
```

## CLI 使用

### 列出所有 Provider 的模型

```bash
loom models list
```

输出示例：
```
Provider: openai
--------------------------------------------------
  • gpt-4o
  • gpt-4o-mini
  • gpt-4-turbo
  • gpt-3.5-turbo
  ...

Provider: bigmodel
--------------------------------------------------
  • glm-4
  • glm-4-flash
  • glm-4-plus
  ...
```

### 列出特定 Provider 的模型

```bash
loom models show openai
```

### JSON 输出

```bash
loom models list --json
```

## API 使用

### 直接调用 LlmClient

```rust
use loom::llm::{ChatOpenAI, LlmClient};

async fn list_openai_models() -> Result<Vec<ModelInfo>, AgentError> {
    let client = ChatOpenAI::new("gpt-4");
    let models = client.list_models().await?;
    
    for model in models {
        println!("  - {}", model.id);
    }
    
    Ok(models)
}
```

### 使用 ModelCache

```rust
use loom::llm::{ModelCache, fetch_provider_models};
use std::time::Duration;

async fn list_with_cache() -> Result<(), AgentError> {
    let cache = ModelCache::new(Duration::from_secs(300)); // 5分钟 TTL
    
    // 第一次查询会调用 API
    let models = cache.get_or_fetch("openai", || {
        fetch_provider_models(
            Some("openai"),
            Some("https://api.openai.com/v1"),
            Some("sk-..."),
        )
    }).await?;
    
    // 第二次查询会使用缓存
    let models = cache.get_or_fetch("openai", || {
        fetch_provider_models(
            Some("openai"),
            Some("https://api.openai.com/v1"),
            Some("sk-..."),
        )
    }).await?;
    
    Ok(())
}
```

## 架构

```
┌─────────────────┐     ┌─────────────────┐
│   CLI Command   │────>│   ModelCache    │
│  models list    │     │   (5min TTL)    │
└─────────────────┘     └────────┬────────┘
                                 │
                    ┌────────────┼────────────┐
                    │            │            │
                    v            v            v
             ┌──────────┐ ┌──────────┐ ┌──────────┐
             │ OpenAI   │ │ BigModel │ │  Ollama  │
             │ Client   │ │  Client  │ │  Client  │
             └────┬─────┘ └────┬─────┘ └────┬─────┘
                  │            │            │
                  v            v            v
             /v1/models   /v1/models   /v1/models
```

## 支持的 Provider

| Provider | Type | 实现 |
|----------|------|------|
| OpenAI | `openai` | `ChatOpenAI` |
| 智谱 BigModel | `bigmodel` | `ChatBigModel` |
| Ollama | `openai` | `ChatOpenAI` |
| 其他 OpenAI 兼容 | `openai` | `ChatOpenAI` |

## 错误处理

如果 Provider 不支持 `/v1/models` endpoint，将返回错误：

```bash
$ loom models show unsupported-provider
Provider: unsupported-provider
Error: list_models request failed: 404 Not Found
```

## 缓存策略

- **TTL**: 默认 5 分钟
- **存储**: 内存（`RwLock<HashMap>`）
- **失效**: 可手动调用 `cache.invalidate()` 或等待 TTL 过期

## 相关文件

- `loom/src/llm/mod.rs` — `LlmClient` trait 和 `ModelInfo` 类型
- `loom/src/llm/model_cache.rs` — 缓存实现和辅助函数
- `loom/src/llm/openai.rs` — OpenAI `list_models()` 实现
- `loom/src/llm/bigmodel.rs` — BigModel `list_models()` 实现
- `cli/src/model_cmd.rs` — CLI 命令实现
