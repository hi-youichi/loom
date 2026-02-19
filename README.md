# Loom

A graph-based agent framework in Rust with a **state-in, state-out** design: a single state type flows through the graph, with no separate Input/Output types.

## Features

- **State graphs**: StateGraph, conditional edges, middleware, checkpointing
- **ReAct / DUP / ToT / GoT**: Multiple run modes
- **LLM integration**: `LlmClient` trait, OpenAI / Mock support
- **Tool system**: Pluggable ToolSource (MCP, Web, Bash, Store, etc.)
- **Memory & persistence**: Checkpointer, Store (SQLite, optional LanceDB)

## Streaming output (JSON)

With `--json`, the CLI emits [NDJSON](https://ndjson.org/) per [docs/protocol_spec.md](docs/protocol_spec.md): one JSON object per line (events with `type` + payload, then a final line with `reply`). Optional envelope fields `session_id`, `node_id`, `event_id` are included for merging multi-turn or multi-session streams.

```bash
cargo run -p loom-cli -- -m "Hello" --json
# or stream to file: --json --file out.json
```

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
