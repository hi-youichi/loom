---
sidebar_position: 1
title: "State Graph - 状态图"
description: "基于状态流转的图结构 Agent 执行框架"
---

# State Graph - 状态图

基于状态流转的图结构Agent执行框架。

## 使用场景

| 场景 | 描述 |
|------|------|
| 多步骤任务处理 | 需要按顺序或条件执行多个处理步骤的复杂任务 |
| 工作流编排 | 需要动态路由和分支控制的业务流程 |
| Agent链式调用 | 多个Agent协作完成任务的场景 |
| 状态追踪 | 需要维护和传递单一状态类型贯穿整个执行过程 |

## 核心概念

State Graph是Loom框架的核心执行模型，采用图结构来组织和管理Agent的执行流程。通过`StateGraph<S>`构建器，你可以定义节点（Node）、边（Edge）和条件路由，最终编译为`CompiledStateGraph<S>`进行执行。

状态图的核心机制是单一状态类型`S`在所有节点间流转。每个节点接收当前状态，处理后返回更新后的状态和下一个执行指示（`Next`枚举）。`Next::Continue`沿线性边继续，`Next::Node(id)`跳转到指定节点，`Next::End`终止执行。图结构使用`START`和`END`哨兵值作为入口和出口，确保执行流程的完整性。

## 代码示例

### 基础线性图

```rust
use std::sync::Arc;
use async_trait::async_trait;
use loom::graph::{StateGraph, Node, Next, START, END};

#[derive(Clone, Debug)]
struct QuestionState {
    question: String,
    answer: String,
}

struct QuestionAgent {
    name: String,
}

#[async_trait]
impl Node<QuestionState> for QuestionAgent {
    fn id(&self) -> &str {
        &self.name
    }

    async fn run(&self, state: QuestionState) -> Result<(QuestionState, Next), Box<dyn std::error::Error>> {
        println!("{} 处理问题: {}", self.name, state.question);
        let answer = format!("{}的回答", self.name);
        Ok((QuestionState { answer, ..state }, Next::Continue))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<QuestionState>::new();
    
    graph
        .add_node("analyzer", Arc::new(QuestionAgent { name: "分析器".to_string() }))
        .add_node("solver", Arc::new(QuestionAgent { name: "求解器".to_string() }))
        .add_edge(START, "analyzer")
        .add_edge("analyzer", "solver")
        .add_edge("solver", END);
    
    let compiled = graph.compile()?;
    let initial_state = QuestionState {
        question: "什么是状态图？".to_string(),
        answer: String::new(),
    };
    
    let final_state = compiled.invoke(initial_state, None).await?;
    println!("最终答案: {}", final_state.answer);
    
    Ok(())
}
```

### 带条件路由的图

```rust
use std::sync::Arc;
use std::collections::HashMap;
use async_trait::async_trait;
use loom::graph::{StateGraph, Node, Next, START, END};

#[derive(Clone, Debug)]
struct TaskState {
    task_type: String,
    complexity: i32,
    result: String,
}

struct TaskClassifier;

#[async_trait]
impl Node<TaskState> for TaskClassifier {
    fn id(&self) -> &str {
        "classifier"
    }

    async fn run(&self, state: TaskState) -> Result<(TaskState, Next), Box<dyn std::error::Error>> {
        println!("分类任务: {} (复杂度: {})", state.task_type, state.complexity);
        let next_node = if state.complexity > 5 {
            "complex_handler"
        } else {
            "simple_handler"
        };
        Ok((state, Next::Node(next_node.to_string())))
    }
}

struct SimpleHandler;

#[async_trait]
impl Node<TaskState> for SimpleHandler {
    fn id(&self) -> &str {
        "simple_handler"
    }

    async fn run(&self, state: TaskState) -> Result<(TaskState, Next), Box<dyn std::error::Error>> {
        println!("简单处理模式");
        Ok((TaskState { result: "简单任务完成".to_string(), ..state }, Next::End))
    }
}

struct ComplexHandler;

#[async_trait]
impl Node<TaskState> for ComplexHandler {
    fn id(&self) -> &str {
        "complex_handler"
    }

    async fn run(&self, state: TaskState) -> Result<(TaskState, Next), Box<dyn std::error::Error>> {
        println!("复杂处理模式");
        Ok((TaskState { result: "复杂任务完成".to_string(), ..state }, Next::End))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<TaskState>::new();
    
    graph
        .add_node("classifier", Arc::new(TaskClassifier))
        .add_node("simple_handler", Arc::new(SimpleHandler))
        .add_node("complex_handler", Arc::new(ComplexHandler))
        .add_edge(START, "classifier");
    
    let path_map: HashMap<String, String> = [
        ("simple_handler".into(), END.into()),
        ("complex_handler".into(), END.into()),
    ].into_iter().collect();
    
    graph.add_conditional_edges(
        "classifier",
        Arc::new(|state: &TaskState| {
            if state.complexity > 5 {
                "complex_handler".to_string()
            } else {
                "simple_handler".to_string()
            }
        }),
        Some(path_map),
    );
    
    let compiled = graph.compile()?;
    
    let complex_task = TaskState {
        task_type: "数据分析".to_string(),
        complexity: 8,
        result: String::new(),
    };
    
    let result = compiled.invoke(complex_task, None).await?;
    println!("复杂任务结果: {}", result.result);
    
    Ok(())
}
```

