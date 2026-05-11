---
sidebar_position: 1
title: "流式输出"
description: "实时事件流系统"
---

# 流式输出

实时事件流系统，提供图执行过程中的细粒度事件输出和实时响应能力。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 实时对话系统 | ✅ 最佳选择 | 逐字显示 LLM 回复，提升用户体验 |
| 长时间任务监控 | ✅ 推荐使用 | 实时显示任务进度和状态变化 |
| 调试和开发 | ✅ 专门设计 | 详细的事件流帮助理解执行过程 |
| 简单批量处理 | ⚠️ 可用但过度 | 短任务可使用非流式接口 |

## 核心概念

### 流式架构

Loom 的流式系统基于事件驱动架构，通过 `StreamEvent` 枚举传递执行过程中的各种事件：

1. **事件生成**: 图执行节点和工具产生事件
2. **事件传输**: 通过 `StreamWriter` 和通道传递事件
3. **事件消费**: 应用通过回调函数处理事件
4. **实时交付**: 支持 SSE/WebSocket 等实时通信协议

### 核心类型

**StreamEvent**: 事件类型枚举
```rust
pub enum StreamEvent<S> {
    // 基础状态事件
    Values(S),                                    // 完整状态快照
    Updates { node_id: String, state: S, namespace: Option<String> },
    
    // 消息流式传输
    Messages { chunk: MessageChunk, metadata: StreamMetadata },
    
    // 自定义和检查点事件
    Custom(Value),                                // JSON 载荷
    Checkpoint(CheckpointEvent<S>),              // 检查点事件
    TaskStart { node_id: String, namespace: Option<String> },
    TaskEnd { node_id: String, result: Result<(), String>, namespace: Option<String> },
    
    // ToT 特定事件
    TotExpand { candidates: Vec<String> },        // 多候选扩展
    TotEvaluate { chosen: usize, scores: Vec<f32> },
    TotBacktrack { reason: String, to_depth: u32 },
    
    // GoT 特定事件
    GotPlan { node_count: usize, edge_count: usize, node_ids: Vec<String> },
    GotNodeStart { node_id: String },
    GotNodeComplete { node_id: String, result_summary: String },
    GotNodeFailed { node_id: String, error: String },
    GotExpand { node_id: String, nodes_added: usize, edges_added: usize },
    
    // 使用量统计
    Usage { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32, 
            prefill_duration: Option<Duration>, decode_duration: Option<Duration> },
    
    // 工具生命周期事件
    ToolCallChunk { call_id: Option<String>, name: Option<String>, arguments_delta: String },
    ToolCall { call_id: Option<String>, name: String, arguments: Value },
    ToolStart { call_id: Option<String>, name: String },
    ToolOutput { call_id: Option<String>, name: String, content: String },
    ToolEnd { call_id: Option<String>, name: String, result: String, is_error: bool, raw_result: Option<String> },
    ToolApproval { call_id: Option<String>, name: String, arguments: Value },
}
```

**StreamMode**: 流式模式
```rust
pub enum StreamMode {
    Values,      // 每个节点后发出完整状态
    Updates,     // 发出带节点 ID 的增量更新
    Messages,    // 发出消息块（LLM 流式传输）
    Custom,      // 发出自定义 JSON 载荷
    Checkpoints, // 发出检查点事件
    Tasks,       // 发出任务开始/结束事件
    Tools,       // 发出工具生命周期事件
    Debug,       // 同时发出检查点和任务事件
}
```

**MessageChunk**: 消息块
```rust
pub struct MessageChunk {
    pub content: String,
    pub kind: MessageChunkKind,
}

pub enum MessageChunkKind {
    Message,  // 最终助手回复
    Thinking, // 智能体推理过程
}

impl MessageChunk {
    pub fn message(content: impl Into<String>) -> Self
    pub fn thinking(content: impl Into<String>) -> Self
}
```

## 代码示例

### 基础流式设置

```rust
use loom::agent::react::{ReactRunner, build_react_runner};
use loom::agent::react::ReactBuildConfig;
use loom::stream::{StreamEvent, MessageChunk};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig::default();
    let runner = build_react_runner(&config, None, true).await?;

    let result = runner.stream_with_callback(
        "解释一下量子计算的基本原理",
        Some(|event| {
            match event {
                StreamEvent::Messages { chunk, metadata } => {
                    match chunk.kind {
                        loom::stream::MessageChunkKind::Message => {
                            print!("{}", chunk.content);
                        },
                        loom::stream::MessageChunkKind::Thinking => {
                            print!("[思考: {}]", chunk.content);
                        }
                    }
                },
                StreamEvent::TaskStart { node_id, .. } => {
                    println!("\n🚀 开始执行节点: {}", node_id);
                },
                StreamEvent::TaskEnd { node_id, result, .. } => {
                    match result {
                        Ok(()) => println!("✅ 节点完成: {}", node_id),
                        Err(e) => println!("❌ 节点失败: {} - {}", node_id, e),
                    }
                },
                _ => {}
            }
            // 回调函数需要返回异步结果
            async move { Ok(()) }
        })
    ).await?;

    println!("\n执行完成");
    Ok(())
}
```

