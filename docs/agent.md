# Agent System

This document describes the Loom Agent system: how agents are configured, resolved, and executed across CLI, WebSocket, and ACP entry points.

---

## 1. Overview

An **agent** in Loom is a configured persona that combines a role (instructions), a model, tools, and behavioral settings into a runnable unit. Agents execute one of four graph patterns — **ReAct**, **DUP**, **ToT**, or **GoT** — and stream structured events during execution.

The lifecycle is:

1. **Resolve profile** — Load agent configuration (YAML) from a named profile, a default profile, or the built-in `dev` agent.
2. **Build config** — Merge profile settings, CLI flags, environment variables, and MCP config into a `ReactBuildConfig`.
3. **Build runner** — Construct the LLM client, tools, and graph runner.
4. **Execute** — Run the graph (think → act → observe loop for ReAct) and stream events.

---

## 2. Agent Profiles

A profile is a YAML file (or Markdown with YAML front matter) that declares the agent's name, role, model, tools, behavior, and environment.

### 2.1 Profile Schema

```yaml
name: my-agent
description: "Short description of this agent."
version: "1.0"
extends: base                       # optional: inherit from another profile

role:
  file: instructions.md             # load role content from a file (relative to profile dir)
  content: "Inline role text."      # or provide content directly

model:
  name: gpt-4o
  temperature: 0.7
  max_tokens: 4096

tools:
  builtin:
    enabled: [bash, read, websearch]
    disabled: [web_fetcher]
  mcp:
    config: ./mcp.json
    servers:
      - name: filesystem
        command: npx
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        env: { API_KEY: "..." }
        enabled: true

behavior:
  approval_policy: destructive      # "destructive" or "always"
  max_iterations: 50
  timeout: 300

environment:
  working_folder: /path/to/project
  thread_id: thread-1
  user_id: user-1
```

All sections are optional. Unspecified fields fall back to defaults or environment variables.

### 2.2 Front Matter Format

Profiles can also be written as Markdown files with YAML front matter. The Markdown body becomes `role.content`:

```markdown
---
name: debugger
description: Debug specialist
model:
  name: gpt-4o
---
You are an expert debugger. Focus on root cause analysis.
Prefer reading logs and stack traces before suggesting fixes.
```

### 2.3 Profile Inheritance

A profile can extend another using `extends`:

```yaml
extends: base
name: reviewer
tools:
  builtin:
    disabled: [bash]
```

The base profile is loaded first, then the child merges on top. Simple values (name, description, model, role) are replaced by the child. `tools.builtin.disabled` lists are **combined** (union of base and child, deduplicated).

---

## 3. Profile Resolution

When Loom starts, it resolves the agent profile in this order:

### 3.1 Named Profile (`--agent NAME` / `-P NAME`)

1. **Built-in `dev`** — If name is `dev`, the compile-time embedded profile is used (no file lookup).
2. **Project** — `.loom/agents/<NAME>/config.yaml` (or `.yml`, `.md`, or `<NAME>.yaml`, `<NAME>.yml`, `<NAME>.md`).
3. **User** — `~/.loom/agents/<NAME>/` (same file patterns).

### 3.2 Default Profile (no `--agent`)

When no `--agent` flag is given, Loom searches for a default profile:

1. `.loom/agents/default/config.yaml` (or `.yml`, `.md`)
2. `.loom/agents/default.yaml` (or `.yml`, `.md`)
3. `agent.yaml` or `agent.yml` in the current directory
4. `~/.loom/agents/default/` (same file patterns)

If no profile is found, no profile-level settings are applied; role resolution still proceeds (see §4).

---

## 4. Role / Instructions Resolution

The agent's role (system persona / instructions) is resolved in priority order:

1. **Profile `role.content`** — From the loaded profile (inline content or referenced file).
2. **`--role FILE`** — CLI flag pointing to a role file.
3. **`instructions.md`** in working folder (or legacy `SOUL.md`).
4. **Built-in default** — The embedded `dev` agent instructions.

The resolved role is prepended to the system prompt. If an `AGENTS.md` file exists in the current directory or working folder, its content is also injected between the role and the base system prompt.

**System prompt assembly order**: `role_setting` → `AGENTS.md` → base ReAct prompt (with workdir rules and approval policy).

---

## 5. MCP Tool Configuration

MCP (Model Context Protocol) servers provide external tools to the agent. The MCP config path is resolved in order:

1. `--mcp-config PATH` CLI flag
2. Profile `tools.mcp.config`
3. `LOOM_MCP_CONFIG_PATH` environment variable
4. `.loom/mcp.json` in working folder
5. `~/.loom/mcp.json`

### 5.1 MCP Config Format

The config file uses a Cursor-compatible JSON format:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": { "API_KEY": "..." },
      "disabled": false
    },
    "remote-server": {
      "url": "https://mcp.example.com/sse",
      "headers": { "Authorization": "Bearer ..." }
    }
  }
}
```

Servers can be **stdio** (spawned as a child process via `command` + `args`) or **HTTP** (connected via `url`).

---

## 6. CLI Usage

The `loom` binary is the primary entry point.

### 6.1 Basic Usage

```bash
# Single-turn with a message
loom -m "explain this function"

# Positional message (no -m needed)
loom explain this function

# Specify working folder and agent
loom -m "fix the bug" -w ./my-project -P dev

# Interactive REPL
loom -i -w ./my-project