### 编译和执行

```rust
use std::sync::Arc;
use async_trait::async_trait;
use loom::graph::{StateGraph, CompiledStateGraph, Node, Next, START, END};

#[derive(Clone, Debug)]
struct ProcessState {
    step: i32,
    data: String,
}

struct StepProcessor {
    step_num: i32,
}

#[async_trait]
impl Node<ProcessState> for StepProcessor {
    fn id(&self) -> &str {
        &format!("step_{}", self.step_num)
    }

    async fn run(&self, state: ProcessState) -> Result<(ProcessState, Next), Box<dyn std::error::Error>> {
        println!("执行步骤 {}", self.step_num);
        let new_data = format!("{} -> 步骤{}", state.data, self.step_num);
        Ok((
            ProcessState {
                step: self.step_num,
                data: new_data,
            },
            Next::Continue,
        ))
    }
}

fn build_processing_graph() -> Result<CompiledStateGraph<ProcessState>, Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<ProcessState>::new();
    
    graph
        .add_node("step_1", Arc::new(StepProcessor { step_num: 1 }))
        .add_node("step_2", Arc::new(StepProcessor { step_num: 2 }))
        .add_node("step_3", Arc::new(StepProcessor { step_num: 3 }))
        .add_edge(START, "step_1")
        .add_edge("step_1", "step_2")
        .add_edge("step_2", "step_3")
        .add_edge("step_3", END);
    
    graph.compile()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let compiled_graph = build_processing_graph()?;
    
    let initial_state = ProcessState {
        step: 0,
        data: "初始数据".to_string(),
    };
    
    println!("开始处理...");
    let final_state = compiled_graph.invoke(initial_state, None).await?;
    
    println!("处理完成!");
    println!("最终状态: {:?}", final_state);
    
    Ok(())
}
```

## 边类型决策表

| 边类型 | 使用场景 | 实现方式 | 执行特点 |
|--------|----------|----------|----------|
| 线性边 | 固定顺序执行 | `add_edge(from, to)` | 简单直接，按预设路径执行 |
| 条件边 | 动态路由选择 | `add_conditional_edges()` | 基于状态或条件函数动态选择下一节点 |
| 条件函数 | 复杂逻辑判断 | 在Node中返回`Next::Node(id)` | 节点内部决定下一跳，最灵活 |

## 最佳实践

✅ **推荐做法**
- 为每个节点使用描述性ID，便于调试和维护
- 保持状态类型简洁，避免包含不必要的数据
- 使用条件边处理复杂的路由逻辑，保持节点职责单一
- 在编译阶段验证图结构，捕获潜在错误
- 为状态类型实现`Clone`和`Debug`，便于调试和状态追踪

⚠️ **避免模式**
- 在单个状态图中混合过多不同的状态类型
- 创建过于复杂的嵌套条件逻辑，考虑拆分为多个子图
- 忽略错误处理，所有节点应返回适当的错误类型
- 在状态中存储临时数据，状态应包含业务核心数据
- 过度使用`Next::Node()`进行跳转，优先使用条件边

## 页面边界

**本页面涵盖：**
- StateGraph的基本概念和构建模式
- 节点、边和条件路由的使用方法
- 图的编译和执行流程
- 线性和条件边的区别与选择

**不包含：**
- 具体Node实现的详细设计模式
- 状态管理和持久化机制
- 错误处理和重试策略
- 性能优化和并发控制

## 下一步

- [Node Trait](./node.md) - 了解如何实现自定义节点
- [状态管理](./state-management.md) - 学习状态流转和管理机制
- [条件路由](./conditional-routing.md) - 深入了解动态路由策略
- [编译过程](./compilation.md) - 探索图编译的详细机制