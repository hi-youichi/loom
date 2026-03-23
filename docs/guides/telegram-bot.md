# Telegram Bot

The Loom Telegram Bot connects a Loom agent to Telegram with streaming chat, media downloads, and multi-bot management.

## Prerequisites

- Loom is installed and configured (`~/.loom/` exists)
- At least one Telegram bot created via [@BotFather](https://t.me/BotFather), with a token
- LLM-related environment variables set (e.g. `OPENAI_API_KEY`)

## Quick start

### 1. Create the config file

Put the config at `~/.loom/telegram-bot.toml`:

```toml
[settings]
download_dir = "downloads"
log_level = "info"

[bots.my_bot]
token = "${TELOXIDE_TOKEN}"
enabled = true
description = "My assistant"
```

Tokens support `${ENV_VAR}` interpolation so secrets are not stored in plain text.

### 2. Run

```bash
cargo run -p telegram-bot
```

After startup, the bot receives updates via Telegram long polling; logs appear in the terminal.

## Commands

| Command | Description |
|---------|-------------|
| `/reset` | Clears the current chat’s session memory (deletes Loom SQLite checkpoints) |
| `/status` | Checks whether the bot is running |

Plain text messages are sent to the Loom agent for a normal conversation.

## Features

### Streaming chat

After you send text, the bot shows the agent’s reasoning in real time:

- **Think phase** — Shows the agent’s thoughts (🤔); one Telegram message is edited as content grows
- **Act phase** — Shows tool use (⚡); each tool shows status (🔧 in progress / ✅ success / ❌ failure)

Each Think/Act round uses its own Telegram message; streaming is done by editing those messages.

### Reply context

If you reply to a message with text, the bot passes both the replied-to text and your reply to the agent for coherent follow-ups.

### Media downloads

The bot can receive and save:

| Type | Description |
|------|-------------|
| 📷 Photo | Largest resolution is chosen automatically |
| 📁 Document | Original filename and extension preserved |
| 🎬 Video | Format detected automatically |

Files go under the directory set by `download_dir`, grouped by `chat_id`. Optional JSON metadata (file_id, MIME type, size, timestamp, etc.) can be written next to files when enabled in code.

### Mention gating (group chats)

With `only_respond_when_mentioned = true`, the bot only responds when:

- The message contains `@bot_username`, or
- The message is a reply to something the bot sent

Useful in groups where you do not want the bot on every message. Commands (`/reset`, `/status`) are not affected.

### Multiple bots

One process can run several bot instances:

```toml
[bots.assistant]
token = "${ASSISTANT_TOKEN}"
enabled = true
description = "Main assistant"

[bots.helper]
token = "${HELPER_TOKEN}"
enabled = true
description = "Helper bot"

[bots.test]
token = "..."
enabled = false
description = "Test bot (disabled)"
```

Set `enabled = false` to disable a bot without removing its block.

### Session management

Each Telegram chat maps to its own Loom session (thread ID: `telegram_{chat_id}`). Session data is persisted via Loom’s SQLite checkpoints, so conversation state survives bot restarts.

`/reset` clears all checkpoints for the current chat and starts a fresh thread.

## Configuration reference

Full structure:

```toml
[settings]
# Media download directory (relative to ~/.loom/ or absolute)
download_dir = "downloads"

# Log level: trace, debug, info, warn, error
log_level = "info"

# Log file (optional; if unset, logs go to the console only)
# log_file = "logs/telegram-bot.log"

# Only respond when @mentioned or when replying to the bot (default false)
only_respond_when_mentioned = false

# Streaming UI
[settings.streaming]
# Max characters for Think phase (0 = unlimited, default 500)
max_think_chars = 500

# Max characters for Act phase (0 = unlimited, default 500)
max_act_chars = 500

# Show Think phase (default true)
show_think_phase = true

# Show Act phase (default true)
show_act_phase = true

# Emoji for Think messages (default 🤔)
think_emoji = "🤔"

# Emoji for Act messages (default ⚡)
act_emoji = "⚡"

# Minimum interval between Telegram edits, in milliseconds (default 300)
throttle_ms = 300

# At least one bot is required
[bots.my_bot]
# Bot token; supports ${ENV_VAR} interpolation
token = "${TELOXIDE_TOKEN}"

# Enabled (default true)
enabled = true

# Optional description
description = "My bot"
```

### Config resolution order

1. `$LOOM_HOME/telegram-bot.toml` if `LOOM_HOME` is set
2. `~/.loom/telegram-bot.toml`
3. `telegram-bot.toml` in the current working directory

### Default values

| Setting | Default |
|---------|---------|
| `download_dir` | `telegram-bot-downloads` |
| `log_level` | `info` |
| `polling_timeout` | `30` seconds |
| `retry_timeout` | `5` seconds |
| `only_respond_when_mentioned` | `false` |
| `streaming.max_think_chars` | `500` |
| `streaming.max_act_chars` | `500` |
| `streaming.show_think_phase` | `true` |
| `streaming.show_act_phase` | `true` |
| `streaming.throttle_ms` | `300` |
