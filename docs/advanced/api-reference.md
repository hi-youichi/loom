---
sidebar_position: 1
title: "API 参考手册"
description: "完整的公共 API 文档"
---

# Loom API 参考手册

Loom 框架完整的公共 API 文档，按模块组织并提供详细的参数说明和使用示例。

## 1. 图模块 (Graph Module)

### StateGraph

状态图构建器，用于创建和编译智能体执行图。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | - | ✅ | 创建新的空状态图 |
| `with_store()` | `Arc<dyn Store>` | ❌ | 配置状态存储 |
| `with_middleware()` | `Arc<dyn NodeMiddleware<S>>` | ❌ | 添加节点中间件 |
| `with_state_updater()` | `BoxedStateUpdater<S>` | ❌ | 配置状态更新器 |
| `with_retry_policy()` | `RetryPolicy` | ❌ | 设置重试策略 |
| `with_interrupt_handler()` | `Arc<dyn InterruptHandler>` | ❌ | 添加中断处理器 |

#### 图构建方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `add_node()` | `id: impl Into<String>`, `node: Arc<dyn Node<S>>` | ✅ | 添加节点到图中 |
| `add_edge()` | `from_id: impl Into<String>`, `to_id: impl Into<String>` | ✅ | 添加节点间的边 |
| `add_conditional_edges()` | `source: impl Into<String>`, `path: ConditionalRouterFn<S>`, `path_map: Option<HashMap<String, String>>` | ✅ | 添加条件边 |

#### 编译方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `compile()` | - | ✅ | 编译图为可执行图 |
| `compile_with_checkpointer()` | `checkpointer: Arc<dyn Checkpointer<S>>` | ✅ | 编译并集成检查点功能 |
| `compile_with_middleware()` | `middleware: Arc<dyn NodeMiddleware<S>>` | ✅ | 编译并集成中间件 |

#### 示例用法

```rust
use loom::graph::{StateGraph, Node, Next, START, END};

let mut graph = StateGraph::new();
graph.add_node("think", Arc::new(think_node));
graph.add_node("act", Arc::new(act_node));
graph.add_edge(START, "think");
graph.add_edge("think", "act");
graph.add_edge("act", END);

let compiled = graph.compile()?;
```

### CompiledStateGraph

编译后的状态图，支持同步和流式执行。

#### 执行方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `invoke()` | `state: S`, `config: Option<RunnableConfig>` | ✅ | 同步执行图 |
| `invoke_with_context()` | `state: S`, `run_ctx: RunContext<S>` | ✅ | 带上下文执行 |
| `stream()` | `state: S`, `config: Option<RunnableConfig>`, `stream_mode: impl Into<HashSet<StreamMode>>`, `cancellation: Option<CancellationToken>`, `run_cancellation: Option<RunCancellation>`, `any_stream_event_sender: Option<...>` | ✅ | 流式执行 |

#### 示例用法

```rust
let result = compiled.invoke(initial_state, Some(config)).await?;

let stream = compiled.stream(
    initial_state, 
    Some(config), 
    [StreamMode::Values, StreamMode::Messages],
    None, None, None
);
```

### Node Trait

图节点接口，所有节点必须实现此 trait。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `id()` | - | ✅ | `&str` | 返回节点唯一标识符 |
| `run()` | `state: S` | ✅ | `Result<(S, Next), AgentError>` | 执行节点逻辑 |
| `run_with_context()` | `state: S`, `ctx: &RunContext<S>` | ❌ | `Result<(S, Next), AgentError>` | 带上下文执行 |

#### 示例用法

```rust
struct MyNode {
    id: String,
}

#[async_trait]
impl Node<ReActState> for MyNode {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, state: ReActState) -> Result<(ReActState, Next), AgentError> {
        // 节点逻辑
        Ok((state, Next::Continue))
    }
}
```

### Next Enum

节点执行后的下一个动作。

#### 变体

| 变体 | 描述 |
|------|------|
| `Continue` | 按线性边顺序继续执行 |
| `Node(String)` | 跳转到指定节点 |
| `End` | 停止图执行 |

#### 示例用法

