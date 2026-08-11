# Loom CLI Guide

## Starting a Task

Run from your project root:

```powershell
loom -m "Summarize the test structure of this repo"
loom --working-folder . "Fix failing tests and report verification results"
```

Without `--working-folder`, Loom uses the current directory. Always confirm that this directory is the project boundary you want the agent to access.

You can choose a run mode: `react` (default), `dup`, `tot`, `got`. For example:

```powershell
loom dup -m "Plan and execute this refactoring"
loom --tier strong -m "Analyze this complex failure"
loom --model openai/gpt-4o-mini -m "Explain this code"
```

Conflicts between `--model`, `--provider`, and `--tier` will error before execution. `--effort` passes reasoning intensity supported by the model; unknown values may be rejected by the provider.

## Sessions and Interaction

```powershell
loom -i -m "Analyze this issue"                # Interactive conversation in this process
loom --session-id issue-42 "Continue analysis" # Resume across calls
loom session list
loom session show issue-42
loom session rename issue-42 "Login bug"
loom session delete issue-42
```

Deleting a session removes its conversation data; it does not delete project files, memory, or skills. Use `session show` or `session cat` to confirm the target first.

## Tools, MCP, and Agent Profiles

```powershell
loom tool list
loom tool show <name>
loom mcp list
loom models
loom agent list
loom --agent coding -m "Implement this feature"
```

MCP config can be specified via `--mcp-config <path>`; otherwise Loom reads it from config and project discovery rules. Before enabling third-party MCP, review its commands, network access, and credential requirements.

## Context Management

```powershell
loom memory list
loom skills list
loom skill-usage show
loom review pending
loom curator status
```

Memory and skills may affect subsequent tasks. Before creating, editing, or accepting review suggestions, confirm the scope and source.

## Output, Logging, and Debugging

```powershell
loom --json -m "task"                     # Machine-readable events and results
loom --json --pretty --file result.json -m "task"
loom -v -m "task"                          # More verbose runtime output
loom --log-file .loom/logs/loom.log -m "task"
```

The stdout from `--json` is intended for programmatic consumption only; write logs to files or stderr. Do not parse task status from plain-text output in scripts.

## Modification Tasks and Cancellation

```powershell
loom --worktree -m "Upgrade dependencies and run tests"
```

`--worktree` runs in an isolated Git worktree; it auto-cleans when there are no changes, and preserves the working directory for review when there are. The first Ctrl+C during a run requests graceful cancellation; a second Ctrl+C within a short window force-quits. Cancellation is not rollback: check Git status and the final output.

## Security Notes

File, shell, browser, and MCP tools can all affect local or remote resources. Do not put secrets in prompts, memory, skills, or logs; review out-of-project paths, install commands, delete commands, and external services. See [Security & Privacy](security-and-privacy.md).
