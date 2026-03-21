# CLI Tool

The **loom** CLI runs agents locally or in remote mode. This document covers command-line design, config loading, profile management, and execution modes.

## Command-line interface design

The binary is produced by the **loom-cli** crate. Main entry points:

- **run** (default): Run an agent with a user message (local or remote by option).
- **serve**: Start the WebSocket server for remote execution (see [Serve](serve.md)).
- **profiles** / **list**: List available agent profiles.
- Other subcommands as defined in the CLI (e.g. config, health).

Arguments typically include:

- **Profile**: Agent to run (e.g. by name from AGENTS.md or profile config).
- **Message**: User message for the run.
- **Working folder**: Directory for AGENTS.md, instructions.md, and profile resolution.
- **Remote**: Use WebSocket to connect to a running **loom serve** instead of running locally.
- **Streaming / verbose**: Control output (e.g. stream events, logs).

## Config loading and profile management

- **Profiles** are resolved from **AGENTS.md** and/or profile configuration (e.g. YAML or env). **list_available_profiles** / **resolve_profile** return **ProfileSummary** / **AgentProfile**.
- **ReactBuildConfig** (and **HelveConfig**) are built from the resolved profile and env: model, API keys, thread_id, user_id, db_path, tools, approval policy, etc.
- **load_agents_md(working_folder)** and **load_soul_md(working_folder)** read AGENTS.md and instructions.md (or SOUL.md) from the current directory and optionally the working folder; used for profile metadata and system prompt.

**build_config_from_profile** / **build_helve_config** convert profile + options into **ReactBuildConfig** and **HelveConfig** for the runner.

## Argument parsing

CLI uses standard argument parsing (e.g. clap). **RunOptions** (or equivalent) hold profile name, message, working folder, remote flag, stream/verbose, and overrides (e.g. model, thread_id). **RunCmd** represents the chosen subcommand and options.

## Execution modes: local vs remote

- **Local**: Build **ReactRunner** (or other runner) via **build_react_runner** / **build_react_run_context**, then **run_agent_with_options** (or **runner.invoke** / **stream_with_config**). Output is printed (or streamed) in the terminal.
- **Remote**: When **--remote** (or equivalent) is set, the CLI connects to a running **loom serve** over WebSocket, sends **RunRequest** with message and config, and consumes **RunStreamEventResponse** and **RunEndResponse** (see [Serve](serve.md) and [Streaming](streaming.md)).

**run_agent** / **run_agent_with_options** abstract over local vs remote: they take **RunOptions** and either run the agent in-process or delegate to the remote client.

## Streaming output

When streaming is enabled, the CLI receives **StreamEvent** (or protocol equivalents) and can:

- Print message chunks as they arrive.
- Show task start/end or tool progress.
- Emit SSE or other format for downstream consumers.

**AnyStreamEvent** (or similar) type-erases **StreamEvent&lt;ReActState&gt;** for CLI handling. Callbacks passed to **run_agent_with_options** or **runner.stream_with_config** process events (e.g. print, forward to WebSocket).

## Summary

| Topic | Notes |
|-------|--------|
| Commands | run, serve, profiles/list; profile + message + working folder |
| Config | ReactBuildConfig, HelveConfig from profile + env; build_config_from_profile |
| Profiles | resolve_profile, list_available_profiles; AGENTS.md, instructions.md |
| Modes | Local (build_react_runner, run_agent_with_options) vs remote (WebSocket) |
| Streaming | StreamEvent handling; AnyStreamEvent; callbacks |

Next: [Serve Module](serve.md) for WebSocket server and remote execution.
