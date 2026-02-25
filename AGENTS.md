# AGENTS.md

## Cursor Cloud specific instructions

### Overview

Loom is a graph-based AI agent framework in Rust. The Cargo workspace contains these crates:

| Crate | Description |
|---|---|
| `loom` | Core library (graph, nodes, state, LLM, tools, memory) |
| `cli` | CLI binary (`loom`) |
| `serve` | WebSocket server |
| `config` | Env/config loading |
| `stream-event` | Protocol types |
| `loom-examples` | Examples |

### Prerequisites

- **Rust stable ≥ 1.85** is required (the `mcp_client`/`mcp_core` git dependencies use edition 2024 / resolver 3). Run `rustup default stable && rustup update stable` if needed.
- **libssl-dev** must be installed (`sudo apt-get install -y libssl-dev`).
- No `Cargo.lock` is committed; first build resolves and downloads all dependencies.
- SQLite is bundled via `rusqlite` (no system install needed).

### Build / Test / Lint / Run

Standard commands (see also `README.md`):

- **Build:** `cargo build`
- **Test:** `cargo test` — most tests run without external APIs. Tests requiring `OPENAI_API_KEY` are `#[ignore]`d.
- **Lint:** `cargo clippy --lib --bins` (warnings only, no errors). Using `--all-targets` triggers a pre-existing clippy error in test code.
- **Format check:** `cargo fmt --check` (pre-existing formatting diffs exist).
- **Run CLI:** `cargo run -p cli -- --help`
- **Run examples (no API key):** `cargo run -p loom-examples --example echo -- "Hello"` or `--example state_graph_echo`
- **Run agent (requires API key):** `cargo run -p cli -- -m "Hello"` (needs `OPENAI_API_KEY` in `.env`)
- **Run WebSocket server:** `cargo run -p cli -- serve`

### Known issues

- `cli_tool_show_existing_local_json_succeeds` test fails (pre-existing) — it references a `get_recent_messages` tool that is no longer in the default local tool list. Skip with `--skip cli_tool_show_existing_local_json_succeeds`.

### Environment variables

Copy `.env.example` to `.env`. `OPENAI_API_KEY` is required for LLM-backed agent runs. Without it, tool listing, help, and non-LLM examples still work.
