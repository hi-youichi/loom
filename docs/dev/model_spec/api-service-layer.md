# API 服务层架构

## 概述

API 服务层负责将内部的 Model 结构体转换为客户端可用的 API 响应格式，提供模型信息的查询和管理功能。

## 数据流架构

```
Model 结构体 (内部存储)
    ↓ 转换
ModelInfo 结构体 (API 响应)
    ↓ HTTP API
客户端请求
```

## 核心组件

### ModelService

**位置**: `loom/src/services/models.rs`

```rust
pub struct ModelService {
    models: HashMap<String, Model>,      // 内部存储 Model 结构体
    providers: HashMap<String, Provider>, // 提供商信息
}
```

**职责**:
- 管理模型数据的内部存储
- 提供模型查询和过滤功能
- 转换内部格式为 API 响应格式

### ModelInfo (API 响应格式)

**位置**: `loom/src/protocol/responses.rs`

```rust
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub capabilities: ModelCapabilities,
    pub provider: ProviderInfo,
    pub pricing: Option<PricingInfo>,
}
```

## 转换逻辑

### Model → ModelInfo 转换

**位置**: `loom/src/services/models.rs:90-96`

```rust
impl From<&Model> for ModelInfo {
    fn from(model: &Model) -> Self {
        ModelInfo {
            id: model.id.clone(),
            name: model.name.clone(),
            capabilities: extract_capabilities(model),
            provider: extract_provider_info(model),
            pricing: extract_pricing_info(model),
        }
    }
}
```

### 能力提取

```rust
fn extract_capabilities(model: &Model) -> ModelCapabilities {
    ModelCapabilities {
        attachment: model.attachment,
        reasoning: model.reasoning,
        tool_call: model.tool_call,
        temperature: model.temperature,
        structured_output: model.structured_output,
        modalities: model.modalities.clone(),
    }
}
```

## API 端点

### 获取所有模型

```rust
// GET /api/models
pub async fn get_models(
    service: Arc<ModelService>,
) -> Result<Json<Vec<ModelInfo>>, Error> {
    let models = service.get_all_models();
    Ok(Json(models))
}
```

### 获取单个模型

```rust
// GET /api/models/{model_id}
pub async fn get_model(
    Path(model_id): Path<String>,
    service: Arc<ModelService>,
) -> Result<Json<ModelInfo>, Error> {
    match service.get_model_info(&model_id) {
        Some(info) => Ok(Json(info)),
        None => Err(Error::NotFound),
    }
}
```

### 搜索模型

```rust
// GET /api/models/search?q={query}
pub async fn search_models(
    Query(params): Query<SearchParams>,
    service: Arc<ModelService>,
) -> Result<Json<Vec<ModelInfo>>, Error> {
    let results = service.search_models(&params.q);
    Ok(Json(results))
}
```

## 服务实现

### 模型查询

```rust
impl ModelService {
    pub fn get_all_models(&self) -> Vec<ModelInfo> {
        self.models.values().map(|m| m.into()).collect()
    }
    
    pub fn get_model_info(&self, model_id: &str) -> Option<ModelInfo> {
        self.models.get(model_id).map(|m| m.into())
    }
    
    pub fn search_models(&self, query: &str) -> Vec<ModelInfo> {
        self.models.values()
            .filter(|m| m.name.contains(query) || m.id.contains(query))
            .map(|m| m.into())
            .collect()
    }
}
```

### 提供商信息提取

```rust
fn extract_provider_info(model: &Model) -> ProviderInfo {
    // 从 model.id 提取提供商信息
    let provider_id = extract_provider_from_model_id(&model.id);
    
    ProviderInfo {
        id: provider_id,
        name: get_provider_name(&provider_id),
        // ... 其他提供商信息
    }
}
```

## 错误处理

### 错误类型

```rust
pub enum ModelServiceError {
    ModelNotFound(String),
    InvalidModelId(String),
    ServiceUnavailable,
}
```

### 错误响应

```rust
impl IntoResponse for ModelServiceError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ModelServiceError::ModelNotFound(id) => 
                (StatusCode::NOT_FOUND, format!("Model not found: {}", id)),
            ModelServiceError::InvalidModelId(id) => 
                (StatusCode::BAD_REQUEST, format!("Invalid model ID: {}", id)),
            ModelServiceError::ServiceUnavailable => 
                (StatusCode::SERVICE_UNAVAILABLE, "Service unavailable".to_string()),
        };
        
        (status, Json(json!({ "error": message }))).into_response()
    }
}
```

## 性能优化

### 缓存策略

- **内存缓存**: ModelService 内部使用 HashMap 快速查找
- **查询优化**: 支持索引和搜索优化
- **懒加载**: 按需加载模型信息

### 并发处理

```rust
pub struct ModelService {
    models: Arc<RwLock<HashMap<String, Model>>>,
    providers: Arc<RwLock<HashMap<String, Provider>>>,
}
```

## 监控和日志

### 指标收集

- 查询响应时间
- 模型数量统计
- 错误率监控

### 日志记录

```rust
tracing::info!("Serving {} models", models.len());
tracing::debug!("Model query: {}", model_id);
```

## 相关文件

- `loom/src/services/models.rs`: ModelService 实现
- `loom/src/protocol/responses.rs`: API 响应格式定义
- `loom/src/model_spec/spec.rs`: Model 结构体重新导出
- `serve/src/routes/models.rs`: API 端点路由