# 节点与中间件

Loom 框架的核心抽象，提供基于图的状态转换和可组合的执行中间件系统。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 自定义图结构 | ✅ 最佳选择 | 构建复杂的非标准工作流 |
| 跨切面关注点 | ✅ 原生支持 | 日志、监控、权限控制等 |
| 状态转换逻辑 | ✅ 基础抽象 | 实现业务逻辑的核心组件 |
| 简单线性流程 | ⚠️ 可用但过度 | 简单任务可能过度设计 |

## 核心概念

### Node<S> 节点抽象

节点是状态图中的执行单元，接收输入状态并返回转换后的状态和路由指令。

```rust
#[async_trait]
pub trait Node<S>: Send + Sync
where
    S: Clone + Send + Sync + Debug + 'static,
{
    fn id(&self) -> &str; // 节点唯一标识符
    
    async fn run(&self, state: S) -> Result<(S, Next), AgentError>;
    
    // 带上下文的变体（可选）
    async fn run_with_context(
        &self,
        state: S,
        ctx: &RunContext<S>,
    ) -> Result<(S, Next), AgentError> {
        self.run(state).await
    }
}
```

### Next 路由枚举

控制图执行流向的指令：

```rust
pub enum Next {
    /// 按线性边序继续；当前节点是最后一个时等价于 End
    Continue,
    /// 跳转到指定节点（如 ReAct 循环：observe → think）
    Node(String),
    /// 停止执行并返回当前状态
    End,
}
```

### RunContext 运行上下文

为节点执行提供运行时服务和能力：

```rust
pub struct RunContext<S> {
    pub config: RunnableConfig,                           // 线程ID、用户ID等配置
    pub stream_tx: Option<mpsc::Sender<StreamEvent<S>>>,  // 流式事件通道
    pub stream_mode: HashSet<StreamMode>,                 // 启用的流模式
    pub managed_values: HashMap<String, Arc<dyn ManagedValue<Value, S>>>,
    
    // 运行时集成字段
    pub store: Option<Arc<dyn Store>>,                    // 长期记忆存储
    pub previous: Option<S>,                              // 前置状态
    pub runtime_context: Option<serde_json::Value>,       // 自定义上下文
    pub cancellation: Option<CancellationToken>,
    pub any_stream_event_sender: Option<Arc<dyn Fn(AnyStreamEvent) + Send + Sync>>,
}
```

### NodeMiddleware 中间件

实现节点的 AOP（面向切面编程）模式，在节点执行前后注入逻辑：

```rust
#[async_trait]
pub trait NodeMiddleware<S>: Send + Sync
where
    S: Clone + Send + Sync + Debug + 'static,
{
    async fn around_run(
        &self,
        node_id: &str,
        state: S,
        inner: Box<dyn FnOnce(S) -> Pin<Box<dyn Future<Output = Result<(S, Next), AgentError>> + Send>> + Send>,
    ) -> Result<(S, Next), AgentError>;
}
```

### START 和 END 常量

图的入口和出口哨兵：

```rust
/// 图入口哨兵：在 `add_edge(START, first_node_id)` 中使用
pub const START: &str = "__start__";

/// 图出口哨兵：在 `add_edge(last_node_id, END)` 中使用
pub const END: &str = "__end__";
```

## 代码示例

### 实现自定义节点

```rust
use loom::graph::{Node, Next, StateGraph, START, END};
use loom::error::AgentError;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct MyState {
    counter: i32,
    messages: Vec<String>,
}

struct CounterNode {
    increment: i32,
}

#[async_trait]
impl Node<MyState> for CounterNode {
    fn id(&self) -> &str {
        "counter"
    }

    async fn run(&self, state: MyState) -> Result<(MyState, Next), AgentError> {
        let mut new_state = state;
        new_state.counter += self.increment;
        new_state.messages.push(format!("计数增加到 {}", new_state.counter));
        
        // 根据计数决定继续还是结束
        if new_state.counter >= 10 {
            Ok((new_state, Next::End))
        } else {
            Ok((new_state, Next::Continue))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<MyState>::new();
    
    let counter_node = Arc::new(CounterNode { increment: 2 });
    graph.add_node("counter", counter_node);
    graph.add_edge(START, "counter");
    graph.add_edge("counter", END);
    
    let compiled = graph.compile()?;
    let initial_state = MyState { counter: 0, messages: vec![] };
    let result = compiled.invoke(initial_state).await?;
    
    println!("最终计数: {}", result.counter);
    println!("消息: {:?}", result.messages);
    
    Ok(())
}
```

