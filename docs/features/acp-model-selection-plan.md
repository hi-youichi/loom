# ACP 模型选择功能 - 开发计划

## 概述

为 ACP 协议添加模型选择功能，允许 IDE 客户端（如 Zed、JetBrains）在创建会话时看到可用的模型列表，并允许用户在会话中切换模型。

## 当前状态

- `loom-acp/src/agent.rs` 已有 `build_model_config_options` 函数，但 `options` 数组为空
- `loom::llm::model_cache` 已有 `ModelCache` 和 `ModelFetcher` 缓存实现
- `config` 模块可以加载 provider 配置

## 开发阶段

### Phase 1: 基础模型列表功能 [预计 2-3 小时]

**目标**: 在 `session/new` 响应中返回可用的模型列表

**任务**:

1. **定义 ModelOption 结构体**
   - 文件: `loom-acp/src/agent.rs`
   - 字段: `id`, `name`, `provider`
   - 添加 Serialize/Deserialize

2. **添加 get_available_models 方法**
   - 复用 `loom::llm::ModelFetcher` 缓存
   - 从 `config::load_full_config` 获取 providers
   - 聚合所有 provider 的模型列表

3. **修改 build_model_config_options**
   - 接受 `Vec<ModelOption>` 参数
   - 填充 `options` 数组

4. **修改 new_session 方法**
   - 调用 `get_available_models`
   - 传递给 `build_model_config_options`

**验收标准**:
- [ ] `session/new` 响应的 `config_options[0].options` 包含模型列表
- [ ] 模型列表来自所有配置的 providers
- [ ] 缓存生效（5 分钟内不重复请求 API）

### Phase 2: 错误处理和降级 [预计 1 小时]

**目标**: 优雅处理 provider 不可用的情况

**任务**:

1. **Provider 失败降级**
   - 单个 provider 失败不影响其他 provider
   - 记录警告日志
   - 至少返回环境变量中的默认模型

2. **空列表兜底**
   - 如果所有 provider 都失败，返回默认模型选项
   - 默认: `MODEL` 环境变量或 `OPENAI_MODEL` 环境变量

**验收标准**:
- [ ] Provider API 不可用时不崩溃
- [ ] 无配置时至少显示默认模型
- [ ] 有明确的错误日志

### Phase 3: 会话级模型切换 [预计 1-2 小时]

**目标**: 允许用户在会话中切换模型

**任务**:

1. **验证 set_session_config_option**
   - 确认现有实现正确更新 `session_config.model`
   - 确认 `prompt` 时使用会话级模型

2. **响应中返回更新后的模型列表**
   - `set_session_config_option` 响应包含完整模型列表
   - 客户端可以刷新 UI

**验收标准**:
- [ ] `session/set_config_option` 正确更新模型
- [ ] 后续 `prompt` 使用新选择的模型
- [ ] 响应包含更新后的配置选项

### Phase 4: 测试和文档 [预计 1-2 小时]

**目标**: 确保功能稳定可靠

**任务**:

1. **单元测试**
   - 测试 `build_model_config_options` 序列化
   - 测试 `get_available_models` 聚合逻辑
   - 测试缓存行为

2. **集成测试**
   - Mock provider 测试完整流程
   - 测试错误场景

3. **更新文档**
   - 更新 `docs/guides/acp.md`
   - 更新 `loom-acp/src/lib.rs` 文档注释

**验收标准**:
- [ ] 所有测试通过
- [ ] 文档更新完成

## 技术细节

### 依赖关系

```
loom-acp
  ├── loom::llm::{ModelCache, ModelFetcher, ModelInfo}
  ├── config::{load_full_config, ProviderDef}
  └── agent_client_protocol (ACP 类型)
```

### 数据流

```
session/new 请求
    │
    ▼
LoomAcpAgent::new_session
    │
    ├─► get_available_models()
    │       │
    │       ├─► load_full_config("loom")
    │       │       │
    │       │       ▼
    │       │   providers: HashMap<String, ProviderDef>
    │       │
    │       ├─► ModelFetcher::list_all_models(&providers)
    │       │       │
    │       │       ├─► [缓存命中] 返回缓存
    │       │       │
    │       │       └─► [缓存未命中]
    │       │               │
    │       │               ▼
    │       │           fetch_provider_models() per provider
    │       │               │
    │       │               ▼
    │       │           存入 ModelCache (TTL 5min)
    │       │
    │       └─► 聚合为 Vec<ModelOption>
    │
    ├─► build_model_config_options(current_model, model_options)
    │       │
    │       └─► JSON 序列化为 SessionConfigOption
    │
    └─► NewSessionResponse { session_id, config_options }
```

### 关键代码变更

#### 1. 新增结构体 (`loom-acp/src/agent.rs`)

```rust
/// 模型选项，用于 ACP SessionConfigSelect
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelOption {
    /// 模型 ID (e.g., "gpt-4o", "claude-3-opus")
    id: String,
    /// 显示名称
    name: String,
    /// Provider 名称 (e.g., "openai", "anthropic")
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
}
```

