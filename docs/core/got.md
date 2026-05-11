---
sidebar_position: 6
title: "GoT 运行模式"
description: "Graph of Thought 有向无环图推理"
---

# GoT (Graph of Thought) 运行模式

基于有向无环图（DAG）的复杂任务分解与执行模式，通过智能体协作实现多步骤任务的并行处理。

## 模式对比

| 特性 | ReAct | GoT | ToT |
|------|-------|-----|-----|
| 结构 | 线性循环 | DAG 图 | 树状探索 |
| 规划 | 无规划 | LLM 生成 DAG | 多候选生成 |
| 执行 | 顺序迭代 | 拓扑序并行 | 系统探索 |
| 适用场景 | 简单工具调用 | 多步骤任务 | 复杂推理 |
| 复杂度 | 低 | 中 | 高 |
| 并行能力 | ❌ | ✅ | ❌ |
| 依赖管理 | ❌ | ✅ | ❌ |

## 核心概念

### GoT 三阶段流程

1. **规划阶段 (Plan)**: `PlanGraphNode` 调用 LLM 生成 DAG 任务图
2. **执行阶段 (Execute)**: `ExecuteGraphNode` 按拓扑序执行子任务
3. **聚合阶段 (Aggregate)**: 汇总所有子任务结果

### 核心组件

**GotState**: 整体状态管理
```rust
pub struct GotState {
    pub input_message: String,                              // 原始用户消息
    pub task_graph: TaskGraph,                              // 任务 DAG 结构
    pub node_states: HashMap<String, TaskNodeState>,        // 各节点状态
}
```

**TaskGraph**: 任务图结构
```rust
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,              // 任务节点列表
    pub edges: Vec<(String, String)>,      // 依赖边 [from_id, to_id]
}
```

**TaskNode**: 单个任务节点
```rust
pub struct TaskNode {
    pub id: String,                        // 唯一标识符
    pub description: String,               // 任务描述
    pub tool_calls: Vec<ToolCall>,         // 工具调用记录
}
```

**TaskStatus**: 节点执行状态
```rust
pub enum TaskStatus {
    Pending,    // 等待执行
    Running,    // 正在执行
    Done,       // 执行完成
    Failed,     // 执行失败
}
```

## 代码示例

### 基础 GoT 智能体

```rust
use loom::agent::got::{GotRunner, build_got_runner};
use loom::agent::react::{ReactBuildConfig, GotRunnerConfig};
use loom::llm::{LlmClient, ChatOpenAI};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 配置 LLM 客户端
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    // 构建 GoT 配置
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        got_config: GotRunnerConfig {
            adaptive: false,      // 禁用自适应扩展
            agot_llm_complexity: false,
        },
        ..Default::default()
    };

    // 构建 GoT 运行器
    let runner = build_got_runner(&config, Some(llm), true).await?;

    // 执行复杂任务
    let result = runner.invoke("帮我分析并总结 2024 年人工智能发展报告").await?;

    println!("生成的任务图: {:?}", result.task_graph);
    println!("最终结果: {}", result.summary_result());

    Ok(())
}
```

### 启用自适应扩展 (AGoT)

```rust
use loom::agent::got::{GotRunner, build_got_runner};
use loom::agent::react::{ReactBuildConfig, GotRunnerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        got_config: GotRunnerConfig {
            adaptive: true,       // 启用 AGoT 自适应扩展
            agot_llm_complexity: true,  // 使用 LLM 评估复杂度
        },
        ..Default::default()
    };

    let runner = build_got_runner(&config, None, true).await?;

    // 复杂任务会自动分解和扩展
    let result = runner.invoke(
        "设计一个完整的机器学习系统架构，包括数据收集、模型训练、部署和监控"
    ).await?;

    println!("自适应扩展后的节点数: {}", result.task_graph.nodes.len());
    println!("任务完成情况: {:?}", result.node_states);

    Ok(())
}
```

### 流式输出 GoT 执行

```rust
use loom::agent::got::{GotRunner, build_got_runner};
use loom::agent::react::ReactBuildConfig;
use loom::agent::got::runner::StreamEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig::default();
    let runner = build_got_runner(&config, None, true).await?;

    let result = runner.stream_with_callback(
        "帮我调研并比较最新的开源大语言模型",
        Some(|event| {
            match event {
                StreamEvent::GotPlan(graph) => {
                    println!("📋 生成任务计划: {} 个节点", graph.nodes.len());
                },
                StreamEvent::GotNodeStart(node_id) => {
                    println!("🚀 开始执行节点: {}", node_id);
                },
                StreamEvent::GotNodeComplete(node_id, result) => {
                    println!("✅ 节点完成: {} - 长度: {} 字符", node_id, result.len());
                },
                StreamEvent::GotNodeFailed(node_id, error) => {
                    println!("❌ 节点失败: {} - 错误: {}", node_id, error);
                },
                _ => {}
            }
            async move { Ok(()) }
        })
    ).await?;

    println!("\n最终结果: {}", result.final_state.summary_result());

    Ok(())
}
```

### 直接使用 GotRunner