### 使用 NodeFunc 快速创建节点

```rust
use loom::graph::{NodeFunc, StateGraph, START, END};
use std::sync::Arc;

#[derive(Clone, Debug)]
struct ProcessState {
    data: String,
    processed: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<ProcessState>::new();
    
    // 使用闭包快速创建节点
    let data_processor = NodeFunc::new(
        "process_data",
        |mut state: ProcessState| async move {
            state.data = state.data.to_uppercase();
            state.processed = true;
            Ok((state, Next::Continue))
        }
    );
    
    graph.add_node("process", Arc::new(data_processor));
    graph.add_edge(START, "process");
    graph.add_edge("process", END);
    
    let compiled = graph.compile()?;
    let initial_state = ProcessState { 
        data: "hello world".to_string(), 
        processed: false 
    };
    let result = compiled.invoke(initial_state).await?;
    
    println!("处理结果: {}", result.data); // "HELLO WORLD"
    
    Ok(())
}
```

### 实现节点中间件

```rust
use loom::graph::{Node, NodeMiddleware, Next, RunContext, StateGraph, START, END};
use loom::error::AgentError;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Debug)]
struct TimedState {
    value: String,
    execution_times: Vec<String>,
}

struct TimingMiddleware;

#[async_trait]
impl<S> NodeMiddleware<S> for TimingMiddleware
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    async fn around_run(
        &self,
        node_id: &str,
        state: S,
        inner: Box<dyn FnOnce(S) -> Pin<Box<dyn Future<Output = Result<(S, Next), AgentError>> + Send>> + Send>,
    ) -> Result<(S, Next), AgentError> {
        let start = Instant::now();
        println!("[Timing] 节点 {} 开始执行", node_id);
        
        let result = inner(state).await;
        
        let duration = start.elapsed();
        println!("[Timing] 节点 {} 执行耗时: {:?}", node_id, duration);
        
        result
    }
}

// 使用带类型参数的中间件
struct StateTimingMiddleware;

#[async_trait]
impl NodeMiddleware<TimedState> for StateTimingMiddleware {
    async fn around_run(
        &self,
        node_id: &str,
        mut state: TimedState,
        inner: Box<dyn FnOnce(TimedState) -> Pin<Box<dyn Future<Output = Result<(TimedState, Next), AgentError>> + Send>> + Send>,
    ) -> Result<(TimedState, Next), AgentError> {
        let start = Instant::now();
        
        let (result_state, next) = inner(state.clone()).await?;
        
        let duration = start.elapsed();
        state = result_state;
        state.execution_times.push(format!("{}: {:?}", node_id, duration));
        
        Ok((state, next))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<TimedState>::new();
    
    let simple_node = Arc::new(NodeFunc::new(
        "work",
        |mut state: TimedState| async move {
            state.value.push_str("-processed");
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            Ok((state, Next::Continue))
        }
    ));
    
    graph.add_node("work", simple_node);
    graph.add_edge(START, "work");
    graph.add_edge("work", END);
    
    // 使用中间件编译
    let compiled = graph
        .compile_with_checkpointer_and_middleware(
            Arc::new(loom::checkpoint::MemoryCheckpointer::new()),
            Arc::new(StateTimingMiddleware)
        )?;
    
    let initial_state = TimedState { 
        value: "test".to_string(), 
        execution_times: vec![] 
    };
    let result = compiled.invoke(initial_state).await?;
    
    println!("最终值: {}", result.value);
    println!("执行时间记录: {:?}", result.execution_times);
    
    Ok(())
}
```

### 使用 Next 变量进行路由

