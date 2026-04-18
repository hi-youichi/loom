# models.dev 集成架构

## 概述

models.dev 是 Model 结构体的主要数据源之一，系统通过 HTTP API 获取模型元数据并解析为内部的 Model 结构体。

## 数据流架构

```
models.dev API
    ↓ HTTP 请求
ModelsDevResolver
    ↓ JSON 响应
parse_model() 函数
    ↓ 解析转换
Model 结构体实例
    ↓ 存储
ModelService
```

## 核心组件

### ModelsDevResolver

**位置**: `loom/src/model_spec/models_dev.rs`

```rust
pub struct ModelsDevResolver {
    client: HttpClient,
    base_url: String,
}
```

**职责**:
- 从 models.dev API 获取模型数据
- 处理 HTTP 请求和响应
- 管理 API 端点配置

### 解析函数

**位置**: `model-spec-core/src/parser.rs`

```rust
pub fn parse_model(json: &serde_json::Value) -> Result<Model, ParseError> {
    // 从 JSON 提取模型信息并创建 Model 实例
}
```

**解析逻辑**:
- 提取模型基本信息（ID、名称、系列）
- 解析能力标志（附件、推理、工具调用等）
- 处理成本和限制信息
- 验证数据完整性

## 集成模式

### 1. 数据获取

```rust
// loom/src/model_spec/models_dev.rs
impl ModelsDevResolver {
    pub async fn fetch_all(&self) -> Result<Vec<Model>, Error> {
        let response = self.client.get(&self.base_url).await?;
        let json: serde_json::Value = serde_json::from_str(&response)?;
        
        json["models"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(parse_model)
            .collect()
    }
}
```

### 2. 缓存集成

**位置**: `loom/src/model_spec/cached.rs`

```rust
pub struct CachedResolver<R: ModelLimitResolver> {
    inner: R,
    cache: Arc<RwLock<HashMap<String, Model>>>,
}
```

**缓存策略**:
- 内存缓存模型数据
- 支持定期刷新
- 减少 API 调用频率

### 3. 配置覆盖

**位置**: `loom/src/model_spec/config_override.rs`

```rust
pub struct ConfigOverride {
    base: Box<dyn ModelLimitResolver>,
    overrides: HashMap<String, Model>,
}
```

**用途**:
- 支持本地配置覆盖远程数据
- 开发和测试环境配置
- 紧急修复模型数据

## 错误处理

### 解析错误

```rust
pub enum ParseError {
    MissingField(String),
    InvalidFormat(String, String),
    UnsupportedValue(String, String),
}
```

### 网络错误

- HTTP 请求失败
- JSON 解析错误
- 超时处理

## 配置选项

### API 端点配置

```rust
pub const DEFAULT_MODELS_DEV_URL: &str = "https://models.dev/api/v1/models";
```

### 刷新间隔

```rust
pub struct ResolverRefresher {
    resolver: Arc<CachedResolver<ModelsDevResolver>>,
    interval: Duration,
}
```

## 最佳实践

1. **错误恢复**: 网络失败时使用缓存数据
2. **增量更新**: 只更新变化的模型数据
3. **验证机制**: 解析时验证数据完整性
4. **监控告警**: 记录 API 调用成功率和延迟

## 相关文件

- `model-spec-core/src/parser.rs`: JSON 解析逻辑
- `loom/src/model_spec/models_dev.rs`: models.dev 解析器实现
- `loom/src/model_spec/cached.rs`: 缓存层实现
- `loom/src/model_spec/refresher.rs`: 定时刷新机制