```rust
match next {
    Next::Continue => println!("继续到下一个节点"),
    Next::Node(node_id) => println!("跳转到节点: {}", node_id),
    Next::End => println!("执行结束"),
}
```

### 常量

| 常量 | 类型 | 值 | 描述 |
|------|------|-----|------|
| `START` | `&str` | `"START"` | 图的起始节点标识符 |
| `END` | `&str` | `"END"` | 图的结束节点标识符 |

---

## 2. 智能体运行器 (Agent Runners)

### ReactRunner

ReAct 智能体运行器，支持工具调用和循环推理。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | `provider: Arc<dyn LlmProvider>`, `tool_source: Box<dyn ToolSource>`, `checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>`, `store: Option<Arc<dyn Store>>`, `runnable_config: Option<RunnableConfig>`, `system_prompt: String`, `approval_policy: Option<ApprovalPolicy>`, `compaction_config: Option<CompactionConfig>`, `_user_message_store: Option<Arc<dyn UserMessageStore>>`, `cancellation: Option<RunCancellation>`, `verbose: bool`, `title_provider: Option<Arc<dyn LlmProvider>>`, `title_headers: Option<LlmHeaders>` | ✅ | 创建新的 ReAct 运行器 |
| `with_cancellation()` | `cancellation: Option<RunCancellation>` | ❌ | 配置取消令牌 |

#### 执行方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `invoke()` | `user_message: &str` | ✅ | `Result<ReActState, RunError>` | 执行用户消息 |
| `invoke_with_config()` | `user_message: &str`, `config: Option<RunnableConfig>` | ❌ | `Result<ReActState, RunError>` | 带配置执行 |
| `stream_with_callback()` | `user_message: &str`, `on_event: Option<F>` | ✅ | `Result<StreamRunOutcome<ReActState>, RunError>` | 流式执行带回调 |
| `stream_with_config()` | `user_message: &str`, `config: Option<RunnableConfig>`, `on_event: Option<F>`, `any_stream_event_sender: Option<...>` | ✅ | `Result<StreamRunOutcome<ReActState>, RunError>` | 带配置的流式执行 |

#### 示例用法

```rust
let runner = ReactRunner::new(
    provider,
    tool_source,
    Some(checkpointer),
    Some(store),
    Some(config),
    "你是一个有用的助手".to_string(),
    None, None, None, None, false, None, None
)?;

let result = runner.invoke("帮我查询天气").await?;
```

### ReActState

ReAct 智能体的执行状态。

#### 字段

| 字段名 | 类型 | 描述 |
|--------|------|------|
| `model_config` | `ModelConfig` | 模型配置 |
| `messages` | `Vec<Message>` | 消息历史 |
| `last_reasoning_content` | `Option<String>` | 最后的推理内容 |
| `tool_calls` | `Vec<ToolCall>` | 工具调用列表 |
| `tool_results` | `Vec<ToolResult>` | 工具执行结果 |
| `turn_count` | `u32` | 循环轮次计数 |
| `approval_result` | `Option<bool>` | 审批结果 |
| `usage` | `Option<LlmUsage>` | 当前轮次使用量 |
| `total_usage` | `Option<LlmUsage>` | 总计使用量 |
| `think_count` | `u32` | 思考次数统计 |
| `summary` | `Option<String>` | 摘要信息 |
| `should_continue` | `bool` | 是否继续循环 |

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `apply_think()` | `content: String`, `reasoning_content: Option<String>`, `tool_calls: Vec<ToolCall>`, `response_usage: Option<LlmUsage>` | ✅ | `Self` | 应用思考结果到状态 |
| `last_assistant_reply()` | - | ✅ | `Option<String>` | 获取最后一条助手回复 |

#### 示例用法

```rust
let updated_state = state.apply_think(
    "这是我的思考结果".to_string(),
    Some("推理过程".to_string()),
    vec![],
    Some(LlmUsage { total_tokens: 100, ..Default::default() })
);
```

### build_react_runner

构建 ReAct 运行器的便捷函数。

#### 参数