```rust
use loom::graph::{Node, Next, StateGraph, START, END};
use loom::error::AgentError;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct RouterState {
    step: u32,
    path: Vec<String>,
}

struct RouterNode;

#[async_trait]
impl Node<RouterState> for RouterNode {
    fn id(&self) -> &str {
        "router"
    }

    async fn run(&self, state: RouterState) -> Result<(RouterState, Next), AgentError> {
        let mut new_state = state;
        new_state.step += 1;
        
        match new_state.step {
            1 => {
                new_state.path.push("branch_a".to_string());
                Ok((new_state, Next::Node("process_a".to_string())))
            }
            2 => {
                new_state.path.push("branch_b".to_string());
                Ok((new_state, Next::Node("process_b".to_string())))
            }
            3 => {
                new_state.path.push("finish".to_string());
                Ok((new_state, Next::End))
            }
            _ => Ok((new_state, Next::End))
        }
    }
}

struct ProcessNode {
    name: &'static str,
}

#[async_trait]
impl Node<RouterState> for ProcessNode {
    fn id(&self) -> &str {
        self.name
    }

    async fn run(&self, state: RouterState) -> Result<(RouterState, Next), AgentError> {
        let mut new_state = state;
        new_state.path.push(format!("processed_{}", self.name));
        
        // 回到路由器
        Ok((new_state, Next::Node("router".to_string())))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<RouterState>::new();
    
    graph.add_node("router", Arc::new(RouterNode));
    graph.add_node("process_a", Arc::new(ProcessNode { name: "process_a" }));
    graph.add_node("process_b", Arc::new(ProcessNode { name: "process_b" }));
    
    graph.add_edge(START, "router");
    graph.add_edge("process_a", END);  // 可能的出口
    graph.add_edge("process_b", END);  // 可能的出口
    
    let compiled = graph.compile()?;
    let initial_state = RouterState { step: 0, path: vec![] };
    let result = compiled.invoke(initial_state).await?;
    
    println!("执行路径: {:?}", result.path);
    println!("最终步骤: {}", result.step);
    
    Ok(())
}
```

### 使用 RunContext 访问运行时服务

```rust
use loom::graph::{Node, Next, RunContext, StateGraph, START, END};
use loom::error::AgentError;
use loom::store::Store;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct ContextState {
    data: String,
    from_store: Option<String>,
}

struct ContextAwareNode;

#[async_trait]
impl Node<ContextState> for ContextAwareNode {
    fn id(&self) -> &str {
        "context_aware"
    }

    async fn run_with_context(
        &self,
        state: ContextState,
        ctx: &RunContext<ContextState>,
    ) -> Result<(ContextState, Next), AgentError> {
        let mut new_state = state;
        
        // 访问存储服务
        if let Some(store) = &ctx.store {
            if let Ok(value) = store.get("user_data").await {
                new_state.from_store = Some(value);
            }
        }
        
        // 发送流式事件
        if ctx.is_streaming_mode(loom::graph::StreamMode::Tokens) {
            let _ = ctx.emit_message("处理中...", "context_aware").await;
        }
        
        // 获取自定义配置
        if let Some(user_id) = ctx.config.user_id {
            new_state.data.push_str(&format!("-用户:{}", user_id));
        }
        
        Ok((new_state, Next::Continue))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = StateGraph::<ContextState>::new();
    
    let context_node = Arc::new(ContextAwareNode);
    graph.add_node("context_aware", context_node);
    graph.add_edge(START, "context_aware");
    graph.add_edge("context_aware", END);
    
    let compiled = graph.compile()?;
    let initial_state = ContextState { 
        data: "test".to_string(), 
        from_store: None 
    };
    let result = compiled.invoke(initial_state).await?;
    
    println!("处理结果: {}", result.data);
    println!("存储数据: {:?}", result.from_store);
    
    Ok(())
}
```

## 最佳实践

### 节点设计
- 保持节点单一职责，每个节点专注一个特定转换
- 避免在节点中直接访问外部服务，使用 RunContext 提供的能力
- 为节点提供描述性的 ID，便于调试和监控

### 路由控制
- 使用 `Next::Continue` 保持图的线性可读性
- 使用 `Next::Node(id)` 实现循环和条件跳转
- 确保所有路径最终能到达 `END` 节点，避免死循环

### 中间件应用
- 将日志、监控、性能分析等横切关注点实现为中间件
- 中间件应该无状态或线程安全，避免共享可变状态
- 使用中间件链实现多层横切逻辑

### 错误处理
- 节点应该返回 `AgentError` 而非 panic
- 在中间件中统一处理错误，提供一致的错误响应
- 考虑实现重试中间件处理临时性故障

---

## 相关概念

- **StateGraph**: 状态图的构建和编译
- **Checkpointer**: 状态持久化和恢复机制
- **StreamEvent**: 流式输出事件系统
- **Store**: 长期记忆存储接口

---

**下一页**: [状态图系统](./state-graph.md) | [检查点机制](./checkpoint.md) | [运行模式概览](./run-modes.md)