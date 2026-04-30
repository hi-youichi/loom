# ReAct 运行模式

基于图结构循环推理的智能体运行模式，通过 Think-Act-Observe 循环实现复杂任务的逐步推理和工具调用。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 多步骤推理任务 | ✅ 最佳选择 | 需要多次思考和工具调用的复杂任务 |
| 实时对话系统 | ✅ 推荐使用 | 支持流式输出和实时交互 |
| 工具调用密集型 | ✅ 专门设计 | 原生支持多轮工具调用链 |
| 简单问答任务 | ⚠️ 可用但过度 | 单轮任务可使用更简单的模式 |

## 核心概念

### ReAct 循环

ReAct 采用经典的 Think-Act-Observe 循环模式：

1. **Think (思考节点)**: LLM 分析当前状态，决定是否需要调用工具
2. **Act (行动节点)**: 执行工具调用，获取结果
3. **Observe (观察节点)**: 将工具结果整合到消息历史，准备下一轮思考

### 核心组件

**ReActState**: 维护整个循环的状态
```rust
pub struct ReActState {
    pub messages: Vec<Message>,           // 对话消息历史
    pub tool_calls: Vec<ToolCall>,        // 当前工具调用列表
    pub tool_results: Vec<ToolResult>,    // 工具执行结果
    pub turn_count: u32,                  // 循环轮次计数
    pub think_count: u32,                 // 思考次数统计
    pub should_continue: bool,            // 循环控制标志
    pub usage: Option<LlmUsage>,          // 当前轮次使用量
    pub total_usage: Option<LlmUsage>,    // 总计使用量
}
```

**ReactRunner**: 编译好的 ReAct 图执行器
```rust
impl ReactRunner {
    pub async fn invoke(&self, user_message: &str) -> Result<ReActState, RunError>
    pub async fn stream_with_callback<F>(&self, user_message: &str, on_event: Option<F>) 
        -> Result<StreamRunOutcome<ReActState>, RunError>
}
```

## 代码示例

### 最小化 ReAct 智能体

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::llm::{LlmProvider, OpenAIProvider};
use loom::tools::{ToolSource, BuiltinToolFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 配置 LLM 提供者
    let provider = Arc::new(OpenAIProvider::new("gpt-4o", "your-api-key".into()));
    
    // 构建配置
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        builtin_tools: BuiltinToolFilter::All,
        ..Default::default()
    };
    
    // 构建并运行 ReAct 智能体
    let runner = build_react_runner(&config, Some(provider), true).await?;
    let result = runner.invoke("帮我查询当前的系统时间").await?;
    
    println!("最终回复: {:?}", result.messages.last());
    
    Ok(())
}
```

### 自定义最大迭代次数

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::agent::react::config::BehaviorConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        behavior: BehaviorConfig {
            max_iterations: Some(10),  // 限制最多10轮循环
            ..Default::default()
        },
        ..Default::default()
    };
    
    let runner = build_react_runner(&config, None, true).await?;
    let result = runner.invoke("分析这个复杂的多步骤问题").await?;
    
    println!("总循环轮次: {}", result.turn_count);
    
    Ok(())
}
```

### 自定义工具配置

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::tools::{ToolSource, CustomTool, ToolDefinition};
use loom::state::Message;