| 参数名 | 类型 | 必需 | 描述 |
|--------|------|------|------|
| `config` | `&ReactBuildConfig` | ✅ | ReAct 构建配置 |
| `llm` | `Option<Box<dyn LlmClient>>` | ❌ | 自定义 LLM 客户端 |
| `verbose` | `bool` | ❌ | 是否启用详细日志 |

#### 返回类型
`Result<ReactRunner, BuildRunnerError>`

#### 示例用法

```rust
let config = ReactBuildConfig {
    model: "gpt-4o".to_string(),
    ..Default::default()
};

let runner = build_react_runner(&config, None, true).await?;
```

### Node 构造器

#### ThinkNode

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | `provider: Arc<dyn LlmProvider>` | ✅ | 创建思考节点 |

#### ActNode

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | `tool_source: Box<dyn ToolSource>` | ✅ | 创建行动节点 |
| `with_handle_tool_errors()` | `handle: HandleToolErrors` | ❌ | 配置错误处理策略 |
| `with_approval_policy()` | `policy: Option<ApprovalPolicy>` | ❌ | 设置审批策略 |

#### ObserveNode

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `with_loop()` | - | ✅ | 启用循环 |
| `with_loop_max_turns()` | `max_turns: u32` | ✅ | 启用循环并设置最大轮次 |
| `without_loop()` | - | ✅ | 禁用循环 |

#### 示例用法

```rust
let think_node = ThinkNode::new(provider);
let act_node = ActNode::new(tool_source)
    .with_handle_tool_errors(HandleToolErrors::Continue);
let observe_node = ObserveNode::with_loop_max_turns(10);
```

---

## 3. LLM 客户端 (LLM Client)

### LlmClient Trait

LLM 客户端接口，支持调用和流式响应。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `invoke()` | `messages: &[Message]` | ✅ | `Result<LlmResponse, AgentError>` | 非流式调用 |
| `invoke_stream()` | `messages: &[Message]`, `chunk_tx: Option<mpsc::Sender<MessageChunk>>` | ✅ | `Result<LlmResponse, AgentError>` | 流式调用 |
| `invoke_stream_with_tool_delta()` | `messages: &[Message]`, `chunk_tx: Option<mpsc::Sender<MessageChunk>>`, `tool_delta_tx: Option<mpsc::Sender<ToolCallDelta>>` | ✅ | `Result<LlmResponse, AgentError>` | 带工具调用流式调用 |
| `list_models()` | - | ✅ | `Result<Vec<ModelInfo>, AgentError>` | 列出可用模型 |

#### 示例用法

```rust
let response = llm_client.invoke(&messages).await?;

let (chunk_tx, mut chunk_rx) = mpsc::channel(100);
llm_client.invoke_stream(&messages, Some(chunk_tx)).await?;
```

### ChatOpenAI

OpenAI API 客户端实现。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | `base_url: String`, `api_key: String`, `model: String` | ✅ | 创建 OpenAI 客户端 |
| `with_temperature()` | `temp: f64` | ❌ | 设置温度参数 |
| `with_max_tokens()` | `max: u32` | ❌ | 设置最大 token 数 |
| `with_tool_choice()` | `choice: ToolChoiceMode` | ❌ | 设置工具调用模式 |
| `with_headers()` | `headers: LlmHeaders` | ❌ | 添加自定义 HTTP 头 |

#### 示例用法

```rust
let client = ChatOpenAI::new(
    "https://api.openai.com/v1/chat/completions".to_string(),
    "your-api-key".to_string(),
    "gpt-4o".to_string()
)
.with_temperature(0.7)
.with_tool_choice(ToolChoiceMode::Auto);
```

### MockLlm

测试用的 LLM 客户端模拟器。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | `content: String` | ✅ | 创建带有固定内容的模拟器 |
| `with_tool_calls()` | `calls: Vec<ToolCall>` | ❌ | 添加工具调用响应 |
| `with_reasoning_content()` | `content: String` | ❌ | 设置推理内容 |
| `with_usage()` | `usage: LlmUsage` | ❌ | 设置 token 使用量 |
| `enable_streaming()` | `enabled: bool` | ❌ | 启用/禁用流式响应 |
| `with_stateful_mode()` | `enabled: bool` | ❌ | 启用状态模式 |