#### 2. 新增字段 (`loom-acp/src/agent.rs`)

```rust
pub struct LoomAcpAgent {
    sessions: SessionStore,
    checkpointer: Arc<SqliteSaver<JsonSerializer>>,
    #[cfg(feature = "mcp")]
    mcp_manager: crate::mcp::McpManager,
    
    // 新增: 模型缓存服务
    model_fetcher: Arc<ModelFetcher>,
}
```

#### 3. 修改构造函数 (`loom-acp/src/agent.rs`)

```rust
impl LoomAcpAgent {
    pub fn new(checkpointer: Arc<SqliteSaver<JsonSerializer>>) -> Self {
        Self {
            sessions: SessionStore::default(),
            checkpointer,
            #[cfg(feature = "mcp")]
            mcp_manager: crate::mcp::McpManager::default(),
            model_fetcher: Arc::new(ModelFetcher::new(ModelCache::default())),
        }
    }
}
```

#### 4. 新增方法 (`loom-acp/src/agent.rs`)

```rust
impl LoomAcpAgent {
    /// 获取所有配置 provider 的可用模型列表
    async fn get_available_models(&self) -> Vec<ModelOption> {
        let config = match load_full_config("loom") {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to load config: {}", e);
                return self.get_default_model_options();
            }
        };
        
        if config.providers.is_empty() {
            return self.get_default_model_options();
        }
        
        let providers: Vec<_> = config.providers.into_iter().collect();
        
        match self.model_fetcher.list_all_models(&providers).await {
            Ok(models_by_provider) => {
                models_by_provider
                    .into_iter()
                    .flat_map(|(provider_name, provider_models)| {
                        provider_models.into_iter().map(move |m| ModelOption {
                            id: m.id.clone(),
                            name: m.id,
                            provider: Some(provider_name.clone()),
                        })
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Failed to fetch models: {}", e);
                self.get_default_model_options()
            }
        }
    }
    
    /// 获取默认模型选项（从环境变量）
    fn get_default_model_options(&self) -> Vec<ModelOption> {
        let default_model = std::env::var("MODEL")
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_default();
        
        if default_model.is_empty() {
            vec![]
        } else {
            vec![ModelOption {
                id: default_model.clone(),
                name: default_model,
                provider: None,
            }]
        }
    }
}
```

#### 5. 修改 build_model_config_options (`loom-acp/src/agent.rs`)

```rust
fn build_model_config_options(
    current_model: &str,
    model_options: Vec<ModelOption>,
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let options: Vec<_> = model_options
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": &m.id,
                "name": &m.name,
            })
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

#### 6. 修改 new_session (`loom-acp/src/agent.rs`)

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
    
    // 获取可用模型列表
    let model_options = self.get_available_models().await;
    
    let config_options = build_model_config_options(&current_model, model_options)
        .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?;
    
    Ok(NewSessionResponse::new(session_id).config_options(Some(config_options)))
}
```

## 测试计划

### 单元测试

#### 1. `build_model_config_options` 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_build_model_config_options_empty() {
        let options = vec![];
        let result = build_model_config_options("gpt-4o", options).unwrap();
        
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id.as_str(), "model");
        // options 数组应为空
    }
    
    #[test]
    fn test_build_model_config_options_with_models() {
        let options = vec![
            ModelOption { id: "gpt-4o".into(), name: "GPT-4o".into(), provider: "openai".into() },
            ModelOption { id: "gpt-4o-mini".into(), name: "GPT-4o Mini".into(), provider: "openai".into() },
        ];
        let result = build_model_config_options("gpt-4o", options).unwrap();
        
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].options.len(), 2);
        assert_eq!(result[0].current_value.as_str(), "gpt-4o");
    }
    
    #[test]
    fn test_build_model_config_options_current_value() {
        let options = vec![
            ModelOption { id: "gpt-4o".into(), name: "GPT-4o".into(), provider: "openai".into() },
        ];
        let result = build_model_config_options("gpt-4o", options).unwrap();
        
        assert_eq!(result[0].current_value.as_str(), "gpt-4o");
    }
}
```

#### 2. `ModelOption` 序列化测试

```rust
#[test]
fn test_model_option_serialization() {
    let opt = ModelOption {
        id: "gpt-4o".to_string(),
        name: "GPT-4o".to_string(),
        provider: "openai".to_string(),
    };
    
    let json = serde_json::to_string(&opt).unwrap();
    assert!(json.contains("\"id\":\"gpt-4o\""));
    
    let decoded: ModelOption = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, "gpt-4o");
}
```

### 集成测试

#### 1. `session/new` 返回模型列表

```rust
// loom-acp/tests/model_selection.rs

use loom_acp::LoomAcpAgent;
use agent_client_protocol::{NewSessionRequest, Agent};

