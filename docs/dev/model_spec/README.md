# Model Specification Architecture

Model 结构体是整个系统的核心数据类型，用于定义和管理 AI 模型的元数据、能力和配置信息。

## 核心架构

### 模块结构

```
model-spec-core/
├── src/
│   ├── lib.rs              # 模块导出和公共接口
│   ├── model.rs           # Model 结构体定义 (核心)
│   ├── cost.rs            # 成本结构定义
│   ├── limit.rs           # 限制和模态定义
│   ├── provider.rs        # 提供商定义
│   ├── tier.rs            # 模型等级逻辑
│   ├── parser.rs          # JSON 解析器
│   └── spec.rs            # 重新导出公共接口
```

### 依赖关系

```
model-spec-core (核心数据定义)
    ↓ 被依赖
loom (主应用逻辑)
    ↓ 依赖  
serve (API 服务器)
    ↓ 依赖
config (配置管理)
```

## Model 结构体定义

**位置**: `model-spec-core/src/model.rs:12-55`

```rust
pub struct Model {
    pub id: String,                    // 模型唯一标识符
    pub name: String,                  // 模型显示名称
    pub family: Option<String>,        // 模型系列（可选）
    pub attachment: bool,              // 是否支持附件
    pub reasoning: bool,               // 是否支持推理
    pub tool_call: bool,               // 是否支持工具调用
    pub temperature: bool,             // 是否支持温度参数
    pub structured_output: Option<bool>, // 结构化输出支持
    pub knowledge: Option<String>,     // 知识截止日期
    pub release_date: Option<String>,  // 发布日期
    pub last_updated: Option<String>,  // 最后更新时间
    pub modalities: Modalities,        // 支持的模态（文本、图像等）
    pub open_weights: bool,            // 是否开源权重
    pub cost: Option<Cost>,            // 成本信息
    pub limit: Option<ModelLimit>,     // 限制信息
}
```

## 架构优势

1. **关注点分离**: 核心数据定义与业务逻辑分离
2. **可测试性**: model-spec-core 可独立测试
3. **可扩展性**: 支持多种数据源（models.dev、本地文件、缓存）
4. **类型安全**: Rust 类型系统确保数据一致性
5. **性能优化**: 支持缓存和定期刷新机制

## 相关文档

- [models.dev 集成](./models-dev-integration.md)
- [API 服务层](./api-service-layer.md)
- [数据流和转换](./data-flow.md)