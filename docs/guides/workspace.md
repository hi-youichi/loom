# Workspace & Thread Management

Workspace and thread management in Loom tie runs to a **workspace** (e.g. project directory) and **thread** (conversation). This document covers workspace association, thread lifecycle, storage, and metadata.

## Workspace association

- **Working folder** (e.g. **HelveConfig::working_folder**, **RunOptions::working_folder**) identifies the workspace. It is used for:
  - File tools: scope read/write to this path.
  - Profile resolution: AGENTS.md, instructions.md can be read from the working folder.
  - System prompt assembly: workdir rules and approval hints when **assemble_system_prompt** is used with the workdir.
- The CLI and serve layer pass workspace (working folder) in run requests; the runner receives it via **ReactBuildConfig** / **HelveConfig**.

## Thread lifecycle

- **thread_id** (in **RunnableConfig**) identifies a conversation/thread. It is required for checkpoint put/get and is often used to scope user messages and tool state.
- Threads are created implicitly: the first run with a given thread_id starts a new thread; subsequent runs with the same thread_id resume or extend it (using checkpointer and optional **UserMessageStore**).
- There is no explicit “close thread” in the core API; persistence is determined by the checkpointer and store (e.g. SQLite retains data until deleted or pruned by the app).

## Separate storage per workspace

- Checkpointer and store can be configured to use paths or namespaces that include workspace identity (e.g. one SQLite file per workspace, or namespace `[workspace_id, user_id]`). The exact layout is application-defined; Loom provides **thread_id**, **user_id**, and **checkpoint_ns** in **RunnableConfig** for this.
- **SqliteSaver** and **SqliteStore** take a path; the serve layer or CLI can set the path based on workspace (e.g. `workspace_root / ".loom" / "memory.db"`).

## Metadata management

- **CheckpointMetadata**, **CheckpointListItem**: Checkpointer list/get return metadata (e.g. step, timestamp) for time-travel or UI.
- **ThreadMetadata** (if used in your layer): Application-specific thread info (title, created_at, etc.) can be stored in a separate table or store keyed by thread_id.
- **UserMessageStore**: Per-thread message history (append/list) for displaying or editing conversation before/after runs; see **user_message** module.

## Summary

| Topic | Notes |
|-------|--------|
| Workspace | working_folder for file tools, profile, system prompt |
| Thread | thread_id in RunnableConfig; checkpoint and messages scoped by thread |
| Storage | Path/namespace per workspace when desired; thread_id/user_id in config |
| Metadata | CheckpointMetadata; optional ThreadMetadata and UserMessageStore |

Next: [ACP](acp.md) for Agent Client Protocol and IDE integration.
