# Telegram Tools

Telegram tools let the Loom agent interact with Telegram directly — send messages, create polls, and send files — during a ReAct run.

## Architecture

```
┌──────────────────────────────┐
│  telegram-bot                │
│  (depends on loom + teloxide)│
│                              │
│  telegram_tools/mod.rs       │── implements TelegramApi trait
│  init_telegram_api(bot)      │── registers impl globally
└──────────┬───────────────────┘
           │
┌──────────▼───────────────────┐
│  loom (crate-agnostic)       │
│                              │
│  tools/telegram/mod.rs       │── TelegramApi trait + global OnceLock
│  tools/telegram/send_message │── telegram_send_message tool
│  tools/telegram/send_poll    │── telegram_send_poll tool
│  tools/telegram/send_document│── telegram_send_document tool
│  tool_source/context.rs      │── ToolCallContext.chat_id
│  memory/config.rs            │── RunnableConfig.chat_id
└──────────────────────────────┘
```

The `TelegramApi` trait lives in `loom` so tools are testable without teloxide. The concrete `TeloxideTelegramApi` lives in `telegram-bot` and is injected at startup via `set_telegram_api()`.

## chat_id Flow

1. `telegram-bot` sets `RunOptions.chat_id` with the Telegram chat ID
2. Loom propagates it through `ReactBuildConfig` → `RunnableConfig.chat_id`
3. `ActNode` reads `run_ctx.config.chat_id` and sets `ToolCallContext.chat_id`
4. Each Telegram tool reads `_ctx.chat_id` as the default target chat

If `chat_id` is not in context, the tool requires `chat_id` in its explicit parameters. If neither is provided, the tool returns an error.

## Available Tools

| Tool | Description |
|------|-------------|
| `telegram_send_message` | Send a text message (supports MarkdownV2, HTML parse modes) |
| `telegram_send_poll` | Create a poll with configurable options |
| `telegram_send_document` | Send a file with optional caption |

## Setup

### 1. Register the tool source

In your agent build, add `TelegramToolsSource` to the tool source chain:

```rust
use loom::tool_source::TelegramToolsSource;

let telegram_source = Box::new(TelegramToolsSource::new());
// Add to AggregateToolSource or pass to ActNode
```

### 2. Initialize the API at bot startup

```rust
use telegram_bot::telegram_tools::init_telegram_api;

// After creating the Bot instance:
init_telegram_api(bot.clone());
```

### 3. Pass chat_id through RunOptions

```rust
let opts = RunOptions {
    chat_id: Some(chat_id),  // i64 Telegram chat ID
    ..Default::default()
};
```

## Multi-bot Considerations

The global API uses `OnceLock`, which means only the first registered bot takes effect. For multi-bot setups, use per-bot context instead of the global singleton (future work).

## File Reference

| File | Role |
|------|------|
| `loom/src/tools/telegram/mod.rs` | `TelegramApi` trait, global state, shared params |
| `loom/src/tools/telegram/send_message.rs` | `telegram_send_message` tool |
| `loom/src/tools/telegram/send_poll.rs` | `telegram_send_poll` tool |
| `loom/src/tools/telegram/send_document.rs` | `telegram_send_document` tool |
| `loom/src/tool_source/telegram_tools_source.rs` | `TelegramToolsSource` (ToolSource adapter) |
| `telegram-bot/src/telegram_tools/mod.rs` | `TeloxideTelegramApi` impl + `init_telegram_api` |
| `loom/src/tool_source/context.rs` | `ToolCallContext.chat_id` field |
| `loom/src/memory/config.rs` | `RunnableConfig.chat_id` field |
