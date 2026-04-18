# 网关 Provider Tier Resolution 开发方案

## 文档信息
- **版本**: v1.0
- **日期**: 2025-08-19
- **作者**: AI Assistant
- **状态**: Draft

---

## 1. 问题背景

### 1.1 当前问题
当主模型使用网关 provider（如 `openrouter/xiaomi/mimo-v2-pro`）时，子代理在进行 tier resolution 时可能出现以下问题：

1. **Provider 不匹配**：子代理未能正确继承父代理的网关 provider
2. **Tier 解析范围错误**：在错误的 provider 上下文中进行 tier resolution
3. **模型选择不一致**：子代理选择了不同网关或非网关模型

### 1.2 用户场景
```toml
# 父代理配置
[[providers]]
name = "openrouter"
api_key = "your-api-key"
base_url = "https://api.modelgate.dev/v1"
fetch_models = true

# 主模型
model = "openrouter/xiaomi/mimo-v2-pro"
model_tier = "Light"

# 子代理期望
# 应该选择: openrouter/google/gemma-4-26b-a4b-it (相同网关)
# 而不是: 其他网关或直接 provider 模型
```

---

## 2. 需求分析

### 2.1 功能需求
1. **Provider 继承**：子代理默认继承父代理的网关 provider
2. **Tier 一致性**：在相同网关内进行 tier resolution
3. **配置优先**：用户显式配置的 provider 优先级最高
4. **降级策略**：网关不支持请求 tier 时的备选方案
5. **网关识别**：自动识别网关 provider 类型

### 2.2 非功能需求
1. **性能**：tier resolution 时间 < 2秒
2. **可靠性**：成功率 > 95%
3. **兼容性**：不影响现有非网关 provider 功能
4. **可维护性**：代码清晰，易于扩展新网关

---

## 3. 技术方案设计

### 3.1 总体架构

```
Tier Resolution Engine
├── Provider Inheritance Layer
├── Gateway Detection Module
├── Tier Resolution Strategies
│   ├── Gateway-Specific Resolution
│   ├── Standard Resolution
│   └── Fallback Resolution
└── Cache Management Layer
```

### 3.2 核心组件设计

#### 3.2.1 网关识别模块

```rust
#[derive(Debug, Clone)]
pub struct GatewayProvider {
    pub name: String,
    pub gateway_type: GatewayType,
    pub api_endpoint: String,
    pub supports_tier_resolution: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GatewayType {
    OpenRouter,
    CustomGateway(String),
    StandardProvider,
}

pub struct GatewayDetector;

impl GatewayDetector {
    /// 检测是否为网关 provider
    pub fn detect_gateway(provider_name: &str, base_url: &Option<String>) -> GatewayType {
        // 命名约定检测
        if provider_name.to_lowercase().contains("openrouter") {
            return GatewayType::OpenRouter;
        }
        
        // URL 模式检测
        if let Some(url) = base_url {
            if url.contains("modelgate") || url.contains("gateway") {
                return GatewayType::CustomGateway(provider_name.to_string());
            }
        }
        
        // 配置标记检测
        // 可以扩展为从配置中读取网关标记
        
        GatewayType::StandardProvider
    }
    
    /// 判断是否为网关 provider
    pub fn is_gateway(provider_name: &str, base_url: &Option<String>) -> bool {
        !matches!(
            Self::detect_gateway(provider_name, base_url),
            GatewayType::StandardProvider
        )
    }
}
```

#### 3.2.2 Provider 继承逻辑

```rust
pub struct ProviderInheritance {
    pub forced_provider: Option<String>,
    pub inherit_from_parent: bool,
    pub gateway_priority: GatewayPriority,
}

#[derive(Debug, Clone)]
pub enum GatewayPriority {
    ForceSameGateway,    // 强制使用相同网关
    PreferSameGateway,   // 优先使用相同网关
    AllowAnyProvider,    // 允许任何 provider
}

impl ProviderInheritance {
    /// 确定子代理的 provider
    pub fn determine_provider(
        &self,
        parent_config: &ReactBuildConfig,
        child_config: &ReactBuildConfig,
    ) -> Option<String> {
        // 1. 配置优先：用户显式配置
        if let Some(forced) = &self.forced_provider {
            return Some(forced.clone());
        }
        
        // 2. 继承逻辑：使用父代理 provider
        if self.inherit_from_parent {
            if let Some(parent_provider) = &parent_config.llm_provider {
                return Some(parent_provider.clone());
            }
        }
        
        // 3. 从模型 ID 解析
        if let Some(model) = &child_config.model {
            if let Some((provider, _)) = parse_provider_model(model) {
                return Some(provider.to_string());
            }
        }
        
        None
    }
}
```

