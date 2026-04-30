# 5 分钟跑通你的第一个 ReAct Agent

## 前置要求

- Rust 1.70+ 
- OpenAI API Key（或其他兼容的 LLM API）

## 步骤

### 1. 创建项目并添加依赖

```bash
cargo new my_agent
cd my_agent
```

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
loom = "0.1.6"
tokio = { version = "1.0", features = ["full"] }
config = "0.1"  # Loom 统一配置管理
```

### 2. 配置 API Key

Loom 使用 `config` crate 统一管理配置，支持 `~/.loom/config.toml` 和项目 `.env` 两种方式。

**方式一：全局配置（推荐）**

创建 `~/.loom/config.toml`：

```toml
[env]
OPENAI_API_KEY = "your_api_key_here"
OPENAI_BASE_URL = "https://api.openai.com/v1"
```

**方式二：项目 .env**

在项目根目录创建 `.env` 文件：

```env
OPENAI_API_KEY=your_api_key_here
```

### 3. 编写 Agent 代码

在 `src/main.rs` 中：

```rust
use std::sync::Arc;
use loom::{
    ActNode, ChatOpenAICompat, CompiledStateGraph, FixedLlmProvider, Message, 
    ObserveNode, ReActState, StateGraph, ThinkNode, END, START
};

#[tokio::main]
async fn main() {
    // 加载 ~/.loom/config.toml 和项目 .env 中的环境变量
    config::load_and_apply("loom", None).expect("Failed to load config");
    
    let llm_client = Arc::new(ChatOpenAICompat::new("gpt-4")
        .expect("Failed to create LLM client"));
    
    let provider = Arc::new(FixedLlmProvider {
        client: llm_client,
        model_id: "gpt-4".to_string(),
    });
    
    let bash_tools = loom::BashToolsSource::new().await;
    
    let mut graph = StateGraph::<ReActState>::new();
    graph
        .add_node("think", Arc::new(ThinkNode::new(provider)))
        .add_node("act", Arc::new(ActNode::new(Box::new(bash_tools))))
        .add_node("observe", Arc::new(ObserveNode::new()))
        .add_edge(START, "think")
        .add_edge("think", "act")
        .add_edge("act", "observe")
        .add_edge("observe", END);

    let compiled: CompiledStateGraph<ReActState> = graph.compile()
        .expect("Failed to compile graph");

    let state = ReActState {
        model_config: loom::ModelConfig::default(),
        messages: vec![
            Message::user("列出当前目录的文件")
        ],
        tool_calls: vec![],
        tool_results: vec![],
        turn_count: 0,
        approval_result: None,
        usage: None,
        total_usage: None,
        message_count_after_last_think: None,
        last_reasoning_content: None,
        think_count: 0,
        summary: None,
        should_continue: true,
    };

    match compiled.invoke(state, None).await {
        Ok(result) => {
            if let Some(msg) = result.messages.last() {
                println!("Agent 回复: {}", msg);
            }
        }
        Err(e) => eprintln!("错误: {}", e),
    }
}
```

### 4. 运行并验证

```bash
cargo run
```

你应该看到 Agent 执行思考、调用工具、观察结果的完整过程。

## 下一步

- 了解 [StateGraph](../core/state-graph.md) 高级用法
- 探索 [ReAct 模式](../core/react.md) 更多配置选项
- 查看 [LLM 客户端](../core/llm-client.md) 支持的模型提供商
- 添加其他工具如 `WebToolsSource` 或 `MemoryToolsSource`