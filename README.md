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
cargo run -p cli -- -m "Hello" --json
# or stream to file: --json --file out.json
```

## Quick start

```bash
# Set up .env (see .env.example; requires OPENAI_API_KEY)
cp .env.example .env

# Run Loom CLI
cargo run -p cli -- -m "What time is it?"
cargo run -p cli -- --working-folder . "Summarize this repo"
```

## Workspace

| Crate | Description |
|-------|-------------|
| `loom` | Core library: graph, nodes, state, LLM, tools, memory |
| `cli` | CLI binary with React / Dup / Tot / Got / Tool subcommands |
| `loom-examples` | Examples |

## Library usage

Run an agent with a user message. Pass `None` for options to use mock LLM and tool source (good for demos); pass `Some(AgentOptions { ... })` to supply your own `llm`, `tool_source`, and optional `checkpointer`, `store`, `runnable_config`, or `verbose`. Add `dotenv` to `Cargo.toml` if loading `.env`.

```rust
use loom::{run_agent, AgentOptions, ChatOpenAI, MockToolSource};

// Minimal: mock LLM and get_time tool (no API key)
let state = run_agent("What time is it?", None).await?;
println!("{}", state.last_assistant_reply().unwrap_or_default());

// OpenAI with get_time tool — API key from env or .env
dotenv::dotenv().ok();  // load .env (OPENAI_API_KEY=sk-...)
let tool_source = MockToolSource::get_time_example();
let tools = tool_source.list_tools().await?;
// Option A: key from OPENAI_API_KEY env (set in .env or shell)
let llm = ChatOpenAI::new("gpt-4o-mini").with_tools(tools);
// Option B: key set programmatically (add async-openai dep)
// let config = async_openai::config::OpenAIConfig::new()
//     .with_api_key(std::env::var("OPENAI_API_KEY")?);
// let llm = ChatOpenAI::with_config(config, "gpt-4o-mini").with_tools(tools);
let state = run_agent(
    "What time is it?",
    Some(AgentOptions {
        llm: Some(Box::new(llm)),
        tool_source: Some(Box::new(tool_source)),
        ..Default::default()
    }),
).await?;
println!("{}", state.last_assistant_reply().unwrap_or_default());
```

## License

MIT
