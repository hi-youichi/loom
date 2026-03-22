# ACP 协议模型选择功能

## 概述

本文档描述如何在 ACP (Agent Client Protocol) 中实现模型选择功能，允许 IDE 客户端（如 Zed、JetBrains）动态选择 LLM 模型。

## 当前状态

### 已有基础设施

1. **模型配置选项** - `loom-acp/src/agent.rs:530-545` 已定义 `model` 配置选项，但 `options` 数组为空
2. **模型列表查询** - `loom::llm::fetch_provider_models` 可从 provider 获取可用模型
3. **模型缓存** - `loom::llm::model_cache::ModelCache` 已实现带 TTL 的缓存（默认 5 分钟）
4. **配置加载** - `config::load_full_config` 可获取配置的 providers

### 问题

`fetch_provider_models` 函数本身不使用缓存，每次调用都会请求 provider API。

## 实现方案

### 1. 数据结构

```rust
// loom-acp/src/agent.rs

use serde::Serialize;

/// 模型选项，用于 ACP SessionConfigSelect
#[derive(Debug, Clone, Serialize)]
struct ModelOption {
    /// 模型 ID（如 "gpt-4o"）
    id: String,
    /// 显示名称
    name: String,
    /// Provider 名称（可选，用于分组显示）
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
}
```

### 2. 修改 LoomAcpAgent 结构

```rust
// loom-acp/src/agent.rs

use loom::llm::{ModelCache, ModelFetcher};
use std::sync::Arc;

pub struct LoomAcpAgent {
    checkpointer: Arc<Checkpointer<SqliteSaver>>,
    sessions: SessionStore,
    /// 模型缓存服务（复用 loom::llm 的缓存基础设施）
    model_fetcher: Arc<ModelFetcher>,
}
```

### 3. 修改构造函数

```rust
impl LoomAcpAgent {
    pub fn new(checkpointer: Arc<Checkpointer<SqliteSaver>>) -> Self {
        let sessions = SessionStore::default();
        let cache = ModelCache::default(); // TTL 5 分钟
        let model_fetcher = Arc::new(ModelFetcher::new(cache));
        
        Self {
            checkpointer,
            sessions,
            model_fetcher,
        }
    }
}
```

### 4. 实现模型获取方法

```rust
impl LoomAcpAgent {
    /// 获取所有配置 provider 的可用模型列表（带缓存）
    async fn get_available_models(&self) -> Vec<ModelOption> {
        let config = match load_full_config("loom") {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        
        if config.providers.is_empty() {
            return vec![];
        }
        
        let mut all_models = vec![];
        
        for (name, provider) in config.providers {
            // 使用缓存获取模型列表
            let result = self.model_fetcher
                .cache
                .get_or_fetch(&name, || {
                    let provider = provider.clone();
                    async move {
                        fetch_provider_models(
                            Some(&provider.r#type),
                            Some(&provider.base_url),
                            Some(&provider.api_key),
                        ).await
                    }
                })
                .await;
            
            if let Ok(models) = result {
                for model in models {
                    all_models.push(ModelOption {
                        id: model.id.clone(),
                        name: model.id,
                        provider: Some(name.clone()),
                    });
                }
            }
        }
        
        all_models
    }
}
```

### 5. 修改 new_session 方法

```rust
async fn new_session(
    &self,
    args: NewSessionRequest,
) -> agent_client_protocol::Result<NewSessionResponse> {
    crate::logging::init_with_working_folder(&args.cwd);

    let working_directory = Some(args.cwd.clone());
    let our_id = self.sessions.create(working_directory);
    let session_id = SessionId::new(our_id.as_str().to_string());
    
    let current_model = std::env::var("MODEL")
        .unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_default());
    
    // 动态获取模型列表（带缓存）
    let model_options = self.get_available_models().await;
    let config_options = build_model_config_options(&current_model, model_options)
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
    
    Ok(NewSessionResponse::new(session_id).config_options(Some(config_options)))
}
```

### 6. 修改 build_model_config_options 函数