#### 3.2.3 混合 Tier Resolution 策略

```rust
pub struct HybridTierResolver {
    pub gateway_detector: GatewayDetector,
    pub provider_inheritance: ProviderInheritance,
    pub cache_manager: CacheManager,
}

impl HybridTierResolver {
    /// 混合策略 tier resolution
    pub async fn resolve_tier_hybrid(
        &self,
        child_config: &ReactBuildConfig,
        parent_config: &ReactBuildConfig,
        target_tier: ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        // 确定使用的 provider
        let provider = self.provider_inheritance.determine_provider(parent_config, child_config);
        
        match provider {
            Some(provider_name) => {
                // 检查是否为网关
                let provider_config = providers.iter().find(|p| p.name == provider_name);
                let is_gateway = self.gateway_detector.is_gateway(
                    &provider_name,
                    &provider_config.and_then(|p| p.base_url.clone()),
                );
                
                if is_gateway {
                    // 网关专用 tier resolution
                    self.resolve_tier_for_gateway(&provider_name, target_tier, providers).await
                } else {
                    // 标准 tier resolution
                    self.resolve_tier_standard(&provider_name, target_tier, providers).await
                }
            }
            None => {
                // 降级：通用 tier resolution
                self.resolve_tier_fallback(target_tier, providers).await
            }
        }
    }
    
    /// 网关专用 tier resolution
    async fn resolve_tier_for_gateway(
        &self,
        gateway_provider: &str,
        target_tier: ModelTier,
        providers: &[ProviderConfig],
    ) -> Option<ModelEntry> {
        // 1. 尝试从网关 API 获取模型列表
        if let Some(models) = self.fetch_models_from_gateway_api(gateway_provider).await {
            // 2. 过滤符合 tier 的模型
            let tier_models = self.filter_models_by_tier(&models, target_tier);
            if let Some(selected) = self.select_best_model(&tier_models) {
                return Some(selected);
            }
        }
        
        // 3. 降级到标准 tier resolution
        self.resolve_tier_standard(gateway_provider, target_tier, providers).await
    }
    
    /// 从网关 API 获取模型列表
    async fn fetch_models_from_gateway_api(
        &self,
        gateway_provider: &str,
    ) -> Option<Vec<ModelEntry>> {
        // 实现网关 API 调用逻辑
        // 支持缓存
        None
    }
}
```

### 3.3 配置扩展

#### 3.3.1 Provider 配置扩展

```toml
[[providers]]
name = "openrouter"
api_key = "your-api-key"
base_url = "https://api.modelgate.dev/v1"
fetch_models = true
cache_ttl = 300

# 新增：网关配置
is_gateway = true  # 显式标记为网关
gateway_type = "openrouter"  # 网关类型
supports_tier_resolution = true  # 是否支持 tier resolution
tier_resolution_endpoint = "/v1/tiers"  # tier resolution API 端点
```

#### 3.3.2 子代理配置扩展

```toml
# 子代理配置
[[agents]]
name = "sub-agent"
profile = "default"

# 新增：provider 继承策略
provider_inheritance = "inherit"  # inherit | force | auto
forced_provider = ""  # 当 provider_inheritance = "force" 时使用
gateway_priority = "same"  # same | prefer | allow
```

---

## 4. 实现计划

### 4.1 Phase 1: 基础架构 (1-2周)

#### 4.1.1 网关识别模块
- [ ] 实现 `GatewayDetector` 结构
- [ ] 添加网关类型枚举
- [ ] 实现命名约定检测
- [ ] 实现 URL 模式检测
- [ ] 编写单元测试

#### 4.1.2 Provider 继承逻辑
- [ ] 实现 `ProviderInheritance` 结构
- [ ] 添加继承策略枚举
- [ ] 实现 provider 确定逻辑
- [ ] 集成到子代理创建流程
- [ ] 编写集成测试

### 4.2 Phase 2: 核心功能 (2-3周)

