# 数据流和转换架构

## 概述

Model 结构体在整个系统中有复杂的数据流动和转换过程，涉及从外部数据源获取、内部存储、到 API 响应的完整生命周期。

## 完整数据流架构

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   models.dev    │    │  本地配置文件    │    │   缓存存储      │
│      API        │    │                  │    │                 │
└────────┬────────┘    └────────┬─────────┘    └────────┬────────┘
         │                      │                       │
         └──────────────────────┼───────────────────────┘
                                │
                                ▼
                    ┌─────────────────────┐
                    │  ModelLimitResolver │
                    │    (复合解析器)     │
                    └──────────┬──────────┘
                               │
                               ▼
                    ┌─────────────────────┐
                    │   ModelService      │
                    │  (内部存储)         │
                    └──────────┬──────────┘
                               │
                ┌──────────────┼──────────────┐
                │              │              │
                ▼              ▼              ▼
        ┌──────────┐  ┌──────────┐  ┌──────────┐
        │ API 响应  │  │ CLI 命令  │  │ 内部逻辑  │
        │ ModelInfo│  │ ModelInfo│  │  Model   │
        └──────────┘  └──────────┘  └──────────┘
```

## 1. 数据获取阶段

### models.dev 数据源

**位置**: `loom/src/model_spec/models_dev.rs`

```rust
// 数据获取流程
pub async fn fetch_models_from_dev() -> Result<Vec<Model>, Error> {
    // 1. HTTP 请求 models.dev API
    let response = client.get("https://models.dev/api/v1/models").await?;
    
    // 2. JSON 解析
    let json: serde_json::Value = serde_json::from_str(&response)?;
    
    // 3. 转换为 Model 结构体
    json["models"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(parse_model)  // 核心解析函数
        .collect()
}
```

### 本地配置文件

**位置**: `loom/src/model_spec/local_file.rs`

```rust
pub struct LocalFileResolver {
    path: PathBuf,
}

impl LocalFileResolver {
    pub fn load_models(&self) -> Result<Vec<Model>, Error> {
        let content = fs::read_to_string(&self.path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;
        // 解析逻辑与 models.dev 相同
        parse_models_from_json(&json)
    }
}
```

### 缓存层

**位置**: `loom/src/model_spec/cached.rs`

```rust
pub struct CachedResolver<R: ModelLimitResolver> {
    inner: R,
    cache: Arc<RwLock<HashMap<String, Model>>>,
}

impl<R: ModelLimitResolver> CachedResolver<R> {
    pub async fn fetch_all(&self) -> Result<Vec<Model>, Error> {
        // 先检查缓存
        if let Some(cached) = self.get_cached().await {
            return Ok(cached);
        }
        
        // 缓存未命中，从底层解析器获取
        let models = self.inner.fetch_all().await?;
        self.update_cache(models.clone()).await;
        Ok(models)
    }
}
```

## 2. 解析和转换阶段

### JSON 解析函数

**位置**: `model-spec-core/src/parser.rs`

```rust
pub fn parse_model(json: &serde_json::Value) -> Result<Model, ParseError> {
    Ok(Model {
        id: json["id"]
            .as_str()
            .ok_or(ParseError::MissingField("id".to_string()))?
            .to_string(),
        name: json["name"]
            .as_str()
            .ok_or(ParseError::MissingField("name".to_string()))?
            .to_string(),
        family: json["family"].as_str().map(|s| s.to_string()),
        attachment: json["attachment"].as_bool().unwrap_or(false),
        reasoning: json["reasoning"].as_bool().unwrap_or(false),
        tool_call: json["tool_call"].as_bool().unwrap_or(false),
        temperature: json["temperature"].as_bool().unwrap_or(true),
        structured_output: json["structured_output"].as_bool(),
        knowledge: json["knowledge"].as_str().map(|s| s.to_string()),
        release_date: json["release_date"].as_str().map(|s| s.to_string()),
        last_updated: json["last_updated"].as_str().map(|s| s.to_string()),
        modalities: parse_modalities(&json["modalities"])?,
        open_weights: json["open_weights"].as_bool().unwrap_or(false),
        cost: parse_cost(&json["cost"])?,
        limit: parse_model_limit(&json["limit"])?,
    })
}
```

### 数据验证

```rust
impl Model {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id.is_empty() {
            return Err(ValidationError::EmptyId);
        }
        if self.name.is_empty() {
            return Err(ValidationError::EmptyName);
        }
        // 验证其他字段的合理性
        Ok(())
    }
}
```

## 3. 内部存储阶段

### ModelService 存储

**位置**: `loom/src/services/models.rs`

```rust
pub struct ModelService {
    models: HashMap<String, Model>,
    providers: HashMap<String, Provider>,
}

impl ModelService {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
            providers: HashMap::new(),
        }
    }
    
    pub fn add_model(&mut self, model: Model) {
        self.models.insert(model.id.clone(), model);
    }
    
    pub fn add_models(&mut self, models: Vec<Model>) {
        for model in models {
            self.add_model(model);
        }
    }
}
```

### 提供商信息提取

```rust
impl ModelService {
    fn extract_providers(&mut self, models: &[Model]) {
        for model in models {
            let provider_id = extract_provider_from_model_id(&model.id);
            if !self.providers.contains_key(&provider_id) {
                if let Some(provider) = fetch_provider_info(&provider_id) {
                    self.providers.insert(provider_id, provider);
                }
            }
        }
    }
}
```

## 4. API 响应转换阶段

### Model → ModelInfo 转换

**位置**: `loom/src/services/models.rs:90-96`

```rust
impl From<&Model> for ModelInfo {
    fn from(model: &Model) -> Self {
        ModelInfo {
            id: model.id.clone(),
            name: model.name.clone(),
            capabilities: ModelCapabilities {
                attachment: model.attachment,
                reasoning: model.reasoning,
                tool_call: model.tool_call,
                temperature: model.temperature,
                structured_output: model.structured_output,
                modalities: model.modalities.clone(),
            },
            provider: extract_provider_info(&model.id),
            pricing: extract_pricing_info(model),
            metadata: ModelMetadata {
                family: model.family.clone(),
                knowledge: model.knowledge.clone(),
                release_date: model.release_date.clone(),
                last_updated: model.last_updated.clone(),
            },
        }
    }
}
```

### 能力提取函数

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

fn extract_pricing_info(model: &Model) -> Option<PricingInfo> {
    model.cost.as_ref().map(|cost| PricingInfo {
        input: cost.input.clone(),
        output: cost.output.clone(),
        unit: cost.unit.clone(),
    })
}
```

