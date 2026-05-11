---
sidebar_position: 4
title: "DUP 运行模式"
description: "理解-规划-行动-观察循环"
---

# DUP 运行模式

基于理解-规划-行动-观察循环的增强智能体运行模式，在标准 ReAct 循环前增加深度理解阶段，提升复杂任务处理质量。

## DUP vs ReAct 对比

| 特性 | DUP | ReAct | 说明 |
|------|-----|-------|------|
| 初始阶段 | Understand → Plan | Think | DUP 先分析再规划 |
| 循环结构 | Understand → Plan → Act → Observe | Think → Act → Observe | DUP 多一个理解阶段 |
| 适用场景 | 复杂多步骤任务 | 一般推理任务 | DUP 适合需要深度理解的场景 |
| 消息历史 | 包含结构化理解 | 直接对话 | DUP 提供更多上下文 |
| 计划质量 | 更高 | 标准 | DUP 通过理解阶段提升计划质量 |

## 核心概念

### DUP 循环流程

DUP 在标准 ReAct 循环前增加了理解阶段：

1. **Understand (理解节点)**: LLM 深度分析用户请求，提取核心目标和约束
2. **Plan (规划节点)**: 基于理解结果制定执行计划  
3. **Act (行动节点)**: 执行工具调用（与 ReAct 相同）
4. **Observe (观察节点)**: 整合结果，准备下一轮规划（与 ReAct 相同）

### 状态结构

**DupState**: 组合 ReAct 状态和理解输出
```rust
pub struct DupState {
    pub core: ReActState,                  // 核心执行状态
    pub understood: Option<UnderstandOutput>, // 理解结果
}

pub struct UnderstandOutput {
    pub core_goal: String,           // 用户核心目标
    pub key_constraints: Vec<String>, // 关键约束条件
    pub relevant_context: String,     // 相关上下文信息
}
```

### 执行流程对比

**ReAct 流程**:
```
START → think → tools_condition → act/observe → think
```

**DUP 流程**:
```
START → understand → plan → tools_condition → act/observe → plan
```

## 代码示例

### 基础 DUP 智能体

```rust
use loom::agent::dup::DupRunner;
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::llm::{LlmClient, ChatOpenAI};
use loom::tools::ToolSource;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 配置 LLM 客户端
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    // 配置工具源
    let tool_source = Box::new(loom::tools::MemoryToolSource::new());

    // 创建 DUP 运行器
    let runner = DupRunner::new(
        llm,
        tool_source,
        None,  // checkpointer
        None,  // store
        None,  // runnable_config
        None,  // system_prompt
        None,  // approval_policy
        None,  // cancellation
        false, // verbose
    )?;

    // 运行 DUP 智能体
    let result = runner.invoke("帮我分析这个复杂的市场营销策略，并制定详细的执行计划").await?;

    println!("最终回复: {:?}", result.last_assistant_reply());
    println!("理解结果: {:?}", result.understood);
    
    Ok(())
}
```

### 通过配置构建 DUP

```rust
use loom::agent::react::{build_dup_runner, ReactBuildConfig};
use loom::agent::react::config::BuiltinToolFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ReactBuildConfig {
        model: "gpt-4o".to_string(),
        builtin_tools: BuiltinToolFilter::All,
        system_prompt: Some("你是一个专业的项目管理助手".to_string()),
        ..Default::default()
    };

    let runner = build_dup_runner(&config, None, true).await?;
    
    let result = runner.invoke("制定一个产品发布的完整计划").await?;
    
    println!("核心目标: {}", result.understood.unwrap().core_goal);
    println!("总规划轮次: {}", result.core.turn_count);
    
    Ok(())
}
```

### 流式输出示例

```rust
use loom::agent::dup::DupRunner;
use loom::llm::{LlmClient, ChatOpenAI};
use loom::agent::dup::runner::DupStreamEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    let tool_source = Box::new(loom::tools::MemoryToolSource::new());
    let runner = DupRunner::new(llm, tool_source, None, None, None, None, None, None, false)?;

    let result = runner.stream_with_callback(
        "分析这个技术架构设计并提出改进建议",
        Some(|event| {
            match event {
                DupStreamEvent::UnderstandStart => println!("开始理解任务..."),
                DupStreamEvent::UnderstandContent(text) => print!("{}", text),
                DupStreamEvent::PlanStart => println!("\n开始规划执行步骤..."),
                DupStreamEvent::PlanToken(token) => print!("{}", token),
                DupStreamEvent::ToolCall(call) => println!("\n调用工具: {}", call.name),
                DupStreamEvent::ToolResult(result) => println!("工具结果: {}", result.content),
                _ => {}
            }
            async move { Ok(()) }
        })
    ).await?;

    println!("\n最终状态: 理解={}, 规划轮次={}", 
        result.final_state.understood.is_some(),
        result.final_state.core.turn_count
    );
    
    Ok(())
}
```

### 带持久化的 DUP

