# helve-cli

CLI for Helve: run the ReAct agent from the command line.

## Usage

```bash
# From workspace root
cargo run -p helve-cli -- -m "What time is it?"
cargo run -p helve-cli -- --working-folder . "Summarize this repo"
```

## Options

- `-m, --message <TEXT>` — User message (or pass as positional args).
- `-w, --working-folder <DIR>` — Working directory for tools; default: current dir.
- `--thread-id <ID>` — Thread ID for conversation continuity (checkpointer).
- `-v, --verbose` — Log node enter/exit and graph execution.

## Config

Config is loaded from env (and optional `.env`). See `langgraph` Helve config for shared semantics (API base, API key, etc.).