#### 测试辅助方法

| 方法名 | 描述 |
|--------|------|
| `first_tools_then_end()` | 创建先返回工具调用然后返回结束的模拟器 |
| `get_time_example()` | 创建获取时间的示例模拟器 |

#### 示例用法

```rust
let mock = MockLlm::new("测试回复".to_string())
    .with_tool_calls(vec![tool_call])
    .with_usage(LlmUsage {
        total_tokens: 100,
        prompt_tokens: 50,
        completion_tokens: 50,
        ..Default::default()
    });
```

### LlmResponse

LLM 响应结构。

#### 字段

| 字段名 | 类型 | 描述 |
|--------|------|------|
| `content` | `String` | 主要回复内容 |
| `reasoning_content` | `Option<String>` | 推理过程内容 |
| `tool_calls` | `Vec<ToolCall>` | 工具调用列表 |
| `usage` | `Option<LlmUsage>` | Token 使用统计 |

### ToolChoiceMode

工具调用模式枚举。

#### 变体

| 变体 | 描述 |
|------|------|
| `Auto` | 自动决定是否调用工具 |
| `Required` | 强制要求调用工具 |
| `None` | 禁用工具调用 |
| `Specific { name: String, id: Option<String> }` | 指定特定工具 |

### LlmProvider Trait

LLM 提供者接口，用于创建 LLM 客户端实例。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `create_client()` | `model: &str` | ✅ | `Result<Box<dyn LlmClient>, AgentError>` | 创建客户端实例 |
| `create_client_with_headers()` | `model: &str`, `headers: Option<LlmHeaders>` | ❌ | `Result<Box<dyn LlmClient>, AgentError>` | 创建带自定义头的客户端 |
| `default_model()` | - | ✅ | `&str` | 获取默认模型名称 |
| `provider_name()` | - | ✅ | `&str` | 获取提供者名称 |

---

## 4. 工具系统 (Tools)

### ToolSource Trait

工具源接口，提供工具列表和调用功能。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `list_tools()` | - | ✅ | `Result<Vec<ToolSpec>, ToolSourceError>` | 列出所有可用工具 |
| `call_tool()` | `name: &str`, `arguments: &Value` | ✅ | `Result<Value, ToolSourceError>` | 调用指定工具 |
| `call_tool_with_context()` | `name: &str`, `arguments: &Value`, `context: ToolCallContext` | ❌ | `Result<Value, ToolSourceError>` | 带上下文调用工具 |

#### 示例用法

```rust
let tools = tool_source.list_tools().await?;
let result = tool_source.call_tool("get_weather", &json!({"city": "Beijing"})).await?;
```

### ToolSpec

工具规范描述。

#### 字段

| 字段名 | 类型 | 描述 |
|--------|------|------|
| `name` | `String` | 工具名称 |
| `description` | `Option<String>` | 工具描述 |
| `input_schema` | `Value` | 输入参数的 JSON Schema |
| `output_hint` | `Option<ToolOutputHint>` | 输出类型提示 |

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `with_output_hint()` | `output_hint: ToolOutputHint` | ✅ | `Self` | 设置输出提示 |

### ToolCall

工具调用结构。

#### 字段

| 字段名 | 类型 | 描述 |
|--------|------|------|
| `id` | `Option<String>` | 工具调用 ID |
| `name` | `String` | 工具名称 |
| `arguments` | `String` | 序列化的参数 JSON |

### ToolResult

工具执行结果。

#### 字段

| 字段名 | 类型 | 描述 |
|--------|------|------|
| `tool_call_id` | `String` | 对应的工具调用 ID |
| `content` | `String` | 执行结果内容 |
| `is_error` | `bool` | 是否为错误结果 |

---

## 5. 内存管理 (Memory)

### Checkpointer Trait