### 消费不同类型的事件

```rust
use loom::agent::react::{ReactRunner, build_react_runner};
use loom::agent::react::ReactBuildConfig;
use loom::stream::StreamEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig::default();
    let runner = build_react_runner(&config, None, true).await?;

    let result = runner.stream_with_callback(
        "帮我查询天气并进行总结",
        Some(|event| {
            match event {
                // LLM 消息流
                StreamEvent::Messages { chunk, .. } => {
                    print!("{}", chunk.content);
                },
                
                // 工具调用事件
                StreamEvent::ToolCall { name, arguments, .. } => {
                    println!("\n🔧 调用工具: {} ({})", name, arguments);
                },
                StreamEvent::ToolStart { name, .. } => {
                    println!("⏳ 工具开始: {}", name);
                },
                StreamEvent::ToolOutput { name, content, .. } => {
                    println!("📤 工具输出: {} - {}", name, content);
                },
                StreamEvent::ToolEnd { name, result, is_error, .. } => {
                    if is_error {
                        println!("❌ 工具失败: {} - {}", name, result);
                    } else {
                        println!("✅ 工具完成: {}", name);
                    }
                },
                
                // Token 使用统计
                StreamEvent::Usage { prompt_tokens, completion_tokens, total_tokens, .. } => {
                    println!("\n📊 Token 使用: 输入 {}, 输出 {}, 总计 {}", 
                             prompt_tokens, completion_tokens, total_tokens);
                },
                
                // 状态更新
                StreamEvent::Values(state) => {
                    // 获取完整状态快照
                    println!("\n📋 当前状态: {} 条消息", state.messages.len());
                },
                
                _ => {}
            }
            async move { Ok(()) }
        })
    ).await?;

    Ok(())
}
```

### 实时输出显示

```rust
use loom::agent::got::{GotRunner, build_got_runner};
use loom::agent::react::ReactBuildConfig;
use loom::stream::StreamEvent;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig {
        got_config: loom::agent::react::GotRunnerConfig {
            adaptive: true,
            agot_llm_complexity: true,
        },
        ..Default::default()
    };
    
    let runner = build_got_runner(&config, None, true).await?;
    let mut stdout = io::stdout();

    let result = runner.stream_with_callback(
        "帮我制定一个完整的机器学习项目计划",
        Some(move |event| {
            let stdout = &mut stdout;
            
            match event {
                // GoT 特定事件
                StreamEvent::GotPlan { node_count, edge_count, node_ids } => {
                    writeln!(stdout, "📋 任务计划: {} 个节点, {} 条依赖", node_count, edge_count).unwrap();
                    writeln!(stdout, "   节点顺序: {:?}", node_ids).unwrap();
                },
                StreamEvent::GotNodeStart { node_id } => {
                    writeln!(stdout, "🚀 开始节点: {}", node_id).unwrap();
                },
                StreamEvent::GotNodeComplete { node_id, result_summary } => {
                    writeln!(stdout, "✅ 完成节点: {}", node_id).unwrap();
                    writeln!(stdout, "   结果摘要: {}", result_summary).unwrap();
                },
                StreamEvent::GotExpand { node_id, nodes_added, edges_added } => {
                    writeln!(stdout, "🔄 扩展节点: {} (+{} 子节点, +{} 依赖)", 
                             node_id, nodes_added, edges_added).unwrap();
                },
                
                // 通用消息流
                StreamEvent::Messages { chunk, .. } => {
                    write!(stdout, "{}", chunk.content).unwrap();
                    stdout.flush().unwrap();
                },
                
                // 使用量统计
                StreamEvent::Usage { total_tokens, .. } => {
                    writeln!(stdout, "\n📊 总 Token 使用: {}", total_tokens).unwrap();
                },
                
                _ => {}
            }
            async move { Ok(()) }
        })
    ).await?;

    writeln!(stdout, "\n🎉 任务完成！").unwrap();
    Ok(())
}
```

### 自定义事件处理

