# Loom

Loom is a local-first AI Agent runtime. It lets developers run agents in the CLI, IDE (ACP), and messaging bots, while keeping tool calls, sessions, memory, skills, and workflows within a controllable project context.

Loom's goal is not to replace code review or let agents modify systems unattended, but to enable them to complete real project tasks continuously and interpretably.

> The current version is still evolving. Workflows, browser extension, and task modes include experimental capabilities; `evolve` is not yet implemented.

## Quick Start

### Install a Release Binary

On Linux or macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/hi-youichi/loom/main/scripts/install.sh | sh
```

On Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/hi-youichi/loom/main/scripts/install.ps1 | iex
```

The installers use a user-level directory and do not require administrator privileges. Set `LOOM_VERSION`, `LOOM_REPO`, or `LOOM_INSTALL_DIR` to override the release, repository, or destination.

### 1. Configure Your Model

Copy the example environment file and fill in your model credentials:

```powershell
Copy-Item .env.example .env
```

You can also create a `config.toml` in the user config directory (default `~/.loom/`, overridable via `LOOM_HOME`). The `.env` in the project root takes precedence over that config.

### 2. Run an Agent in Your Project

```powershell
# Run the default ReAct agent
cargo run -p cli -- -m "Survey this repo and list test entry points"

# Explicitly specify the agent's working directory
cargo run -p cli -- --working-folder . "Find failing tests and explain why"

# Continue in the same session
cargo run -p cli -- --session-id bug-123 "Now fix it and run the relevant tests"
```

Before the first run, verify the agent's effective working directory, model, and tool permissions. For modification tasks, use `--worktree` to run in an isolated Git worktree.

## What Loom Can Do

| Capability | Use Case |
| --- | --- |
| Local Agents | Complete multi-step tasks using ReAct, DUP, ToT, or GoT. |
| Models & Tools | Configure multiple providers, model tiers, MCP, file, shell, web, and more. |
| Persistent Context | Continue project work across sessions with checkpoints, memory, and skills. |
| Workflows | Orchestrate multi-agent tasks in Lua; inspect instance summaries, events, cancel and resume. |
| Multiple Entry Points | Use from CLI, or connect via ACP to compatible IDEs; also supports Telegram multi-bot. |

## Common Commands

```text
loom -m "task"                         # Start a one-shot task
loom -i -m "task"                      # Enter an interactive session
loom --session-id <id> "continue task" # Resume a session
loom session list                       # List sessions
loom models                             # List available models
loom tool list                          # List tools
loom mcp list                           # Manage MCP services
loom skills list / loom memory list     # Manage reusable context
loom acp                                # Start as an ACP server
```

For full usage, see the [CLI Guide](docs/guides/cli.md).

## Documentation

- [CLI Guide](docs/guides/cli.md)
- [IDE / ACP Integration](docs/guides/acp-ide.md)
- [Workflow Guide](docs/guides/workflows.md)
- [Security & Privacy](docs/guides/security-and-privacy.md)
- [Troubleshooting](docs/guides/troubleshooting.md)

## Development

```powershell
cargo build -p cli
cargo test -p cli
```

Loom is a Rust workspace; crate and experimental module details can be found in each module's `Cargo.toml`, source code, and `docs/design/`. User experience and scope are defined by the guides under `docs/`.

## License

MIT
