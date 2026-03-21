# Loom Overview

## Project Overview

Loom is a Rust-based agent framework with a **state-in, state-out** design: a single state type flows through the graph, with no separate Input/Output types. You build directed graphs of nodes, each receiving and returning the same state; the framework handles routing, checkpointing, and streaming.

## Philosophy

- **Single state type**: Each graph uses one state struct (e.g. `ReActState`) that all nodes read from and write to.
- **One step per run**: Each node implements a single step—receive state, return updated state and routing (`Next`).
- **State graphs**: Compose nodes into `StateGraph` with conditional edges for complex workflows.
- **Minimal core API**: `CompiledStateGraph::invoke` stays state-in/state-out; use `CompiledStateGraph::stream` when you need incremental output.

## Key Benefits

- **Composability**: Build agents as nodes and wire them in graphs with conditional routing.
- **Persistence**: Optional checkpointing (in-memory or SQLite) and long-term memory (Store, optional LanceDB).
- **Tool integration**: Extensible tool system with MCP, web, bash, memory, and store tools.
- **Streaming**: Per-step state updates, task events, checkpoints, and custom events via `StreamMode`.
- **Testing**: `MockLlm`, `MockToolSource`, and in-memory stores make tests straightforward.

## Use Cases

- **ReAct agents**: Think → Act → Observe loops with LLM and tools (see [ReAct Pattern](architecture/react-pattern.md)).
- **Conditional workflows**: Route by state (e.g. "need tools" vs "done") via conditional edges.
- **Human-in-the-loop**: Interrupt handlers and approval policies for sensitive actions.
- **Multi-step pipelines**: Linear or branching graphs with checkpointing and resume.
- **Remote execution**: WebSocket server and protocol for running agents from IDEs or other clients.

## Getting Started

### Prerequisites

- Rust toolchain
- For OpenAI-style LLMs: API key and compatible endpoint
- Optional: SQLite for persistent checkpoints; LanceDB (feature `lance`) for vector memory

### Build and install CLI

```bash
cargo build --release
cp target/release/loom ~/.local/bin/
```

Ensure `~/.local/bin` is on your `PATH`.

### Minimal example (echo agent)

```rust
use async_trait::async_trait;
use loom::{Agent, AgentError, Message};

#[derive(Clone, Debug, Default)]
struct MyState {
    messages: Vec<Message>,
}

struct EchoAgent;

#[async_trait]
impl Agent for EchoAgent {
    fn name(&self) -> &str {
        "echo"
    }
    type State = MyState;

    async fn run(&self, state: Self::State) -> Result<Self::State, AgentError> {
        let mut messages = state.messages;
        if let Some(Message::User(s)) = messages.last() {
            messages.push(Message::Assistant(s.clone()));
        }
        Ok(MyState { messages })
    }
}
```

Run the echo example:

```bash
cargo run -p loom-examples --example echo -- "hello, world!"
```

### Next steps

- [Core Concepts](architecture/core-concepts.md) — State, graph structure, execution, tools, checkpointing
- [State Graphs](architecture/state-graphs.md) — Building graphs, conditional edges, middleware
- [ReAct Pattern](architecture/react-pattern.md) — Think/Act/Observe and `ReactRunner`

## Documentation map

| Topic | Document |
|-------|----------|
| Introduction | This document |
| Core concepts | [architecture/core-concepts.md](architecture/core-concepts.md) |
| State graphs | [architecture/state-graphs.md](architecture/state-graphs.md) |
| ReAct pattern | [architecture/react-pattern.md](architecture/react-pattern.md) |
| LLM integration | [guides/llm-integration.md](guides/llm-integration.md) |
| Tool system | [architecture/tool-system.md](architecture/tool-system.md) |
| Memory & checkpointing | [architecture/memory-checkpointing.md](architecture/memory-checkpointing.md) |
| Streaming | [guides/streaming.md](guides/streaming.md) |
| CLI | [guides/cli.md](guides/cli.md) |
| Serve (WebSocket) | [guides/serve.md](guides/serve.md) |
| Advanced patterns | [architecture/advanced-patterns.md](architecture/advanced-patterns.md) |
| Workspace | [guides/workspace.md](guides/workspace.md) |
| ACP | [guides/acp.md](guides/acp.md) |
| Compression | [architecture/compression.md](architecture/compression.md) |
| Configuration | [guides/configuration.md](guides/configuration.md) |
| Visualization | [guides/visualization.md](guides/visualization.md) |
| Testing | [guides/testing.md](guides/testing.md) |
