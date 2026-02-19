# Loom

A graph-based agent framework in Rust with a **state-in, state-out** design: a single state type flows through the graph, with no separate Input/Output types.

## Features

- **State graphs**: StateGraph, conditional edges, middleware, checkpointing
- **ReAct / DUP / ToT / GoT**: Multiple run modes
- **LLM integration**: `LlmClient` trait, OpenAI / Mock support
- **Tool system**: Pluggable ToolSource (MCP, Web, Bash, Store, etc.)
- **Memory & persistence**: Checkpointer, Store (SQLite, optional LanceDB)

## Quick start

```bash
# Set up .env (see .env.example; requires OPENAI_API_KEY)
cp .env.example .env

# Run Loom CLI
cargo run -p loom-cli -- -m "What time is it?"
cargo run -p loom-cli -- --working-folder . "Summarize this repo"
```

## Workspace

| Crate | Description |
|-------|-------------|
| `loom` | Core library: graph, nodes, state, LLM, tools, memory |
| `loom-cli` | CLI binary with React / Dup / Tot / Got / Tool subcommands |
| `loom-examples` | Examples |

## Library usage

```rust
use loom::{Agent, StateGraph, Message, ReActState};
// See loom crate docs
```

## License

MIT