#### 4.2.1 混合 Tier Resolution
- [ ] 实现 `HybridTierResolver` 结构
- [ ] 实现网关专用 tier resolution
- [ ] 实现标准 tier resolution
- [ ] 实现降级策略
- [ ] 添加缓存支持

#### 4.2.2 网关 API 集成
- [ ] 实现网关 API 调用逻辑
- [ ] 添加模型列表获取
- [ ] 实现 tier 过滤逻辑
- [ ] 添加错误处理和重试机制

### 4.3 Phase 3: 配置和集成 (1周)

#### 4.3.1 配置系统扩展
- [ ] 扩展 Provider 配置结构
- [ ] 扩展子代理配置结构
- [ ] 更新配置解析逻辑
- [ ] 添加配置验证

#### 4.3.2 集成到现有系统
- [ ] 集成到子代理创建流程
- [ ] 集成到 tier resolution 流程
- [ ] 更新日志和监控
- [ ] 向后兼容性测试

### 4.4 Phase 4: 测试和优化 (1-2周)

#### 4.4.1 测试覆盖
- [ ] 单元测试：网关识别、provider 继承
- [ ] 集成测试：完整 tier resolution 流程
- [ ] 端到端测试：子代理创建和 tier resolution
- [ ] 性能测试：响应时间和资源使用

#### 4.4.2 性能优化
- [ ] 缓存策略优化
- [ ] 并发请求优化
- [ ] 内存使用优化
- [ ] 监控指标添加

---

## 5. 代码示例

### 5.1 网关识别示例

```rust
use crate::gateway::{GatewayDetector, GatewayType};

fn example_gateway_detection() {
    let provider_name = "openrouter";
    let base_url = Some("https://api.modelgate.dev/v1".to_string());
    
    let gateway_type = GatewayDetector::detect_gateway(provider_name, &base_url);
    
    match gateway_type {
        GatewayType::OpenRouter => println!("OpenRouter 网关 detected"),
        GatewayType::CustomGateway(name) => println!("自定义网关: {}", name),
        GatewayType::StandardProvider => println!("标准 provider"),
    }
}
```

### 5.2 Provider 继承示例

```rust
use crate::provider_inheritance::{ProviderInheritance, GatewayPriority};

async fn example_provider_inheritance() {
    let inheritance = ProviderInheritance {
        forced_provider: None,
        inherit_from_parent: true,
        gateway_priority: GatewayPriority::PreferSameGateway,
    };
    
    let parent_config = ReactBuildConfig {
        llm_provider: Some("openrouter".to_string()),
        model: Some("openrouter/xiaomi/mimo-v2-pro".to_string()),
        ..Default::default()
    };
    
    let child_config = ReactBuildConfig {
        model: Some("google/gemma-4-26b-a4b-it".to_string()),
        ..Default::default()
    };
    
    let provider = inheritance.determine_provider(&parent_config, &child_config);
    println!("子代理 provider: {:?}", provider); // 输出: Some("openrouter")
}
```

### 5.3 混合 Tier Resolution 示例

```rust
use crate::hybrid_resolver::HybridTierResolver;
use crate::model_spec::ModelTier;

async fn example_hybrid_tier_resolution() {
    let resolver = HybridTierResolver::new();
    
    let parent_config = ReactBuildConfig {
        llm_provider: Some("openrouter".to_string()),
        model: Some("openrouter/xiaomi/mimo-v2-pro".to_string()),
        model_tier: Some(ModelTier::Light),
        ..Default::default()
    };
    
    let child_config = ReactBuildConfig {
        model: Some("google/gemma-4-26b-a4b-it".to_string()),
        ..Default::default()
    };
    
    let providers = load_provider_configs().await;
    
    let resolved_model = resolver.resolve_tier_hybrid(
        &child_config,
        &parent_config,
        ModelTier::Light,
        &providers,
    ).await;
    
    match resolved_model {
        Some(entry) => println!("Resolved model: {}", entry.id),
        None => println!("Tier resolution failed"),
    }
}
```

### 5.4 配置示例

#### 5.4.1 网关 Provider 配置

```toml
# OpenRouter 网关配置
[[providers]]
name = "openrouter"
api_key = "your-openrouter-api-key"
base_url = "https://api.modelgate.dev/v1"
fetch_models = true
cache_ttl = 300

# 网关特定配置
is_gateway = true
gateway_type = "openrouter"
supports_tier_resolution = true
tier_resolution_endpoint = "/v1/tiers"

# 自定义网关配置
[[providers]]
name = "custom-gateway"
api_key = "your-custom-api-key"
base_url = "https://gateway.example.com/v1"
fetch_models = true
cache_ttl = 600

is_gateway = true
gateway_type = "custom"
supports_tier_resolution = false  # 不支持 tier resolution
```