检查点接口，用于保存和恢复图执行状态。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `put()` | `config: &RunnableConfig`, `checkpoint: &Checkpoint<S>` | ✅ | `Result<String, CheckpointError>` | 保存检查点 |
| `get_tuple()` | `config: &RunnableConfig` | ✅ | `Result<Option<(Checkpoint<S>, CheckpointMetadata)>, CheckpointError>` | 获取检查点和元数据 |
| `list()` | `config: &RunnableConfig`, `limit: Option<usize>`, `before: Option<&str>`, `after: Option<&str>` | ✅ | `Result<Vec<CheckpointListItem>, CheckpointError>` | 列出检查点历史 |

#### 示例用法

```rust
let checkpoint_id = checkpointer.put(&config, &checkpoint).await?;
let (saved_checkpoint, metadata) = checkpointer.get_tuple(&config).await?.unwrap();
let history = checkpointer.list(&config, Some(10), None, None).await?;
```

### MemorySaver

内存检查点器，用于开发测试。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | - | ✅ | 创建新的内存检查点器 |

#### 示例用法

```rust
let checkpointer = Arc::new(MemorySaver::<ReActState>::new());
```

### SqliteSaver

SQLite 持久化检查点器，用于生产环境。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | `db_path: P`, `serializer: Arc<dyn Serializer<S>>` | ✅ | 创建 SQLite 检查点器 |

#### 示例用法

```rust
let serializer = Arc::new(JsonSerializer);
let checkpointer = Arc::new(SqliteSaver::<ReActState>::new(
    "./checkpoints.db",
    serializer
)?);
```

### Store Trait

键值存储接口，用于跨会话的长期记忆。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `put()` | `namespace: &Namespace`, `key: &str`, `value: &serde_json::Value` | ✅ | `Result<(), StoreError>` | 存储键值对 |
| `get()` | `namespace: &Namespace`, `key: &str` | ✅ | `Result<Option<serde_json::Value>, StoreError>` | 获取值 |
| `get_item()` | `namespace: &Namespace`, `key: &str` | ✅ | `Result<Option<Item>, StoreError>` | 获取完整项 |
| `delete()` | `namespace: &Namespace`, `key: &str` | ✅ | `Result<(), StoreError>` | 删除键值对 |
| `list()` | `namespace: &Namespace` | ✅ | `Result<Vec<String>, StoreError>` | 列出命名空间中的所有键 |
| `search()` | `namespace_prefix: &Namespace`, `options: SearchOptions` | ✅ | `Result<Vec<SearchItem>, StoreError>` | 搜索键值对 |
| `list_namespaces()` | `options: ListNamespacesOptions` | ✅ | `Result<Vec<Namespace>, StoreError>` | 列出命名空间 |
| `batch()` | `ops: Vec<StoreOp>` | ✅ | `Result<Vec<StoreOpResult>, StoreError>` | 批量操作 |

#### 示例用法

```rust
store.put(&namespace, "user_123", &json!({"name": "Alice"})).await?;
let user = store.get(&namespace, "user_123").await?;
let results = store.search(&namespace, SearchOptions::new().with_query("Alice")).await?;
```

### InMemoryStore

内存存储实现。

#### 构造方法

| 方法名 | 参数类型 | 必需 | 描述 |
|--------|----------|------|------|
| `new()` | - | ✅ | 创建新的内存存储 |

#### 示例用法

```rust
let store = Arc::new(InMemoryStore::new());
```

---

## 6. 流式传输 (Streaming)

### StreamEvent

流式事件枚举，包含各种执行事件类型。

#### 变体

| 变体 | 描述 |
|------|------|
| `Values(S)` | 完整状态快照 |
| `Updates { node_id, state, namespace }` | 增量状态更新 |
| `Messages { chunk, metadata }` | 消息块事件 |
| `Custom(Value)` | 自定义 JSON 载荷 |
| `Checkpoint(CheckpointEvent<S>)` | 检查点事件 |
| `TaskStart { node_id, namespace }` | 任务开始事件 |
| `TaskEnd { node_id, result, namespace }` | 任务结束事件 |
| `TotExpand { candidates }` | ToT 扩展事件 |
| `TotEvaluate { chosen, scores }` | ToT 评估事件 |
| `TotBacktrack { reason, to_depth }` | ToT 回溯事件 |
| `GotPlan { node_count, edge_count, node_ids }` | GoT 计划生成事件 |
| `GotNodeStart { node_id }` | GoT 节点开始事件 |
| `GotNodeComplete { node_id, result_summary }` | GoT 节点完成事件 |
| `GotNodeFailed { node_id, error }` | GoT 节点失败事件 |
| `GotExpand { node_id, nodes_added, edges_added }` | GoT 扩展事件 |
| `Usage(LlmUsage)` | Token 使用量事件 |
| `Error(String)` | 错误事件 |

