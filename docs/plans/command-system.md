# Loom Command System

## Overview

Add a unified slash command system in `loom/src/command/`. Commands are intercepted **outside the graph** before invoke/stream. All three entry points (CLI REPL, Telegram Bot, ACP Server) share the same parse + execute logic.

Reference: Cursor (`/clear` aliases `/reset`, `/new`), Claude Code (`/clear` aliases `/reset`, `/new`, `/compact`).

## Command List

| Command | Aliases | Description |
|---|---|---|
| `/reset` | `/clear`, `/new` | Clear messages (keep system prompt), reset runtime fields |
| `/compact` | `/compact [instructions]` | Compress conversation context with optional focus instructions |
| `/summarize` | `/summarize` | Summarize conversation so far into a summary field |
| `/models` | `/models`, `/models <query>`, `/models use <id>` | List, search, or switch LLM model |


## Architecture

```
User input → parse(text) → Command?
  ├── Yes: execute command → return result (no graph entry)
  └── No:  normal invoke/stream graph
```

## File Structure

```
loom/src/command/
├── mod.rs        # pub mod + re-exports
├── parser.rs     # parse(text) -> Option<Command>
├── command.rs    # enum Command + CommandResult
└── builtins.rs   # execute() for each built-in command
```

## Core Types

```rust
// command.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ResetContext,
    Compact { instructions: Option<String> },
    Summarize,
    Models { query: Option<String> },
    ModelsUse { model_id: String },
}

/// Result of executing a command.
pub enum CommandResult {
    /// Command produced a text reply. Do not enter graph.
    Reply(String),
    /// Let the message pass through to the graph normally.
    PassThrough,
}
```

```rust
// parser.rs

/// Parse slash command from user input text.
/// Returns None if text is not a recognized command.
pub fn parse(text: &str) -> Option<Command> {
    let trimmed = text.trim();
    let token = trimmed.split_whitespace().next()?;
    match token {
        "/reset" | "/clear" | "/new" => Some(Command::ResetContext),
        "/compact" => {
            let instructions = trimmed.strip_prefix("/compact")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            Some(Command::Compact { instructions })
        }
        "/summarize" => Some(Command::Summarize),
        "/models" => {
            let rest = trimmed.strip_prefix("/models")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            match rest {
                Some(q) if q.starts_with("use ") => {
                    Some(Command::ModelsUse { model_id: q[4..].trim().to_string() })
                }
                Some(q) => Some(Command::Models { query: Some(q.to_string()) }),
                None => Some(Command::Models { query: None }),
            }
        }
        _ => None,
    }
}
```

```rust
// builtins.rs

/// Execute a parsed command.
///
/// Some commands mutate state and return a reply (ResetContext, Compact, Summarize).
/// Others are side-effects that need external context (Models) and return PassThrough
/// after setting flags on state.
pub fn execute<S: ResetState + CompactState + SummarizeState>(
    cmd: Command,
    state: &mut S,
) -> CommandResult {
    match cmd {
        Command::ResetContext => {
            state.reset_context();
            CommandResult::Reply("Context cleared.".into())
        }
        Command::Compact { instructions } => {
            state.compact_context(instructions);
            CommandResult::Reply("Context compacted.".into())
        }
        Command::Summarize => {
            state.summarize();
            CommandResult::Reply("Context summarized.".into())
        }
        Command::Models { .. } | Command::ModelsUse { .. } => {
            // Models command requires external context (model registry, provider).
            // Handled at the integration layer, not here.
            CommandResult::PassThrough
        }
    }
}
```

## State Traits

All traits defined in `loom/src/command/builtins.rs`, implemented by each state type.

### ResetState

```rust
pub trait ResetState {
    fn reset_context(&mut self);
}
```

- Clear `messages` but preserve the first `Message::System` if present.
- Clear `tool_calls`, `tool_results`, `last_reasoning_content`, `summary`.
- Reset `turn_count` to 0.

### CompactState

```rust
pub trait CompactState {
    fn compact_context(&mut self, instructions: Option<String>);
}
```

- Use the existing compression graph (`loom/src/compress/`) to summarize messages.
- If `instructions` is provided, pass it as a focus hint to the compression prompt.
- Replace `messages` with: `[system_prompt] + [summary_message]`.
- Set `summary` field.

### SummarizeState

```rust
pub trait SummarizeState {
    fn summarize(&mut self);
}
```

- Generate a summary of the current `messages` (full conversation so far).
- Store the result in the `summary` field.
- Unlike compact, messages are **not** removed — the summary is additive.

## Command Implementation Details

### `/reset` — Clear Context

**Aliases**: `/clear`, `/new`

**Behavior**:
1. Find and preserve the first `Message::System` in `messages` (system prompt).
2. Clear all other messages.
3. Reset all runtime fields: `tool_calls`, `tool_results`, `last_reasoning_content`, `turn_count`, `summary`, `think_count`, `message_count_after_last_think`.
4. Keep `usage` and `total_usage` intact (cost tracking should persist across resets).