#### 5.4.2 子代理配置

```toml
# 子代理配置示例
[[agents]]
name = "coding-assistant"
profile = "developer"

# Provider 继承策略
provider_inheritance = "inherit"  # 继承父代理 provider
gateway_priority = "same"  # 优先使用相同网关

# 可选：强制特定 provider
# provider_inheritance = "force"
# forced_provider = "openrouter"

# Tier 配置
model_tier = "Standard"
```

---

## 6. 配置示例

### 6.1 完整配置示例

```toml
# 全局配置
[general]
log_level = "info"
cache_enabled = true

# Provider 配置
[[providers]]
# OpenRouter 网关
name = "openrouter"
api_key = "${OPENROUTER_API_KEY}"
base_url = "https://api.modelgate.dev/v1"
fetch_models = true
cache_ttl = 300

# 网关标记
is_gateway = true
gateway_type = "openrouter"
supports_tier_resolution = true

[[providers]]
# 标准 OpenAI provider
name = "openai"
api_key = "${OPENAI_API_KEY}"
base_url = "https://api.openai.com/v1"
fetch_models = false

# 子代理配置
[[agents]]
name = "sub-agent-1"
profile = "default"
provider_inheritance = "inherit"
gateway_priority = "same"
model_tier = "Light"

[[agents]]
name = "sub-agent-2"
profile = "developer"
provider_inheritance = "force"
forced_provider = "openrouter"
model_tier = "Standard"
```

### 6.2 环境变量配置

```bash
# API Keys
export OPENROUTER_API_KEY="your-openrouter-key"
export OPENAI_API_KEY="your-openai-key"

# 配置路径
export LOOM_CONFIG_PATH="~/.loom/config.toml"

# 缓存配置
export LOOM_CACHE_ENABLED=true
export LOOM_CACHE_TTL=300
```

---

## 7. 错误处理和降级策略

### 7.1 错误场景处理

```rust
pub enum TierResolutionError {
    GatewayUnavailable(String),
    TierNotSupported(String, ModelTier),
    ModelNotFound(String),
    ApiError(String),
    CacheError(String),
}

impl HybridTierResolver {
    pub async fn resolve_tier_with_error_handling(
        &self,
        config: &ReactBuildConfig,
        tier: ModelTier,
    ) -> Result<ModelEntry, TierResolutionError> {
        match self.resolve_tier_hybrid(config, tier).await {
            Some(entry) => Ok(entry),
            None => {
                // 降级策略
                self.fallback_resolution(config, tier).await
                    .ok_or_else(|| TierResolutionError::ModelNotFound(
                        format!("No model found for tier {:?}", tier)
                    ))
            }
        }
    }
    
    async fn fallback_resolution(
        &self,
        config: &ReactBuildConfig,
        tier: ModelTier,
    ) -> Option<ModelEntry> {
        // 1. 尝试降低 tier 要求
        let lower_tiers = match tier {
            ModelTier::Premium => vec![ModelTier::Standard, ModelTier::Light],
            ModelTier::Standard => vec![ModelTier::Light],
            ModelTier::Light => vec![],
        };
        
        for lower_tier in lower_tiers {
            if let Some(entry) = self.resolve_tier_hybrid(config, lower_tier).await {
                tracing::warn!(
                    original_tier = ?tier,
                    fallback_tier = ?lower_tier,
                    model = %entry.id,
                    "Using fallback tier due to resolution failure"
                );
                return Some(entry);
            }
        }
        
        // 2. 尝试使用默认模型
        self.get_default_model(config).await
    }
}
```

### 7.2 监控和告警

```rust
pub struct TierResolutionMetrics {
    pub total_requests: Counter,
    pub successful_resolutions: Counter,
    pub gateway_resolutions: Counter,
    pub fallback_resolutions: Counter,
    pub resolution_duration: Histogram,
}

impl TierResolutionMetrics {
    pub fn record_resolution(&self, provider: &str, success: bool, duration: Duration) {
        self.total_requests.inc();
        
        if success {
            self.successful_resolutions.inc();
        }
        
        if provider.contains("gateway") || provider.contains("openrouter") {
            self.gateway_resolutions.inc();
        }
        
        self.resolution_duration.observe(duration.as_secs_f64());
    }
}
```