# REPL with an initial message
loom -i -m "let's review the auth module" -w ./my-project
```

### 6.2 CLI Flags

| Flag | Description |
|------|-------------|
| `-m, --message TEXT` | User message |
| `-w, --working-folder DIR` | Working folder for file tools (default: `/tmp`) |
| `--role FILE` | Override role/instructions file |
| `-P, --agent NAME` | Named agent profile |
| `--thread-id ID` | Thread ID for conversation continuity |
| `-v, --verbose` | Print state info to stderr |
| `-i, --interactive` | Interactive REPL mode |
| `--json` | Output as JSON (NDJSON stream events + reply) |
| `--file PATH` | Write JSON output to file instead of stdout |
| `--pretty` | Pretty-print JSON output |
| `--mcp-config PATH` | MCP config file path |

### 6.3 Subcommands

| Command | Description |
|---------|-------------|
| *(default)* / `react` | ReAct graph (think → act → observe) |
| `dup` | DUP graph (understand → plan → act → observe) |
| `tot` | ToT graph (think_expand → think_evaluate → act → observe) |
| `got [--got-adaptive]` | GoT graph (plan_graph → execute_graph); `--got-adaptive` enables AGoT |
| `tool list` | List all loaded tools |
| `tool show NAME [--output yaml\|json]` | Show a tool's definition |
| `serve [--addr ADDR]` | Start WebSocket server (default `127.0.0.1:8080`) |

### 6.4 REPL Mode

With `-i` / `--interactive`, Loom enters a read-eval-print loop. A thread ID is auto-generated if not provided, enabling conversation continuity across turns. Exit with `quit`, `exit`, `/quit`, or an empty line.

---

## 7. Execution Modes

### 7.1 CLI (Local)

The default mode. Parses args into `RunOptions`, builds the runner, executes the graph, and prints the reply to stdout.

### 7.2 WebSocket Server (`loom serve`)

Starts a WebSocket server. Clients send `RunRequest` messages; the server builds `RunOptions`, executes the agent, and streams protocol events back over the WebSocket. See [protocol_spec.md](./protocol_spec.md) for the event format.

### 7.3 ACP (Agent Client Protocol)

`LoomAcpAgent` implements the ACP `Agent` trait, mapping incoming prompts to `run_agent_with_options`. This enables Loom agents to be used as ACP-compatible services.

### 7.4 GitHub Webhook

`run_options_from_issues_event` builds `RunOptions` from a GitHub `IssuesEvent`, allowing agents to be triggered by issue creation or updates.

---

## 8. Built-in `dev` Agent

The `dev` agent is embedded at compile time from `loom/agents/dev/` (config.yaml + instructions.md). It serves as both the default agent profile and the fallback instructions when no other role is found.

Select it explicitly with `--agent dev` or `-P dev`. Its instructions cover:

- Editing constraints (ASCII default, minimal comments)
- Tool usage preferences (specialized tools over shell)
- Git and workspace hygiene (never revert unrelated changes)
- Frontend design guidelines
- Output formatting and tone

---

## 9. Creating a Custom Agent

### 9.1 Project-Level Agent

Create a profile in your project's `.loom/agents/` directory:

```
.loom/
  agents/
    reviewer/
      config.yaml
      instructions.md
```

`config.yaml`:
```yaml
name: reviewer
description: "Code review specialist"
role:
  file: instructions.md
model:
  name: gpt-4o
tools:
  builtin:
    disabled: [bash]
```

`instructions.md`:
```markdown
You are a code review expert. Focus on correctness, readability,
and adherence to project conventions. Never modify files directly.
```

Run with:
```bash
loom -P reviewer -m "review the latest PR changes" -w .
```

### 9.2 User-Level Agent

Place profiles in `~/.loom/agents/` to make them available across all projects:

```
~/.loom/
  agents/
    writer/
      config.yaml
      instructions.md
```

### 9.3 Default Agent

Name your profile `default` (or place `agent.yaml` in the project root) and it will be used automatically when no `--agent` flag is given.

---

## 10. Environment Variables

| Variable | Description |
|----------|-------------|
| `HELVE_MAX_MESSAGE_LEN` | Max input message length (default: 200 chars) |
| `HELVE_MAX_REPLY_LEN` | Max reply length; 0 = no truncation (default: 0) |
| `LOOM_MCP_CONFIG_PATH` | MCP config file path |
| `PROMPTS_DIR` | Override directory for prompt templates |
| `REACT_SYSTEM_PROMPT` | Override the ReAct base system prompt |

---

## 11. Configuration Hierarchy

Settings are resolved with the following precedence (highest to lowest):

1. **CLI flags** (`--agent`, `--role`, `--mcp-config`, `-w`, `--thread-id`)
2. **Agent profile** (model, MCP config, environment, role)
3. **Environment variables** (`LOOM_MCP_CONFIG_PATH`, `REACT_SYSTEM_PROMPT`)
4. **File discovery** (`instructions.md`, `AGENTS.md`, `.loom/mcp.json`)
5. **Built-in defaults** (dev agent instructions, `/tmp` working folder)

---

## 12. Prompt Templates

Default prompt templates for each graph pattern are embedded from `loom/prompts/`:

| File | Content |
|------|---------|
| `react.yaml` | ReAct system prompt and error templates |
| `tot.yaml` | ToT expand and evaluate addons |
| `got.yaml` | GoT plan_system and AGoT expand_system |
| `dup.yaml` | DUP understand_prompt |
| `helve.yaml` | Workdir template, approval_destructive, approval_always |

Override at runtime by copying the `prompts/` directory into your project or setting `PROMPTS_DIR`.
