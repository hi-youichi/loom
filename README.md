# GraphWeave

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

# Run GraphWeave CLI
cargo run -p graphweave-cli -- -m "What time is it?"
cargo run -p graphweave-cli -- --working-folder . "Summarize this repo"
```

## Workspace

| Crate | Description |
|-------|-------------|
| `graphweave` | Core library: graph, nodes, state, LLM, tools, memory |
| `graphweave-cli` | CLI binary with React / Dup / Tot / Got / Tool subcommands |
| `graphweave-examples` | Examples |

## Library usage

```rust
use graphweave::{Agent, StateGraph, Message, ReActState};
// See graphweave crate docs
```

## License

MIT
