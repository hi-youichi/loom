# Model Specification Architecture - 文档索引

## 快速开始

- [主文档](./README.md) - 架构概览和核心概念

## 详细文档

### 核心架构
- [Model 结构体定义](./README.md#model-结构体定义) - 核心数据结构
- [模块结构](./README.md#模块结构) - 代码组织方式
- [依赖关系](./README.md#依赖关系) - 模块间依赖

### 数据源集成
- [models.dev 集成](./models-dev-integration.md) - 外部数据源集成
  - 数据流架构
  - ModelsDevResolver 实现
  - 缓存策略
  - 错误处理

### 服务层
- [API 服务层](./api-service-layer.md) - API 接口设计
  - ModelService 实现
  - ModelInfo 转换
  - API 端点设计
  - 性能优化

### 数据处理
- [数据流和转换](./data-flow.md) - 完整数据生命周期
  - 数据获取阶段
  - 解析和转换阶段
  - 内部存储阶段
  - API 响应转换阶段
  - 多用途数据流

## 开发指南

### 架构优势
1. **关注点分离**: 核心数据定义与业务逻辑分离
2. **可测试性**: model-spec-core 可独立测试
3. **可扩展性**: 支持多种数据源（models.dev、本地文件、缓存）
4. **类型安全**: Rust 类型系统确保数据一致性
5. **性能优化**: 支持缓存和定期刷新机制

### 关键文件位置
- `model-spec-core/src/model.rs`: Model 结构体定义
- `loom/src/model_spec/models_dev.rs`: models.dev 集成
- `loom/src/services/models.rs`: API 服务层
- `loom/src/protocol/responses.rs`: API 响应格式

### 开发工作流
1. 修改 Model 结构体定义 → 更新解析逻辑 → 更新 API 转换
2. 添加新数据源 → 实现 ModelLimitResolver trait
3. 添加新 API 端点 → 更新 ModelService

## 相关资源

- [Rust 语言规范](https://doc.rust-lang.org/)
- [serde JSON 序列化](https://serde.rs/)
- [Axum Web 框架](https://docs.rs/axum/)