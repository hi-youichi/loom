---
sidebar_position: 1
title: "工具系统"
description: "核心工具抽象和执行机制"
---

# 工具系统

Loom 框架的核心工具抽象和执行机制，提供统一的工具调用接口和丰富的内置工具实现。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 系统管理 | ✅ 完美支持 | Bash/PowerShell 工具支持完整命令执行 |
| 网络请求 | ✅ 原生支持 | HTTP GET/POST 请求和网页获取 |
| 数据持久化 | ✅ 完美支持 | 长期记忆工具支持数据存储和检索 |
| 自定义集成 | ✅ 灵活扩展 | 通过 ToolSource trait 实现自定义工具 |
| 跨服务通信 | ✅ 推荐使用 | MCP 协议支持第三方服务集成 |

## 核心概念

### ToolSource Trait

工具系统的核心抽象，定义了工具列表和调用接口：

```rust
#[async_trait]
pub trait ToolSource: Send + Sync {
    /// 列出所有可用工具及其规范
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError>;

    /// 调用指定工具，传入 JSON 参数
    async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolCallContent, ToolSourceError>;

    /// 带上下文的工具调用（可选实现）
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}
```

### 工具生命周期

1. **注册阶段**: 工具注册到 `AggregateToolSource`
2. **发现阶段**: LLM 通过 `list_tools()` 获取工具规范
3. **调用阶段**: LLM 生成 `ToolCall`，智能体通过 `call_tool()` 执行
4. **结果处理**: `normalize_tool_output` 规范化输出大小
5. **状态更新**: `ToolResult` 整合到智能体状态

### 核心类型

**ToolCall**: 工具调用请求
```rust
pub struct ToolCall {
    pub name: String,           // 工具名称
    pub arguments: String,      // JSON 字符串参数
    pub id: Option<String>,     // 调用ID，用于结果关联
}
```

**ToolResult**: 工具执行结果
```rust
pub struct ToolResult {
    pub call_id: Option<String>,            // 关联的调用ID
    pub name: Option<String>,              // 工具名称
    pub content: String,                   // 后向兼容的内容
    pub is_error: bool,                    // 是否错误
    pub observation_text: Option<String>,  // LLM 观察文本
    pub display_text: Option<String>,      // UI 显示文本
    pub storage_ref: Option<ToolStorageRef>, // 存储引用
    pub truncated: bool,                   // 是否被截断
}
```

## 工具源对比

| 工具源类型 | 用途 | 典型工具 | 适用场景 |
|-----------|------|---------|---------|
| **BashToolsSource** | 系统命令执行 | `bash` | Linux/macOS 系统管理 |
| **WebToolsSource** | HTTP 请求 | `web_fetcher` | API 调用、网页抓取 |
| **StoreToolSource** | 长期记忆 | `remember`, `recall`, `search_memories` | 数据持久化 |
| **MemoryToolsSource** | 综合记忆 | 记忆工具集合 | 完整记忆功能 |
| **AggregateToolSource** | 工具聚合 | 多种工具 | 主要工具源实现 |
| **Custom ToolSource** | 自定义工具 | 用户定义 | 特定业务需求 |

## 代码示例

### 使用 BashToolsSource 与智能体集成

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::tool_source::{BashToolsSource, ToolSource};
use loom::tools::BuiltinToolFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建 Bash 工具源
    let bash_source = BashToolsSource::new();
    
    // 验证工具注册
    let tools = bash_source.list_tools().await?;
    println!("可用工具: {:?}", tools.iter().map(|t| &t.name).collect::<Vec<_>>());
    
    // 配置 ReAct 智能体使用 Bash 工具
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        builtin_tools: BuiltinToolFilter::All, // 包含所有内置工具包括 Bash
        ..Default::default()
    };
    
    let runner = build_react_runner(&config, None, true).await?;
    let result = runner.invoke("列出当前目录的文件").await?;
    
    println!("执行结果: {:?}", result.messages.last());
    
    Ok(())
}
```

### 创建自定义 ToolSource

```rust
use async_trait::async_trait;
use loom::tool_source::{ToolSource, ToolSpec, ToolCallContent, ToolSourceError};
use serde_json::json;

// 自定义天气工具源
struct WeatherToolSource {
    api_key: String,
}

impl WeatherToolSource {
    fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl ToolSource for WeatherToolSource {
    async fn list_tools(&self) -> Result<Vec<ToolSpec>, ToolSourceError> {
        Ok(vec![
            ToolSpec {
                name: "get_weather".to_string(),
                description: Some("获取指定城市的天气信息".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "城市名称，如：北京、上海"
                        }
                    },
                    "required": ["city"]
                }),
                output_hint: None,
            }
        ])
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolCallContent, ToolSourceError> {
        match name {
            "get_weather" => {
                let city = arguments.get("city")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolSourceError::InvalidInput("缺少城市参数".to_string()))?;
                
                // 模拟天气 API 调用
                let weather_data = format!(
                    "{}: 晴天, 温度 25°C, 湿度 60%, 风向 东南风 3级",
                    city
                );
                
                Ok(ToolCallContent::text(weather_data))
            }
            _ => Err(ToolSourceError::ToolNotFound(name.to_string()))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用自定义工具源
    let weather_source = WeatherToolSource::new("your-api-key".to_string());
    
    // 测试工具调用
    let tools = weather_source.list_tools().await?;
    println!("注册工具: {:?}", tools);
    
    let result = weather_source.call_tool(
        "get_weather",
        json!({"city": "深圳"})
    ).await?;
    
    println!("天气查询结果: {}", result.text().unwrap_or("无结果"));
    
    Ok(())
}
```

### 自定义工具并注册到 AggregateToolSource

```rust
use async_trait::async_trait;
use loom::tools::{Tool, ToolRegistryLocked};
use loom::tool_source::{ToolSource, ToolCallContent, ToolSourceError, ToolCallContext};
use loom::tools::aggregate_source::AggregateToolSource;
use serde_json::json;