**Sync** — No LLM call. Pure in-memory state mutation.

**Implementation** (`loom/src/command/builtins.rs`):

```rust
pub trait ResetState {
    fn reset_context(&mut self);
}

impl ResetState for ReActState {
    fn reset_context(&mut self) {
        let system = self.messages.iter()
            .find(|m| matches!(m, Message::System(_)))
            .cloned();
        self.messages.clear();
        if let Some(sys) = system {
            self.messages.push(sys);
        }
        self.tool_calls.clear();
        self.tool_results.clear();
        self.last_reasoning_content = None;
        self.turn_count = 0;
        self.summary = None;
        self.think_count = 0;
        self.message_count_after_last_think = None;
        self.should_continue = true;
    }
}
```

**Checkpoint interaction**: `/reset` clears the in-memory state. If a checkpointer is configured, the old checkpoints remain in DB. The next invoke will start fresh. If desired, the integration layer can also call `checkpointer.reset(thread_id)` to delete old checkpoints (Telegram bot already does this).

---

### `/compact` — Compress Context

**Aliases**: none

**Arguments**: `/compact [instructions]` — optional focus instructions appended to the summary prompt.

**Behavior**:
1. Call `compaction::prune(messages, config)` to remove old tool results beyond token limit.
2. Call `compaction::compact(messages, llm, config)` to summarize older messages, keeping the most recent N messages verbatim.
3. If `instructions` is provided, inject it into the summary prompt (override `build_summary_prompt`).
4. Replace `state.messages` with the compacted result.
5. Set `state.summary` to the generated summary text.

**Async** — Requires LLM call for summarization. The `execute` function must be async.

**Key decision**: `/compact` reuses the existing `loom::compress::compaction` module. It does NOT enter the ReAct graph. Instead, it directly calls `prune` + `compact` with the state's messages and the runner's `LlmClient`.

**Implementation** (`loom/src/command/builtins.rs`):

```rust
pub trait CompactState {
    fn messages(&self) -> &[Message];
    fn messages_mut(&mut self) -> &mut Vec<Message>;
    fn set_summary(&mut self, summary: String);
}

pub async fn execute_compact(
    state: &mut dyn CompactState,
    llm: &dyn LlmClient,
    config: &CompactionConfig,
    instructions: Option<String>,
) -> Result<String, AgentError> {
    let messages = state.messages().to_vec();

    // Step 1: prune old tool results
    let pruned = compaction::prune(messages, config);

    // Step 2: compact (summarize older messages via LLM)
    let compacted = compaction::compact(&pruned, llm, config).await?;

    // Step 3: if instructions provided, we need a custom compact
    //   — For now, instructions are appended as context to the summary.
    //   — Future: inject into build_summary_prompt.

    // Extract summary text from the first System message (compact output format)
    let summary = compacted.first()
        .and_then(|m| match m {
            Message::System(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();

    *state.messages_mut() = compacted;
    state.set_summary(summary);

    Ok("Context compacted.".into())
}
```

**Dependency injection**: The caller (integration layer) must provide `LlmClient` and `CompactionConfig`. The core `execute` function cannot own these — they come from the runner/bot/server that holds them.

**Interface change**: `execute()` becomes async and takes additional context:

```rust
pub struct CommandContext<'a, S> {
    pub state: &'a mut S,
    pub llm: Option<&'a dyn LlmClient>,
    pub compaction_config: Option<&'a CompactionConfig>,
}

pub async fn execute<S>(cmd: Command, ctx: &mut CommandContext<'_, S>) -> CommandResult
where
    S: ResetState + CompactState + SummarizeState,
{
    match cmd {
        Command::ResetContext => {
            ctx.state.reset_context();
            CommandResult::Reply("Context cleared.".into())
        }
        Command::Compact { instructions } => {
            // requires llm + config
            let reply = execute_compact(
                ctx.state,
                ctx.llm.unwrap(),
                ctx.compaction_config.unwrap(),
                instructions,
            ).await?;
            CommandResult::Reply(reply)
        }
        // ...
    }
}
```

---

### `/summarize` — Generate Summary

**Aliases**: none

**Behavior**:
1. Take all current `messages` and send them to the LLM with a summarization prompt.
2. Store the LLM's response in `state.summary`.
3. Do **not** modify `messages` — unlike `/compact`, messages are preserved.

**Async** — Requires LLM call.

**Difference from `/compact`**:
- `/compact` replaces messages with a compressed version (removes old messages).
- `/summarize` only generates a summary string and stores it. Messages stay intact.
- `/summarize` is for "give me a snapshot of where we are" without losing context.

**Implementation** (`loom/src/command/builtins.rs`):