```rust
use loom::agent::dup::DupRunner;
use loom::llm::{LlmClient, ChatOpenAI};
use loom::persistence::SqliteCheckpointer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    let tool_source = Box::new(loom::tools::MemoryToolSource::new());
    
    // 配置持久化检查点
    let checkpointer = Arc::new(SqliteCheckpointer::new(
        "sqlite:./dup_checkpoints.db".to_string()
    ).await?);

    let runner = DupRunner::new(
        llm,
        tool_source,
        Some(checkpointer),
        None,
        None,
        None,
        None,
        None,
        false,
    )?;

    // 运行可恢复的 DUP 任务
    let result = runner.invoke("执行这个复杂的业务分析任务，可能需要多次交互").await?;
    
    println!("任务完成，状态已保存");
    println!("核心目标: {}", result.understood.unwrap().core_goal);
    
    Ok(())
}
```

### 带人工审批的 DUP

```rust
use loom::agent::dup::DupRunner;
use loom::llm::{LlmClient, ChatOpenAI};
use loom::approval::ApprovalPolicy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let llm = Arc::new(ChatOpenAI::new(
        "gpt-4o".to_string(),
        "your-api-key".to_string(),
    ));

    let tool_source = Box::new(loom::tools::MemoryToolSource::new());
    
    // 配置人工审批策略
    let approval_policy = ApprovalPolicy::new()
        .require_tool_approval(true)
        .require_plan_approval(true);

    let runner = DupRunner::new(
        llm,
        tool_source,
        None,
        None,
        None,
        None,
        Some(approval_policy),
        None,
        false,
    )?;

    let result = runner.invoke("制定并执行这个重要的业务决策").await?;
    
    println!("审批流程完成，最终结果: {:?}", result.last_assistant_reply());
    
    Ok(())
}
```

## 理解阶段详解

### UnderstandNode 工作原理

理解阶段通过专门的系统提示词引导 LLM 进行深度分析：

```rust
// 内部实现（loom/src/agent/dup/understand_node.rs）
const DUP_UNDERSTAND_PROMPT: &str = r#"
You are an analytical assistant. Analyze the user's request and provide:
1. Core goal: What does the user ultimately want to achieve?
2. Key constraints: What limitations or requirements must be considered?
3. Relevant context: What background information is important?

Respond in JSON format:
{
  "core_goal": "...",
  "key_constraints": ["...", "..."],
  "relevant_context": "..."
}
"#;
```

### 理解结果的使用

理解结果会被格式化后添加到消息历史中：

```rust
// 理解结果会被添加到对话历史
let summary = format!(
    "**Understanding**\n- Core goal: {}\n- Constraints: {:?}\n- Context: {}",
    understood.core_goal, 
    understood.key_constraints, 
    understood.relevant_context
);

// 这样规划阶段的 LLM 就能获得结构化的上下文
state.core.messages.push(Message::assistant(summary));
```

## 最佳实践

### 何时选择 DUP

**选择 DUP 的场景**:
- 任务复杂度高，需要前期深度分析
- 涉及多个约束条件和背景信息
- 需要系统化的规划过程
- 任务成功与否高度依赖初始理解质量

**选择 ReAct 的场景**:
- 简单直接的问答任务
- 实时性要求高，不能容忍额外延迟
- 任务结构相对简单，无需深度规划

### 配置优化

**系统提示词定制**:
```rust
let config = ReactBuildConfig {
    system_prompt: Some(
        "你是一个专业的业务分析师，擅长处理复杂的企业级任务".to_string()
    ),
    ..Default::default()
};
```

**模型选择建议**:
- 使用能力较强的模型（如 GPT-4o）进行理解阶段
- 理解质量直接影响后续规划效果
- 考虑为不同阶段使用不同模型

### 性能与效果平衡

**减少理解阶段开销**:
- 对于相似类型任务，可以缓存理解结果
- 适当调整系统提示词长度
- 考虑使用更快的模型进行初步理解

**提升规划质量**:
- 确保理解阶段包含充分的上下文信息
- 在规划阶段提供明确的反馈机制
- 监控理解结果的质量指标

### 错误处理与恢复

**理解失败处理**:
```rust
// 配置重试机制
let runner = DupRunner::new(
    llm,
    tool_source,
    None,
    None,
    Some(RunnableConfig {
        max_retries: 3,
        retry_delay: Duration::from_secs(1),
        ..Default::default()
    }),
    None,
    None,
    None,
    false,
)?;
```

**检查点恢复**:
- 使用持久化检查点支持长时间任务恢复
- 在关键节点保存状态
- 提供任务进度查询接口

---

## 相关概念

- **ReAct 运行模式**: DUP 的基础，理解-行动-观察循环
- **ToT 运行模式**: 树状思维推理模式，另一种复杂任务处理方式
- **GoT 运行模式**: 图状思维推理模式，更灵活的思维结构
- **LLM 客户端**: 理解和规划阶段的 LLM 调用

---

**下一页**: [ReAct 运行模式](./react.md) | [ToT 运行模式](./tot.md) | [LLM 客户端](./llm-client.md)