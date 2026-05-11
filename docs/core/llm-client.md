---
sidebar_position: 7
title: "LLM 客户端与模型配置"
description: "统一的 LLM 客户端接口和模型配置"
---

# LLM 客户端与模型配置

统一的 LLM 客户端接口和模型配置管理系统，支持多种提供商和灵活的配置方式。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 生产环境部署 | ✅ 最佳选择 | 支持多种提供商和容错机制 |
| 测试和开发 | ✅ 专门设计 | MockLlm 提供完整测试支持 |
| 多提供商集成 | ✅ 推荐使用 | 统一接口支持 OpenAI、OpenRouter 等 |
| 简单项目 | ✅ 可用 | 基础配置即可快速上手 |

## 核心概念

### LlmClient 接口

**LlmClient** 是所有 LLM 客户端的统一接口：

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn invoke(&self, messages: &[Message]) -> Result<LlmResponse, AgentError>;
    async fn invoke_stream(
        &self,
        messages: &[Message],
        chunk_tx: Option<mpsc::Sender<MessageChunk>>,
    ) -> Result<LlmResponse, AgentError>;
}
```

- `invoke()`: 非流式调用，返回完整响应
- `invoke_stream()`: 流式调用，支持实时获取响应块

### 响应类型

**LlmResponse**: 完整的 LLM 响应
```rust
pub struct LlmResponse {
    pub content: String,                    // 主要回复内容
    pub reasoning_content: Option<String>,   // 推理过程内容（如果支持）
    pub tool_calls: Vec<ToolCall>,          // 工具调用列表
    pub usage: Option<LlmUsage>,            // token 使用统计
}
```

**LlmUsage**: Token 使用量统计
```rust
pub struct LlmUsage {
    pub total_tokens: u32,       // 总 token 数
    pub prompt_tokens: u32,      // 提示词 token 数
    pub completion_tokens: u32,  // 回复 token 数
}
```

## 客户端类型对比

| 特性 | ChatOpenAI | ChatOpenAICompat | MockLlm |
|------|-----------|-----------------|----------|
| 用途 | OpenAI 官方 API | OpenAI 兼容 API | 测试和开发 |
| API 密钥 | 必需 | 必需 | 不需要 |
| 基础 URL | 默认 | 必需配置 | 不适用 |
| 流式支持 | ✅ | ✅ | ✅ |
| 工具调用 | ✅ | ✅ | ✅ |
| 生产就绪 | ✅ | ✅ | ❌ |

## 代码示例

### 基础 OpenAI 配置

```rust
use loom::llm::{LlmClient, ChatOpenAI};
use loom::state::Message;
use loom::llm::ToolChoiceMode;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建 OpenAI 客户端
    let client = ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-openai-api-key".to_string(),
    )
    .with_temperature(0.7)
    .with_tool_choice(ToolChoiceMode::Auto);

    // 准备消息
    let messages = vec![
        Message::User("你好，请介绍一下 Rust 编程语言".to_string())
    ];

    // 发起请求
    let response = client.invoke(&messages).await?;
    
    println!("回复内容: {}", response.content);
    println!("Token 使用: {:?}", response.usage);
    
    Ok(())
}
```

### OpenAI 兼容 API (OpenRouter) 配置

```rust
use loom::llm::{LlmClient, ChatOpenAICompat};
use loom::state::Message;
use loom::llm::ToolChoiceMode;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建 OpenRouter 客户端
    let client = ChatOpenAICompat::new(
        "anthropic/claude-3-opus".to_string(),
        "your-openrouter-api-key".to_string(),
        "https://openrouter.ai/api/v1/chat/completions".to_string(),
    )
    .with_temperature(0.5)
    .with_headers(vec![
        ("HTTP-Referer".to_string(), "https://your-app.com".to_string()),
        ("X-Title".to_string(), "My App".to_string()),
    ]);

    let messages = vec![
        Message::User("分析一下量子计算的现状".to_string())
    ];

    let response = client.invoke(&messages).await?;
    println!("回复: {}", response.content);
    
    Ok(())
}
```

### 流式输出示例

```rust
use loom::llm::{LlmClient, ChatOpenAI};
use loom::state::Message;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    );

    let messages = vec![
        Message::User("写一段关于人工智能的简短介绍".to_string())
    ];

    // 创建流式通道
    let (chunk_tx, mut chunk_rx) = mpsc::channel(100);
    
    // 启动流式请求
    let response_task = tokio::spawn(async move {
        client.invoke_stream(&messages, Some(chunk_tx)).await
    });

    // 实时处理响应块
    while let Some(chunk) = chunk_rx.recv().await {
        match chunk {
            loom::llm::MessageChunk::Content(text) => print!("{}", text),
            loom::llm::MessageChunk::Reasoning(text) => print!("[思考: {}]", text),
            _ => {}
        }
    }

    let response = response_task.await??;
    println!("\n最终统计: {:?}", response.usage);
    
    Ok(())
}
```

### Mock 测试客户端

```rust
use loom::llm::{LlmClient, MockLlm, ToolCall};
use loom::state::Message;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建测试用的 Mock 客户端
    let mock_client = MockLlm::new()
        .with_content("这是一个测试回复")
        .with_tool_calls(vec![
            ToolCall {
                id: "test_call_1".to_string(),
                name: "search_web".to_string(),
                arguments: json!({"query": "test query"}),
            }
        ])
        .with_usage(loom::llm::LlmUsage {
            total_tokens: 100,
            prompt_tokens: 50,
            completion_tokens: 50,
        });

    let messages = vec![
        Message::User("测试消息".to_string())
    ];

    let response = mock_client.invoke(&messages).await?;
    
    println!("Mock 回复: {}", response.content);
    println!("工具调用: {:?}", response.tool_calls);
    assert_eq!(response.usage.unwrap().total_tokens, 100);
    
    Ok(())
}
```

### 通过 config.toml 配置

```toml
# config.toml
[models]
[[models.entries]]
id = "gpt-4o"
name = "GPT-4o"
provider = "openai"
api_key = "your-openai-api-key"
temperature = 0.7
tool_choice = "auto"