// 自定义工具
struct WeatherTool;
impl CustomTool for WeatherTool {
    fn name(&self) -> &str { "get_weather" }
    
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_weather".to_string(),
            description: "获取指定城市的天气信息".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string", "description": "城市名称"}
                },
                "required": ["city"]
            }),
        }
    }
    
    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let city = args["city"].as_str().unwrap_or("北京");
        Ok(format!("{}: 晴天, 25°C", city))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建自定义工具源
    let mut tool_source = Box::new(loom::tools::MemoryToolSource::new());
    tool_source.add_tool(Arc::new(WeatherTool));
    
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        // 使用自定义工具源
        custom_tool_source: Some(tool_source),
        ..Default::default()
    };
    
    let runner = build_react_runner(&config, None, true).await?;
    let result = runner.invoke("查询上海今天的天气").await?;
    
    Ok(())
}
```

### 流式输出示例

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::agent::react::runner::StreamEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig::default();
    let runner = build_react_runner(&config, None, true).await?;
    
    let result = runner.stream_with_callback(
        "解释一下量子计算的基本原理",
        Some(|event| {
            match event {
                StreamEvent::ThinkStart => println!("开始思考..."),
                StreamEvent::ThinkToken(token) => print!("{}", token),
                StreamEvent::ToolCall(call) => println!("调用工具: {}", call.name),
                StreamEvent::ToolResult(result) => println!("工具结果: {}", result.content),
                _ => {}
            }
            async move { Ok(()) }
        })
    ).await?;
    
    println!("\n最终状态: {:?}", result.final_state.should_continue);
    
    Ok(())
}
```

## ReAct 循环流程图

```
用户消息
    ↓
START 节点
    ↓
┌─────────────┐
│  ThinkNode  │ ← LLM 思考，生成回复或工具调用
└──────┬──────┘
       │
       ├─→ tools_condition 判断
       │   ├─ 无工具调用 → END (结束)
       │   └─ 有工具调用 → ActNode
       │
       ↓
┌─────────────┐
│  ActNode    │ ← 执行工具调用，收集结果
└──────┬──────┘
       │
       ↓
┌─────────────┐
│ ObserveNode │ ← 整合结果到消息历史，控制循环
└──────┬──────┘
       │
       ├─→ 检查 max_iterations
       │   ├─ 达到上限 → END
       │   └─ 未达上限 → 回到 ThinkNode
       │
       ↓ (可选)
┌─────────────┐
│ CompressNode│ ← 压缩消息历史 (可选)
└─────────────┘
```

## 工具调用生命周期

```rust
// 1. ThinkNode: LLM 生成工具调用
state.tool_calls = vec![
    ToolCall {
        id: "call_123".to_string(),
        name: "search_web".to_string(),
        arguments: serde_json::json!({"query": "Rust 编程语言"})
    }
];

// 2. tools_condition: 决定继续循环
ToolsConditionResult::Tools  // 有工具调用，继续

// 3. ActNode: 执行工具调用
state.tool_results = vec![
    ToolResult {
        call_id: "call_123".to_string(),
        content: "Rust 是一种系统编程语言...".to_string(),
        is_error: false
    }
];

// 4. ObserveNode: 整合结果
state.messages.push(Message::Tool {
    tool_call_id: "call_123".to_string(),
    content: "Rust 是一种系统编程语言...".to_string()
});
state.turn_count += 1;
state.tool_calls.clear();  // 清空准备下一轮
state.tool_results.clear();

// 5. 回到 ThinkNode 开始新一轮思考
```

## 最佳实践

### 配置管理
- 根据任务复杂度设置合理的 `max_iterations` 避免无限循环
- 使用 `BuiltinToolFilter::Selective` 精准控制可用工具
- 为长时间运行的任务配置超时和错误处理策略

### 性能优化
- 启用消息压缩功能：`compress_config: Some(CompressConfig::default())`
- 合理设置 LLM 的 `temperature` 参数平衡创造性和稳定性
- 使用流式输出提升用户体验，特别是长文本生成场景

### 错误处理
- 配置 `error_handling` 策略：`HandleToolErrors::Continue` 或 `Stop`
- 实现 `approval_workflow` 对敏感操作进行人工确认
- 监控 `usage` 和 `total_usage` 控制成本

---

## 相关概念

- **DUP (Decomposition-Usage-Policy)**: 复杂任务分解策略
- **ToT (Tree of Thoughts)**: 树状思维推理模式  
- **GoT (Graph of Thoughts)**: 图状思维推理模式
- **Tool System**: 工具系统架构和自定义工具开发

---

**下一页**: [DUP 运行模式](./dup.md) | [ToT 运行模式](./tot.md) | [工具系统](./tool-system.md)