```rust
/// Build config_options array with model selection (protocol types are non_exhaustive, so we construct via serde).
/// SessionConfigOption has kind flattened; SessionConfigKind uses tag "type" → "type": "select" and SessionConfigSelect fields at top level.
fn build_model_config_options(
    current_model: &str,
    model_options: Vec<ModelOption>,
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let options: Vec<_> = model_options
        .iter()
        .map(|m| {
            if let Some(ref provider) = m.provider {
                serde_json::json!({
                    "id": &m.id,
                    "name": format!("{} ({})", &m.name, provider),
                })
            } else {
                serde_json::json!({
                    "id": &m.id,
                    "name": &m.name,
                })
            }
        })
        .collect();
    
    let json = serde_json::json!([
        {
            "id": "model",
            "name": "Model",
            "description": "LLM model for this session.",
            "category": "model",
            "type": "select",
            "currentValue": current_model,
            "options": options
        }
    ]);
    serde_json::from_value(json)
}
```

### 7. 同步修改 set_session_config_option_response

```rust
/// Build SetSessionConfigOptionResponse with updated model option.
fn build_set_session_config_option_response(
    current_model: &str,
    model_options: Vec<ModelOption>,
) -> Result<SetSessionConfigOptionResponse, serde_json::Error> {
    let config_options = build_model_config_options(current_model, model_options)?;
    let json = serde_json::json!({
        "config_options": config_options,
        "meta": None::<()>
    });
    serde_json::from_value(json)
}
```

## 实现步骤

1. **添加 `ModelOption` 结构体** - 在 `loom-acp/src/agent.rs`
2. **添加 `model_fetcher` 字段** - 修改 `LoomAcpAgent` 结构
3. **修改构造函数** - 初始化 `ModelFetcher`
4. **实现 `get_available_models`** - 使用缓存获取模型列表
5. **修改 `build_model_config_options`** - 接受 `model_options` 参数
6. **修改 `new_session`** - 调用新方法
7. **修改 `set_session_config_option`** - 返回更新后的选项列表

## 缓存策略

复用 `loom::llm::ModelCache`：

- **TTL**: 5 分钟（`DEFAULT_CACHE_TTL`）
- **Key**: Provider 名称
- **并发**: 使用 `RwLock` 支持并发读取
- **失效**: TTL 过期后自动重新获取

## 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_model_config_options_not_empty() {
        let agent = LoomAcpAgent::new(/* ... */);
        let models = agent.get_available_models().await;
        
        // 如果配置了 provider，应该返回模型列表
        // 注意：这个测试需要 mock 或真实的 provider 配置
    }
    
    #[test]
    fn test_build_model_config_options() {
        let options = vec![
            ModelOption {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: Some("openai".to_string()),
            },
            ModelOption {
                id: "claude-3".to_string(),
                name: "Claude 3".to_string(),
                provider: Some("anthropic".to_string()),
            },
        ];
        
        let result = build_model_config_options("gpt-4o", options).unwrap();
        assert_eq!(result.len(), 1);
        // 验证 options 数组
    }
}
```

## 相关文件

### 需要修改

- `loom-acp/src/agent.rs` - 主要实现

### 依赖的现有模块

- `loom::llm::{ModelCache, ModelFetcher, fetch_provider_models}`
- `config::{load_full_config, ProviderDef}`

## ACP 协议兼容性

根据 ACP 协议，`SessionConfigSelect` 类型的配置选项结构：

```json
{
  "id": "model",
  "name": "Model",
  "description": "LLM model for this session.",
  "category": "model",
  "type": "select",
  "currentValue": "gpt-4o",
  "options": [
    {"id": "gpt-4o", "name": "GPT-4o (openai)"},
    {"id": "gpt-4o-mini", "name": "GPT-4o Mini (openai)"}
  ]
}
```

客户端（IDE）会：
1. 调用 `session/new` 获取 `config_options`
2. 显示模型选择 UI
3. 用户选择后调用 `session/set_config_option` 更新
4. 后续 `session/prompt` 使用选中的模型