[[models.entries]]
id = "claude-3-opus"
name = "Claude 3 Opus" 
provider = "openrouter"
api_key = "your-openrouter-key"
base_url = "https://openrouter.ai/api/v1/chat/completions"
temperature = 0.5

[providers]
[[providers.config]]
name = "openai"
base_url = "https://api.openai.com/v1/chat/completions"
api_key = "default-openai-key"

[[providers.config]]
name = "openrouter"
base_url = "https://openrouter.ai/api/v1/chat/completions"
enable_tier_resolution = true
```

```rust
use loom::llm::{ModelRegistry, LlmProvider};
use loom::state::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 从配置文件加载模型注册表
    let registry = ModelRegistry::from_config_file("config.toml").await?;
    
    // 获取特定模型
    let provider = registry.get_provider("openai")?;
    let client = provider.create_client("gpt-4o")?;
    
    let messages = vec![
        Message::User("你好，请介绍一下自己".to_string())
    ];

    let response = client.invoke(&messages).await?;
    println!("回复: {}", response.content);
    
    Ok(())
}
```

## 工具调用配置

### ToolChoiceMode 选项

```rust
use loom::llm::ToolChoiceMode;

// 自动决定是否调用工具
let mode = ToolChoiceMode::Auto;

// 强制要求调用工具
let mode = ToolChoiceMode::Required;

// 禁用工具调用
let mode = ToolChoiceMode::None;

// 指定特定工具
let mode = ToolChoiceMode::Specific {
    name: "search_web".to_string(),
    id: Some("tool_123".to_string()),
};
```

### 带工具调用的完整示例

```rust
use loom::llm::{LlmClient, ChatOpenAI, ToolChoiceMode};
use loom::state::Message;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    )
    .with_tool_choice(ToolChoiceMode::Auto);

    // 定义可用工具
    let tools = vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "获取指定城市的天气信息",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "城市名称"}
                    },
                    "required": ["city"]
                }
            }
        })
    ];

    let messages = vec![
        Message::User("查询一下北京今天的天气".to_string())
    ];

    let response = client.invoke_with_tools(&messages, &tools).await?;
    
    if !response.tool_calls.is_empty() {
        println!("需要调用工具: {:?}", response.tool_calls);
    } else {
        println!("直接回复: {}", response.content);
    }
    
    Ok(())
}
```

## 环境变量配置

```bash
# .env 文件
OPENAI_BASE_URL=https://api.openai.com/v1/chat/completions
OPENAI_API_KEY=your-openai-api-key

# 可选：分布式追踪
LLM_TRACE_ID=trace-123
LLM_THREAD_ID=thread-456
```

```rust
use loom::llm::{ChatOpenAICompat, LlmClient};
use loom::state::Message;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 从环境变量读取配置
    let base_url = env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
    
    let api_key = env::var("OPENAI_API_KEY")?;
    
    let client = ChatOpenAICompat::new(
        "gpt-4o".to_string(),
        api_key,
        base_url,
    );

    let messages = vec![
        Message::User("你好".to_string())
    ];

    let response = client.invoke(&messages).await?;
    println!("{}", response.content);
    
    Ok(())
}
```

## 最佳实践

### 生产环境配置
- 使用 `ModelRegistry` 统一管理多个提供商和模型
- 配置合理的重试策略和超时时间
- 监控 `LlmUsage` 控制成本
- 使用环境变量管理敏感信息

### 性能优化
- 对于长文本生成使用流式调用
- 合理设置 `temperature` 平衡创造性和稳定性
- 启用模型缓存减少重复配置开销
- 使用连接池管理并发请求

### 错误处理
- 实现完善的错误处理和重试逻辑
- 监控 API 调用失败率和响应时间
- 配置降级策略，在主提供商故障时切换备用

### 测试策略
- 使用 `MockLlm` 进行单元测试
- 测试各种 `ToolChoiceMode` 场景
- 验证流式和非流式调用的一致性
- 模拟网络错误和超时情况

---

## 相关概念

- **ReAct 运行模式**: 基于循环推理的智能体模式
- **工具系统**: 工具调用和开发指南
- **状态管理**: Message 和状态流转机制

---

**下一页**: [ReAct 运行模式](./react.md) | [工具系统](./tool-system.md) | [状态管理](./state-management.md)