### StreamMode

流式模式枚举，控制流式输出的类型。

#### 变体

| 变体 | 描述 |
|------|------|
| `Values` | 完整状态快照 |
| `Updates` | 增量更新 |
| `Messages` | 消息块（LLM 流式） |
| `Custom` | 自定义 JSON 载荷 |
| `Checkpoints` | 检查点事件 |
| `Tasks` | 任务开始/结束事件 |
| `Tools` | 工具生命周期事件 |
| `Debug` | 检查点 + 任务事件 |

#### 示例用法

```rust
let stream_modes = HashSet::from([StreamMode::Values, StreamMode::Messages]);
let stream = compiled.stream(state, config, stream_modes, ...);
```

### MessageChunk

消息块结构，用于流式传输 LLM 响应。

#### 字段

| 字段名 | 类型 | 描述 |
|--------|------|------|
| `kind` | `MessageChunkKind` | 消息块类型 |
| `content` | `String` | 消息内容 |

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `message()` | `content: String` | ✅ | `Self` | 创建普通消息块 |
| `thinking()` | `content: String` | ✅ | `Self` | 创建思考内容块 |
| `tool_call()` | `content: String`, `tool_call_id: String` | ✅ | `Self` | 创建工具调用块 |

#### 示例用法

```rust
let chunk = MessageChunk::message("Hello, world!".to_string());
let thinking = MessageChunk::thinking("Thinking...".to_string());
```

### StreamWriter

流式写入器，用于从节点内部发送自定义事件。

#### 方法

| 方法名 | 参数类型 | 必需 | 返回类型 | 描述 |
|--------|----------|------|----------|------|
| `from_context()` | `ctx: &RunContext<S>` | ✅ | `Self` | 从运行上下文创建写入器 |
| `emit_custom()` | `data: Value` | ✅ | `Result<(), AgentError>` | 发送自定义事件 |
| `emit_message()` | `content: &str`, `node_id: &str` | ✅ | `Result<(), AgentError>` | 发送消息事件 |
| `emit_thinking()` | `content: &str` | ✅ | `Result<(), AgentError>` | 发送思考事件 |

#### 示例用法

```rust
let writer = StreamWriter::from_context(&ctx);
writer.emit_custom(json!({"status": "processing"})).await?;
writer.emit_message("正在处理...", "my_node").await?;
```

---

## 7. 常量与辅助类型

### 常量

| 常量 | 类型 | 值 | 模块 |
|------|------|-----|------|
| `START` | `&str` | `"START"` | `loom::graph` |
| `END` | `&str` | `"END"` | `loom::graph` |

### 主要错误类型

| 类型 | 模块 | 描述 |
|------|------|------|
| `AgentError` | `loom::error` | 智能体执行错误 |
| `CompilationError` | `loom::graph` | 图编译错误 |
| `RunError` | `loom::agent::react` | 运行时错误 |
| `CheckpointError` | `loom::memory` | 检查点错误 |
| `StoreError` | `loom::memory` | 存储错误 |
| `ToolSourceError` | `loom::tool_source` | 工具源错误 |

---

## 8. 类型约束说明

大多数泛型类型 `S` 需要满足以下约束：

```rust
where
    S: Clone + Send + Sync + Debug + 'static
```

这确保状态可以：
- `Clone`: 被复制
- `Send + Sync`: 在线程间安全传递
- `Debug`: 支持 debug 输出
- `'static`: 生命周期足够长

---

**相关文档**: [核心概念](../core/state-graph.md) | [运行模式](../core/react.md) | [工具开发](../tools/tool-system.md) | [内存管理](../memory/checkpointer-store.md)