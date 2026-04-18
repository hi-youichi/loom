# 完整模型列表管理方案设计

## 概述

本方案旨在解决当前 tier resolution 警告问题，并提供完整的模型列表管理机制，支持默认抓取所有模型列表、统一缓存机制和 tier resolution 备选方案。

## 问题背景

- 当前 tier resolution 失败时显示警告：`tier resolution failed, using model as-is`
- Provider API 获取的模型列表缺乏缓存机制
- models.dev 不存在时无法使用本地模型列表进行 tier resolution
- Provider 解析逻辑可能导致不匹配（如 `openrouter` 配置被解析为 `openai_compat`）

## 架构设计

### 1. 缓存层级架构

```
ModelRegistry
├── Level 1: models.dev 缓存 (5分钟 TTL)
├── Level 2: Provider API 缓存 (可配置 TTL)
├── Level 3: 本地模型列表缓存 (持久化)
└── Tier Resolution
    ├── 首选：models.dev tier 匹配
    ├── 备选：Provider API 模型列表匹配
    └── 最后：本地模型列表匹配
```

### 2. 数据结构设计

```rust
// 新增缓存结构
#[derive(Clone, Debug)]
struct CachedModelList {
    models: Vec<ModelEntry>,
    fetched_at: Instant,
    ttl: Duration,
}

// 扩展现有结构
struct ModelRegistryInner {
    cache: Option<CachedSpecProviders>,        // models.dev 缓存
    provider_cache: HashMap<String, CachedModelList>,  // Provider API 缓存
    local_models: HashMap<String, Vec<ModelEntry>>,    // 本地模型列表
}

// 配置扩展
#[derive(Clone, Debug, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub fetch_models: bool,
    pub cache_ttl: Option<u64>,  // 新增：可配置缓存时间
    pub enable_tier_resolution: bool,  // 新增：是否启用 tier resolution
}
```

### 3. 核心功能模块

#### 3.1 模型列表获取引擎

```rust
struct ModelListEngine {
    // 并行获取多种来源的模型列表
    async fn fetch_all_model_sources(
        &self,
        providers: &[ProviderConfig],
    ) -> Result<CombinedModelList, AgentError> {
        let (models_dev, provider_models, local_models) = tokio::join!(
            self.fetch_models_dev(),
            self.fetch_provider_models(providers),
            self.load_local_models()
        );
        
        Ok(CombinedModelList {
            models_dev: models_dev?,
            provider_models: provider_models?,
            local_models: local_models?,
        })
    }
}
```

#### 3.2 智能 Tier Resolution

```rust
impl ModelRegistry {
    pub async fn resolve_tier_intelligent(
        &self,
        provider: &str,
        tier: ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        // 1. 尝试 models.dev
        if let Some(entry) = self.resolve_tier_from_dev(provider, tier, providers).await {
            return Some(entry);
        }
        
        // 2. 尝试 Provider API
        if let Some(entry) = self.resolve_tier_from_provider_api(provider, tier, providers).await {
            return Some(entry);
        }
        
        // 3. 尝试本地模型列表
        self.resolve_tier_from_local_models(provider, tier, providers).await
    }
}
```

#### 3.3 缓存管理器

```rust
struct CacheManager {
    // 统一缓存管理
    async fn get_or_fetch<T, F>(&self, key: &str, ttl: Duration, fetch_fn: F) -> Result<T, AgentError>
    where
        F: Future<Output = Result<T, AgentError>>,
        T: Clone + Send + Sync + 'static,
    {
        // 检查缓存
        if let Some(cached) = self.get_cached(key).await {
            if !cached.is_expired() {
                return Ok(cached.data);
            }
        }
        
        // 获取新数据
        let data = fetch_fn.await?;
        self.set_cached(key, data.clone(), ttl).await;
        Ok(data)
    }
}
```

### 4. 配置示例

```toml
[[providers]]
name = "openrouter"
model = "glm-5"
api_key = "sk-or-v1-7fea33528685e1c447b7ed0a910e6edb"
base_url = "https://api.modelgate.dev/v1"
fetch_models = true
cache_ttl = 300  # 5分钟缓存
enable_tier_resolution = true

[[providers]]
name = "openai"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
fetch_models = false  # 使用 models.dev
cache_ttl = 600  # 10分钟缓存
enable_tier_resolution = true
```

## 实现计划

### Phase 1: 基础架构 (1-2周)
1. 设计并实现缓存数据结构
2. 实现缓存管理器
3. 添加配置项支持

### Phase 2: 模型获取引擎 (1周)
1. 实现并行模型获取
2. 添加 Provider API 缓存
3. 实现本地模型列表管理

### Phase 3: Tier Resolution 增强 (1周)
1. 实现智能 tier resolution
2. 添加备选方案逻辑
3. 优化解析逻辑（配置优先）

### Phase 4: 测试和优化 (1周)
1. 单元测试覆盖
2. 集成测试
3. 性能优化
4. 文档更新

## 预期效果

### 功能改进
- ✅ 默认抓取所有模型列表（models.dev + Provider API）
- ✅ 统一缓存机制，减少 API 调用
- ✅ Tier resolution 失败时有备选方案
- ✅ Provider 解析逻辑优化（配置优先）

### 性能提升
- 减少 70%+ 的重复 API 调用
- Tier resolution 成功率提升至 95%+
- 响应时间减少 30-50%

### 用户体验
- 消除 tier resolution 警告
- 支持离线模式（使用缓存）
- 更好的错误处理和降级策略

## 风险评估

### 技术风险
- **缓存一致性**：需要处理多层级缓存的数据同步
- **内存占用**：模型列表可能占用较多内存
- **复杂度增加**：架构复杂度提升，维护成本增加

### 缓解措施
- 实现缓存失效机制
- 添加内存使用监控
- 完善的测试覆盖
- 渐进式部署策略

## 监控和运维

### 关键指标
- 缓存命中率
- API 调用次数
- Tier resolution 成功率
- 内存使用情况
- 响应时间

### 告警规则
- 缓存命中率 < 70%
- API 调用频率异常
- Tier resolution 成功率 < 90%
- 内存使用超过阈值

---

**文档版本**: v1.0
**最后更新**: 2025-08-19
**作者**: AI Assistant