```rust
pub trait SummarizeState {
    fn messages(&self) -> &[Message];
    fn set_summary(&mut self, summary: String);
}

pub async fn execute_summarize(
    state: &mut dyn SummarizeState,
    llm: &dyn LlmClient,
) -> Result<String, AgentError> {
    let messages = state.messages();
    if messages.is_empty() {
        return Ok("Nothing to summarize.".into());
    }

    // Reuse the same prompt structure as compaction::build_summary_prompt
    let prompt = compaction::build_summary_prompt(messages);
    let summary_msgs = vec![Message::user(UserContent::Text(prompt))];
    let response = llm.invoke(&summary_msgs).await?;

    state.set_summary(response.content.clone());

    Ok(response.content)
}
```

**Note**: `compaction::build_summary_prompt` is currently `pub(crate)`. It needs to be made `pub` so the command module can reuse it. Alternatively, extract the prompt template into a shared utility.

---

### `/models` — List / Search / Switch Model

**Aliases**: none

**Subcommands**:
- `/models` — Show current model.
- `/models <query>` — Search available models by name/id.
- `/models use <id>` — Switch to a specific model.

**Platform-specific** — This command is **parsed** in core but **executed entirely at the integration layer**. Core returns `CommandResult::PassThrough`, and the integration layer handles it.

**Reason**: Model registries, providers, and persistence are different per entry point:
- **CLI**: Models configured via env/config. Switch by rebuilding `LlmClient`.
- **Telegram Bot**: `ModelSelection` + SQLite store (`telegram-bot/src/model_selection.rs`). Already has search/pagination.
- **ACP Server**: ACP protocol's `SetSessionConfigOption`.

**Integration pattern**:

```rust
// Integration layer (e.g., telegram-bot pipeline)
if let Some(cmd) = loom::command::parse(text) {
    match cmd {
        Command::Models { query } => {
            // handle with telegram's ModelSelection
            return handle_models_command(ctx, query).await;
        }
        Command::ModelsUse { model_id } => {
            return handle_models_use_command(ctx, &model_id).await;
        }
        _ => {
            // delegate to core execute()
            let result = loom::command::execute(cmd, &mut cmd_ctx).await;
            // handle result
        }
    }
}
```

**Telegram Bot**: The existing `/model` command in `telegram-bot/src/command/mod.rs` should be renamed to `/models` and refactored to delegate parsing to `loom::command::parse`. The `ModelSelection` store and search logic stay in the bot crate.

**CLI**: Add a simple model registry in `loom/src/` or reuse the provider config. `/models` lists configured providers + models. `/models use <id>` rebuilds the `LlmClient` for the current session.

## Integration Points

### CLI REPL (`cli/src/repl.rs`)

After reading a line, before `run_cli_turn`:

```rust
if let Some(cmd) = loom::command::parse(&line) {
    // execute and print reply
    continue;
}
```

### Telegram Bot (`telegram-bot/src/pipeline/mod.rs`)

Add unified parse first. Bot-specific commands (`/status`) stay in existing `CommandDispatcher`.
`/models` is already handled by `model_selection.rs` — delegate `Command::Models*` to existing logic.

```rust
if let Some(cmd) = loom::command::parse(text) {
    match loom::command::execute(cmd, &mut state) {
        CommandResult::Reply(msg) => { sender.send_text(chat_id, &msg).await; return Ok(()); }
        CommandResult::PassThrough => {} // handled below
    }
}
// Then existing CommandDispatcher for /status, /model etc.
```

### ACP Server (`loom-acp/src/agent.rs`)

In `prompt()`, before building state and invoking graph:

```rust
if let Some(cmd) = loom::command::parse(&prompt_text) {
    // handle command
}
```

## Changes

| Layer | File | Change |
|---|---|---|
| Core | `loom/src/command/mod.rs` | New: module + re-exports |
| Core | `loom/src/command/parser.rs` | New: `parse()` |
| Core | `loom/src/command/command.rs` | New: `Command` enum, `CommandResult` |
| Core | `loom/src/command/builtins.rs` | New: `ResetState` trait, `execute()` |
| Core | `loom/src/state/react_state.rs` | Add `impl ResetState for ReActState` |
| Core | `loom/src/lib.rs` | Add `pub mod command` |
| CLI | `cli/src/repl.rs` | Add parse + execute before run |
| Bot | `telegram-bot/src/command/mod.rs` | Refactor `ResetCommand` to delegate to `loom::command` |
| Bot | `telegram-bot/src/pipeline/mod.rs` | Add unified parse before agent dispatch |
| ACP | `loom-acp/src/agent.rs` | Add parse in `prompt()` |

## Future

- Custom commands via config files (like Cursor's `.cursor/commands/`).
- `CommandRegistry` for runtime registration.
- Async execute support (for `/compact` and `/summarize` if LLM call is needed).

These are deferred until needed (YAGNI).