## 5. 多用途数据流

### CLI 命令使用

**位置**: `cli/src/model_cmd.rs`

```rust
pub fn list_models() -> Result<(), Error> {
    let models = model_service.get_all_models();
    for model in models {
        println!("{}: {}", model.id, model.name);
    }
    Ok(())
}
```

### 内部逻辑使用

**位置**: `loom/src/message.rs`

```rust
pub fn validate_model_capabilities(
    model: &Model,
    required_capabilities: &ModelCapabilities,
) -> Result<(), ValidationError> {
    if required_capabilities.reasoning && !model.reasoning {
        return Err(ValidationError::UnsupportedReasoning);
    }
    // ... 其他能力验证
    Ok(())
}
```

## 6. 错误处理和恢复

### 错误传播

```rust
pub async fn load_models() -> Result<Vec<Model>, Error> {
    // 尝试 models.dev
    match fetch_from_models_dev().await {
        Ok(models) => Ok(models),
        Err(dev_error) => {
            // models.dev 失败，尝试本地文件
            match fetch_from_local_file().await {
                Ok(models) => {
                    tracing::warn!("Using local file due to models.dev error: {}", dev_error);
                    Ok(models)
                }
                Err(local_error) => {
                    // 都失败，返回缓存或空列表
                    Err(Error::AllSourcesFailed {
                        models_dev: dev_error,
                        local_file: local_error,
                    })
                }
            }
        }
    }
}
```

## 7. 性能优化

### 增量更新

```rust
pub fn update_models(&mut self, new_models: Vec<Model>) {
    for new_model in new_models {
        if let Some(existing) = self.models.get(&new_model.id) {
            // 检查是否需要更新
            if should_update(existing, &new_model) {
                self.models.insert(new_model.id.clone(), new_model);
            }
        } else {
            // 新模型
            self.models.insert(new_model.id.clone(), new_model);
        }
    }
}
```

### 批量处理

```rust
pub async fn batch_fetch_models(
    resolvers: Vec<Box<dyn ModelLimitResolver>>,
) -> Result<Vec<Model>, Error> {
    let futures: Vec<_> = resolvers
        .into_iter()
        .map(|r| r.fetch_all())
        .collect();
    
    let results = join_all(futures).await;
    // 合并和去重逻辑
    merge_model_results(results)
}
```

## 相关文件

- `model-spec-core/src/parser.rs`: JSON 解析逻辑
- `loom/src/model_spec/models_dev.rs`: models.dev 数据源
- `loom/src/model_spec/cached.rs`: 缓存层实现
- `loom/src/services/models.rs`: ModelService 存储和转换
- `loom/src/protocol/responses.rs`: API 响应格式定义