```rust
use loom::agent::react::{ReactRunner, build_react_runner};
use loom::agent::react::ReactBuildConfig;
use loom::stream::StreamEvent;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

#[derive(Clone)]
struct StreamStats {
    total_tokens: Arc<Mutex<u32>>,
    tool_calls: Arc<Mutex<Vec<String>>>,
    node_durations: Arc<Mutex<HashMap<String, std::time::Duration>>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stats = StreamStats {
        total_tokens: Arc::new(Mutex::new(0)),
        tool_calls: Arc::new(Mutex::new(Vec::new())),
        node_durations: Arc::new(Mutex::new(HashMap::new())),
    };
    
    let config = ReactBuildConfig::default();
    let runner = build_react_runner(&config, None, true).await?;

    let result = runner.stream_with_callback(
        "帮我分析最新的 AI 技术趋势",
        Some({
            let stats = stats.clone();
            move |event| {
                let stats = stats.clone();
                
                match event {
                    StreamEvent::Usage { total_tokens, .. } => {
                        *stats.total_tokens.lock().unwrap() = total_tokens;
                    },
                    StreamEvent::ToolCall { name, .. } => {
                        stats.tool_calls.lock().unwrap().push(name);
                    },
                    StreamEvent::TaskStart { node_id, .. } => {
                        stats.node_durations.lock().unwrap().insert(node_id.clone(), std::time::Instant::now().elapsed());
                    },
                    StreamEvent::Messages { chunk, .. } => {
                        print!("{}", chunk.content);
                    },
                    _ => {}
                }
                async move { Ok(()) }
            }
        })
    ).await?;

    // 输出统计信息
    println!("\n📊 执行统计:");
    println!("   总 Token 使用: {}", *stats.total_tokens.lock().unwrap());
    println!("   工具调用次数: {}", stats.tool_calls.lock().unwrap().len());
    println!("   调用的工具: {:?}", *stats.tool_calls.lock().unwrap());

    Ok(())
}
```

### 与 SSE/WebSocket 集成

```rust
use loom::agent::react::{ReactRunner, build_react_runner};
use loom::agent::react::ReactBuildConfig;
use loom::stream::StreamEvent;
use warp::{Filter, Rejection, Reply};
use std::convert::Infallible;

// SSE 端点
async fn sse_stream(user_message: String) -> Result<impl Reply, Rejection> {
    let config = ReactBuildConfig::default();
    let runner = build_react_runner(&config, None, true).await
        .map_err(|e| warp::reject::custom(e))?;
    
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    
    // 在后台任务中执行流式处理
    tokio::spawn(async move {
        let _ = runner.stream_with_callback(&user_message, Some(|event| {
            let event_json = serde_json::to_string(&event).unwrap_or_default();
            let _ = tx.send(format!("data: {}\n\n", event_json));
            async move { Ok(()) }
        })).await;
        let _ = tx.send("data: [DONE]\n\n".to_string());
    });
    
    // 返回 SSE 响应
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    Ok(warp::sse::reply(warp::sse::keep_alive().stream(stream)))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let route = warp::path("stream")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(|body: serde_json::Value| async move {
            let message = body["message"].as_str().unwrap_or("你好");
            sse_stream(message.to_string()).await
        });
    
    println!("🚀 SSE 服务器启动在 http://127.0.0.1:3030/stream");
    warp::serve(route).run(([127, 0, 0, 1], 3030)).await;
    
    Ok(())
}
```

## 最佳实践

### 事件处理优化
- 只处理需要的事件类型，避免不必要的计算
- 使用异步回调函数处理耗时操作
- 合理设置通道缓冲区大小，避免积压
- 实现错误处理和重连机制

### 实时用户体验
- 优先使用 `Messages` 事件实现逐字显示
- 区分 `Message` 和 `Thinking` 类型，提供不同展示效果
- 实现平滑的滚动和自动跟随
- 提供暂停/继续功能

### 性能监控
- 监控 `Usage` 事件控制成本
- 跟踪 `TaskStart/TaskEnd` 事件分析性能瓶颈
- 记录工具调用的成功率和耗时
- 设置合理的超时和取消机制

### 调试和日志
- 使用 `Debug` 模式获取详细事件流
- 记录关键事件的完整状态快照
- 实现事件回放功能用于问题排查
- 区分开发和生产环境的事件详细程度

---

## 相关概念

- **ReAct 运行模式**: 基础的循环推理模式
- **GoT 运行模式**: 图状思维推理模式
- **工具系统**: 工具开发和集成指南
- **状态管理**: 状态流转和检查点机制

---

**下一页**: [ReAct 运行模式](../core/react.md) | [GoT 运行模式](../core/got.md) | [工具系统](../core/tool-system.md)