```rust
use loom::agent::got::{GotRunner, build_got_initial_state};
use loom::llm::{LlmClient, ChatOpenAI};
use loom::tools::{ToolSource, MemoryToolSource};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建 LLM 和工具源
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));
    let tool_source = Box::new(MemoryToolSource::new());

    // 直接创建 GotRunner
    let runner = GotRunner::new(
        llm,
        tool_source,
        None,           // checkpointer
        None,           // store
        None,           // runnable_config
        None,           // cancellation
        true,           // verbose
        true,           // adaptive
        true,           // agot_llm_complexity
    )?;

    // 构建初始状态
    let initial_state = build_got_initial_state(
        "帮我制定一个完整的区块链项目开发计划",
        None,
        None,
    ).await?;

    // 执行任务
    let result = runner.invoke_with_state(initial_state).await?;

    println!("任务图结构:");
    for node in &result.task_graph.nodes {
        println!("  - {}: {}", node.id, node.description);
    }

    println!("\n执行结果:");
    for (node_id, node_state) in &result.node_states {
        println!("  {}: {:?}", node_id, node_state.status);
    }

    Ok(())
}
```

### 自定义工具集成

```rust
use loom::agent::got::{GotRunner, build_got_runner};
use loom::agent::react::ReactBuildConfig;
use loom::tools::{ToolSource, CustomTool, ToolDefinition, MemoryToolSource};
use loom::state::Message;
use serde_json::json;

// 自定义研究工具
struct ResearchTool;
impl CustomTool for ResearchTool {
    fn name(&self) -> &str { "conduct_research" }
    
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "conduct_research".to_string(),
            description: "进行学术研究，收集相关资料和文献".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "topic": {"type": "string", "description": "研究主题"}
                },
                "required": ["topic"]
            }),
        }
    }
    
    async fn call(&self, args: serde_json::Value) -> Result<String, String> {
        let topic = args["topic"].as_str().unwrap_or("unknown");
        Ok(format!("关于 {} 的研究报告：包含最新文献综述...", topic))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建自定义工具源
    let mut tool_source = Box::new(MemoryToolSource::new());
    tool_source.add_tool(Arc::new(ResearchTool));

    let config = ReactBuildConfig {
        custom_tool_source: Some(tool_source),
        ..Default::default()
    };

    let runner = build_got_runner(&config, None, true).await?;
    let result = runner.invoke(
        "研究并总结量子计算在密码学中的应用"
    ).await?;

    println!("研究完成: {}", result.summary_result());

    Ok(())
}
```

## DAG 执行流程

```
用户输入复杂任务
    ↓
┌─────────────────┐
│ PlanGraphNode   │ ← LLM 生成 DAG 任务图
└────────┬────────┘
         │
         ↓
┌─────────────────┐
│ TaskGraph       │ ← 节点：[A, B, C, D]
│ Nodes: [A,B,C,D]│   边：[(A,B), (A,C), (B,D), (C,D)]
│ Edges: [...]    │
└────────┬────────┘
         │
         ↓
┌─────────────────┐
│ ExecuteGraphNode│ ← 拓扑排序执行
└────────┬────────┘
         │
         ├─→ 节点 A (无依赖) → 完成
         │
         ├─→ 节点 B,C (依赖 A) → 并行执行 → 完成
         │
         └─→ 节点 D (依赖 B,C) → 执行 → 完成
         │
         ↓
┌─────────────────┐
│ 汇总结果        │ ← 聚合所有节点输出
└─────────────────┘
```

## GoT vs ReAct vs ToT 使用建议

### 选择 ReAct 当：
- 任务相对简单，单次推理足够
- 主要是工具调用和简单决策
- 需要快速响应
- 示例：天气查询、简单问答

### 选择 GoT 当：
- 任务可自然分解为多个步骤
- 存在清晰的依赖关系
- 部分子任务可并行执行
- 需要结构化的问题解决方法
- 示例：项目规划、研究报告生成、复杂分析

### 选择 ToT 当：
- 需要探索多种解决方案
- 任务存在多个可能的推理路径
- 需要系统性的方案比较和回溯
- 示例：数学证明、创意写作、复杂推理

## 最佳实践

### 任务分解
- 确保任务描述清晰且可执行
- 控制节点数量在 2-8 个之间
- 避免过度分解导致的开销
- 利用 DAG 特性实现合理的并行执行

### AGoT 自适应扩展
- 对特别复杂的节点启用自适应扩展
- 合理设置复杂度评估阈值
- 监控扩展后的执行时间和资源消耗
- 在测试环境中验证扩展效果

### 错误处理
- 为关键节点设置重试机制
- 实现优雅的失败处理和回退策略
- 记录详细的节点执行日志
- 提供部分结果的汇总机制

### 性能优化
- 利用并行节点执行加速任务完成
- 合理设置子任务的最大轮次（默认 10 轮）
- 启用检查点支持长时间运行的任务
- 监控内存和 token 使用情况

---

## 相关概念

- **ReAct 运行模式**: 基础的循环推理模式
- **ToT (Tree of Thoughts)**: 树状思维推理模式
- **工具系统**: 工具开发和集成指南
- **状态管理**: 状态流转和检查点机制

---

**下一页**: [ToT 运行模式](./tot.md) | [ReAct 运行模式](./react.md) | [工具系统](./tool-system.md)