#[tokio::test]
async fn test_new_session_returns_model_options() {
    // 需要 mock provider 或使用测试环境
    let agent = LoomAcpAgent::new(/* ... */);
    
    let request = NewSessionRequest::default();
    let response = agent.new_session(request).await.unwrap();
    
    let config_options = response.config_options.as_ref().unwrap();
    assert!(!config_options.is_empty());
    
    let model_option = &config_options[0];
    assert_eq!(model_option.id.as_str(), "model");
    // 如果有配置 provider，options 不应为空
    // assert!(!model_option.options.is_empty());
}
```

#### 2. `set_session_config_option` 更新模型

```rust
#[tokio::test]
async fn test_set_model_updates_session_config() {
    let agent = LoomAcpAgent::new(/* ... */);
    
    // 先创建 session
    let session = agent.new_session(NewSessionRequest::default()).await.unwrap();
    let session_id = session.session_id;
    
    // 设置模型
    let request = SetSessionConfigOptionRequest {
        session_id: session_id.clone(),
        config_id: "model".into(),
        value: serde_json::json!("claude-3-opus"),
    };
    let response = agent.set_session_config_option(request).await.unwrap();
    
    // 验证响应中的 currentValue 已更新
    let config_options = response.config_options.as_ref().unwrap();
    assert_eq!(
        config_options[0].current_value.as_str(),
        "claude-3-opus"
    );
}
```

#### 3. `prompt` 使用选中的模型

```rust
#[tokio::test]
async fn test_prompt_uses_selected_model() {
    // 需要 mock LLM client
    let agent = LoomAcpAgent::new_with_mock_llm(/* ... */);
    
    // 创建 session 并选择模型
    let session = agent.new_session(NewSessionRequest::default()).await.unwrap();
    
    agent.set_session_config_option(SetSessionConfigOptionRequest {
        session_id: session.session_id.clone(),
        config_id: "model".into(),
        value: serde_json::json!("gpt-4o-mini"),
    }).await.unwrap();
    
    // 发送 prompt
    let request = PromptRequest {
        session_id: session.session_id,
        content_blocks: vec![/* ... */],
    };
    let _response = agent.prompt(request).await.unwrap();
    
    // 验证 mock LLM 收到了正确的模型参数
    // assert_eq!(mock_llm.last_model_used(), "gpt-4o-mini");
}
```

### E2E 测试

#### 1. 完整流程测试

```rust
// loom-acp/tests/e2e/model_selection_e2e.rs

#[tokio::test]
#[ignore] // 需要真实 provider API
async fn test_full_model_selection_flow() {
    // 1. 启动 loom-acp 进程
    // 2. 发送 initialize
    // 3. 发送 session/new，验证 config_options 包含模型列表
    // 4. 发送 set_session_config_option 选择模型
    // 5. 发送 prompt，验证使用正确模型
}
```

### 测试数据

创建测试用的 provider 配置：

```toml
# tests/fixtures/test_config.toml

[[providers]]
name = "test-openai"
type = "openai"
base_url = "http://localhost:8080/v1"  # mock server
api_key = "test-key"

[[providers]]
name = "test-bigmodel"
type = "bigmodel"
base_url = "http://localhost:8081/v1"
api_key = "test-key"
```

### Mock Server

使用 `wiremock` 或 `mockito` 模拟 provider API：

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

async fn setup_mock_provider() -> MockServer {
    let server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id": "gpt-4o", "owned_by": "openai"},
                {"id": "gpt-4o-mini", "owned_by": "openai"},
            ]
        })))
        .mount(&server)
        .await;
    
    server
}
```

### 测试覆盖率目标

| 模块 | 目标覆盖率 |
|------|-----------|
| `build_model_config_options` | 100% |
| `get_available_models` | 80% |
| `new_session` (模型相关) | 80% |
| `set_session_config_option` | 90% |
| `prompt` (模型选择) | 70% |

### 测试执行命令

```bash
# 运行所有测试
cargo test -p loom-acp

# 运行特定测试
cargo test -p loom-acp test_build_model_config_options

# 运行 E2E 测试（需要环境配置）
cargo test -p loom-acp --ignored --test e2e

# 生成覆盖率报告
cargo llvm-cov -p loom-acp
```

## 风险和缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| Provider API 响应慢 | `session/new` 延迟 | 使用 ModelFetcher 缓存；考虑异步预热 |
| Provider 返回大量模型 | 响应体过大 | 限制显示数量（如前 50 个）；添加搜索过滤 |
| ACP 协议变更 | 兼容性问题 | 使用 serde_json 动态构建；关注协议版本 |

## 时间估算

| 阶段 | 时间 | 优先级 |
|------|------|--------|
| Phase 1: 基础功能 | 2-3 小时 | P0 |
| Phase 2: 错误处理 | 1 小时 | P0 |
| Phase 3: 模型切换 | 1-2 小时 | P1 |
| Phase 4: 测试文档 | 1-2 小时 | P1 |
| **总计** | **5-8 小时** | |

## 后续优化

1. **模型搜索/过滤**: 当模型列表很长时，支持客户端搜索
2. **模型分组**: 按 provider 分组显示
3. **模型元数据**: 显示 context_limit、capabilities 等
4. **预热缓存**: 启动时预先加载模型列表
5. **配置持久化**: 记住用户上次选择的模型