// 自定义计算工具
struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    async fn call(
        &self,
        args: serde_json::Value,
        _ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError> {
        let expr = args.get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolSourceError::InvalidInput("缺少表达式".to_string()))?;
        
        // 简单的数学表达式求值（实际应用中应使用更安全的解析器）
        let result = match expr {
            "2+2" => "4",
            "10*5" => "50",
            "100/4" => "25",
            _ => return Err(ToolSourceError::ExecutionError("不支持的运算".to_string()))
        };
        
        Ok(ToolCallContent::text(format!("{} = {}", expr, result)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建聚合工具源
    let aggregate_source = AggregateToolSource::new();
    
    // 注册自定义工具
    aggregate_source.register_sync(Box::new(CalculatorTool));
    
    // 验证注册
    let tools = aggregate_source.list_tools().await?;
    let calc_tool = tools.iter().find(|t| t.name == "calculator");
    assert!(calc_tool.is_some(), "计算工具注册失败");
    
    // 调用自定义工具
    let result = aggregate_source.call_tool(
        "calculator",
        json!({"expression": "2+2"})
    ).await?;
    
    println!("计算结果: {}", result.text().unwrap());
    
    Ok(())
}
```

### 工具输出规范化配置

```rust
use loom::state::tool_output_normalizer::{normalize_tool_output, NormalizationConfig, ToolOutputHint};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 大文本工具输出
    let large_output = "A".repeat(10000); // 10KB 数据
    
    // 默认配置（截断策略）
    let config = NormalizationConfig::default();
    let normalized = normalize_tool_output(
        "file_reader",
        &json!({"path": "/large/file.txt"}),
        &large_output,
        false,
        None,
        config
    );
    
    println!("原始大小: {} 字符", large_output.len());
    println!("处理后大小: {} 字符", normalized.observation_text.unwrap_or_default().len());
    println!("是否截断: {}", normalized.truncated);
    
    // 文件引用策略（保存到磁盘）
    let file_config = NormalizationConfig {
        max_inline_size: 1024,          // 超过 1KB 使用文件引用
        default_strategy: loom::state::tool_output_normalizer::OutputStrategy::FileRef,
        ..Default::default()
    };
    
    let file_normalized = normalize_tool_output(
        "data_export",
        &json!({"format": "json"}),
        &large_output,
        false,
        Some(&ToolOutputHint::LargeData),
        file_config
    );
    
    if let Some(storage_ref) = file_normalized.storage_ref {
        println!("大数据已保存到: {:?}", storage_ref);
    }
    
    Ok(())
}
```

## 最佳实践

### 工具设计原则
- **单一职责**: 每个工具只做一件事，保持简单明确
- **输入验证**: 严格验证输入参数，提供清晰的错误信息
- **输出控制**: 使用适当的输出策略，避免过多上下文消耗
- **幂等性**: 相同输入产生相同输出，便于重试和缓存

### 安全考虑
- **沙箱执行**: Bash 工具应在受限环境中执行
- **参数清理**: 防止命令注入和 XSS 攻击
- **权限控制**: 使用 `FilteredToolSource` 限制工具访问
- **错误处理**: 区分用户错误和系统错误，返回适当信息

### 性能优化
- **异步设计**: 所有工具操作应使用异步 I/O
- **结果缓存**: 对幂等工具实现结果缓存
- **批量操作**: 支持批量参数减少调用次数
- **流式输出**: 大数据工具支持流式结果返回

### 监控和调试
- **调用日志**: 记录工具调用参数和结果
- **性能指标**: 监控工具执行时间和成功率
- **错误追踪**: 收集工具错误信息用于改进
- **测试覆盖**: 为每个工具编写单元测试和集成测试

---

## 工具系统基础范围

本文档涵盖工具系统的基础概念和核心功能：
- ✅ ToolSource trait 和内置实现
- ✅ 工具注册和调用机制
- ✅ 输出规范化策略
- ✅ 自定义工具开发

**不包含在此文档中**：
- ❌ MCP (Model Context Protocol) 集成 - 详见 [MCP 协议](./mcp.md)
- ❌ 高级工具链和组合模式
- ❌ 工具权限和安全策略详情

---

## 相关概念

- **ReAct 运行模式**: 工具与智能体的集成方式
- **MCP 协议**: 第三方服务集成标准
- **状态管理**: 工具结果在智能体状态中的流转
- **错误处理**: 工具调用异常的处理机制

---

**下一页**: [MCP 协议集成](./mcp.md) | [ReAct 运行模式](../core/react.md) | [状态管理](../core/state.md)