---

## 8. 测试策略

### 8.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_gateway_detection() {
        let detector = GatewayDetector;
        
        assert_eq!(
            detector.detect_gateway("openrouter", &Some("https://api.modelgate.dev/v1".to_string())),
            GatewayType::OpenRouter
        );
        
        assert_eq!(
            detector.detect_gateway("openai", &Some("https://api.openai.com/v1".to_string())),
            GatewayType::StandardProvider
        );
    }
    
    #[tokio::test]
    async fn test_provider_inheritance() {
        let inheritance = ProviderInheritance {
            forced_provider: None,
            inherit_from_parent: true,
            gateway_priority: GatewayPriority::PreferSameGateway,
        };
        
        let parent = ReactBuildConfig {
            llm_provider: Some("openrouter".to_string()),
            ..Default::default()
        };
        
        let child = ReactBuildConfig::default();
        
        let provider = inheritance.determine_provider(&parent, &child);
        assert_eq!(provider, Some("openrouter".to_string()));
    }
}
```

### 8.2 集成测试

```rust
#[tokio::test]
async fn test_hybrid_tier_resolution_integration() {
    let resolver = HybridTierResolver::new();
    let providers = load_test_providers().await;
    
    let parent_config = ReactBuildConfig {
        llm_provider: Some("openrouter".to_string()),
        model: Some("openrouter/xiaomi/mimo-v2-pro".to_string()),
        model_tier: Some(ModelTier::Light),
        ..Default::default()
    };
    
    let child_config = ReactBuildConfig {
        model: Some("google/gemma-4-26b-a4b-it".to_string()),
        ..Default::default()
    };
    
    let result = resolver.resolve_tier_hybrid(
        &child_config,
        &parent_config,
        ModelTier::Light,
        &providers,
    ).await;
    
    assert!(result.is_some());
    assert!(result.unwrap().id.contains("openrouter"));
}
```

---

## 9. 部署和迁移

### 9.1 渐进式部署

1. **Phase 1**: 实现网关识别和 provider 继承
2. **Phase 2**: 添加混合 tier resolution
3. **Phase 3**: 集成到现有子代理流程
4. **Phase 4**: 全量部署和监控

### 9.2 向后兼容性

- 保持现有 API 不变
- 默认行为与现有系统一致
- 通过配置启用新功能
- 提供迁移指南

### 9.3 回滚策略

- 保留旧代码路径
- 配置开关控制新功能
- 监控关键指标
- 快速回滚机制

---

## 10. 风险评估

### 10.1 技术风险

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|----------|
| 网关 API 不稳定 | 高 | 中 | 添加重试机制和降级策略 |
| 缓存一致性问题 | 中 | 低 | 实现缓存失效机制 |
| 性能下降 | 中 | 低 | 性能测试和优化 |
| 配置复杂度增加 | 低 | 高 | 完善文档和配置示例 |

### 10.2 业务风险

- **用户接受度**：新功能可能需要用户学习成本
- **兼容性问题**：可能影响现有工作流程
- **维护成本**：增加代码复杂度

---

## 11. 项目计划

### 11.1 时间线

- **Week 1-2**: Phase 1 - 基础架构
- **Week 3-5**: Phase 2 - 核心功能
- **Week 6**: Phase 3 - 配置和集成
- **Week 7-8**: Phase 4 - 测试和优化
- **Week 9**: Code Review 和文档完善
- **Week 10**: 发布准备和部署

### 11.2 里程碑

1. **M1**: 网关识别和 provider 继承完成
2. **M2**: 混合 tier resolution 核心功能完成
3. **M3**: 集成测试通过
4. **M4**: 性能测试达标
5. **M5**: 文档和部署准备完成

---

## 12. 总结

本方案通过混合策略解决了网关 provider 的 tier resolution 问题，提供了：

1. **Provider 一致性**：子代理默认继承父代理网关
2. **灵活配置**：支持多种继承策略
3. **降级机制**：网关不支持时的备选方案
4. **性能优化**：缓存和并发处理
5. **可扩展性**：易于添加新网关支持

该方案在保持向后兼容性的同时，显著改善了网关 provider 的用户体验。