# Telegram Bot — End-to-end test cases

This document defines **manual end-to-end (E2E)** scenarios for the `telegram-bot` binary: a real Telegram client, the live Bot API, and a running Loom stack (including LLM credentials). It complements **automated** tests under `telegram-bot/tests/`, which use mocks and do not call Telegram or external LLMs.

For user-facing setup, see [Telegram Bot](telegram-bot.md).

## Scope

| Layer | What is tested | Typical command / tool |
|-------|----------------|-------------------------|
| **E2E (this doc)** | Full path: Telegram → bot process → Loom agent → Telegram UI | Manual messages in Telegram app |
| **Integration (repo)** | Traits, settings, message-flow mirrors | `cargo test -p telegram-bot` |
| **Unit (repo)** | Config, utils, small modules | `cargo test -p telegram-bot --lib` |

**Out of scope for this checklist:** Webhook mode (bot uses long polling), health/metrics HTTP endpoints if not wired into the shipped binary.

## Prerequisites

- A **dedicated test bot** from [@BotFather](https://t.me/BotFather) (do not use production tokens in shared chats).
- `telegram-bot.toml` configured and process running (`cargo run -p telegram-bot` or Docker per [bot-runtime](../../bot-runtime/README.md)).
- Loom global config valid (`~/.loom/` or `LOOM_HOME`): LLM keys and models as required.
- **Private chat** with the test bot for baseline cases; optional **test group** with the bot added for mention-gating cases.
- Access to **host logs** (terminal or `docker-compose logs`) for debugging failures.

## Conventions

- **Test ID**: Stable identifier for traceability (e.g. in bug reports).
- **Priority**: P0 = smoke / release gate; P1 = core product; P2 = edge / operational.
- **Result**: Pass / Fail / Blocked (environment).
- **Timeout**: Unless stated otherwise, wait **30 seconds** after sending a message. If no response within that window, mark as Fail or Blocked.

---

## Test cases

### P0 — Smoke

#### E2E-TG-001 — Process starts and `/status` responds

| Field | Content |
|-------|---------|
| **Priority** | P0 |
| **Preconditions** | Config has one `enabled` bot; token valid; process just started. |
| **Steps** | 1. Open private chat with the bot.<br>2. Send `/status`. |
| **Expected** | Bot replies within 10 s with a message indicating it is running (e.g. contains "running" / "✅"). |
| **Notes** | If no reply, verify token, network, and logs for dispatcher errors. |

#### E2E-TG-002 — Plain text invokes agent and streaming UI updates

| Field | Content |
|-------|---------|
| **Priority** | P0 |
| **Preconditions** | `only_respond_when_mentioned` is `false` (private chat). LLM reachable. |
| **Steps** | 1. Send a short question that forces at least one model turn (e.g. "Say hello in one sentence").<br>2. Observe the chat for up to 30 s. |
| **Expected** | One or more bot messages appear; Think/Act style updates may edit messages (emoji headers per [streaming config](telegram-bot.md)). No unhandled crash in logs. |
| **Notes** | Exact wording depends on `settings.streaming`. Failure to stream may still yield a final error message — see E2E-TG-010. |

---

### P1 — Commands and session

#### E2E-TG-003 — `/reset` clears session checkpoints

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Prior conversation in the same chat (so checkpoints exist). |
| **Steps** | 1. Send `/reset`.<br>2. Read the bot reply.<br>3. (Optional) Ask something that should not depend on the old thread if you had established a strong context before reset. |
| **Expected** | Reply indicates success and reports how many checkpoints were deleted (non-negative integer). Follow-up behavior should reflect a fresh thread (`telegram_{chat_id}`). |
| **Notes** | If DB missing or empty, count may be `0` — still a valid response if no error. |

#### E2E-TG-004 — Reply threading adds quoted context to the prompt

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Bot responds to normal text. |
| **Steps** | 1. Send message A: "Remember this code: BLUE-42".<br>2. Reply to message A with: "What was the code?". |
| **Expected** | Bot answer incorporates or references `BLUE-42` (or equivalent), showing reply context reached the agent. |
| **Notes** | You are replying to your own message — that is intentional. The code extracts the text of the replied-to message regardless of sender. Flaky if the model ignores context; retry once or inspect logs for the composed prompt. |

#### E2E-TG-025 — `/reset <arg>` is accepted as reset command

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Same as E2E-TG-003; chat has existing checkpoints. |
| **Steps** | 1. Send `/reset dry-run` (or any non-empty suffix). |
| **Expected** | Bot treats it as reset command and replies with reset success/failure message; it does not pass this text to the agent. |
| **Notes** | Current command detection allows `/reset` followed by a space and extra text. This case protects that compatibility behavior. |

---

### P1 — Media downloads

#### E2E-TG-005 — Photo download

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Write access to the download directory used by the running process (default handler uses under-workdir `downloads` unless customized in code). |
| **Steps** | 1. Send a photo to the bot (any small image). |
| **Expected** | Bot replies confirming the photo was saved (current UI: `📷 图片已保存: ...`). File appears under `downloads/<chat_id>/` on the host when using default layout. |
| **Notes** | UI strings are currently Chinese and may change. Path in message is host/container-specific; confirm on the machine running the binary. |

#### E2E-TG-006 — Document download

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Same as E2E-TG-005. |
| **Steps** | 1. Send a small document (e.g. `.txt`). |
| **Expected** | Bot confirms file saved (current UI: `📁 文件已保存: ...`); file present under chat subfolder with plausible extension. |

#### E2E-TG-007 — Video download

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Same as E2E-TG-005; Telegram allows video upload. |
| **Steps** | 1. Send a short video file. |
| **Expected** | Bot confirms video saved (current UI: `🎬 视频已保存: ...`); file present on disk. |

---

### P1 — Mention gating (groups)

#### E2E-TG-008 — `only_respond_when_mentioned` suppresses generic messages

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Test group with bot; config `only_respond_when_mentioned = true`; bot `@username` known. |
| **Steps** | 1. Send plain text without `@bot`.<br>2. Send text that includes `@bot` and a question. |
| **Expected** | (1) No agent reply. (2) Agent reply appears. |
| **Notes** | Depends on Telegram mention resolution; use exact username from BotFather. |

#### E2E-TG-009 — Commands work without mention in group

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Same group as E2E-TG-008; mention gating still `true`. |
| **Steps** | 1. Send `/status` without `@bot`.<br>2. Send `/reset` without `@bot` (use only in test group). |
| **Expected** | Both commands get responses from the bot. |
| **Notes** | This works because `router.rs` processes `/reset` and `/status` before the mention gate check — if command handling is moved after the gate in a future refactor, this test will catch it. `/reset` wipes session for that group chat — use a disposable group. |

#### E2E-TG-013 — Reply-to-bot triggers response without `@` mention

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Same group as E2E-TG-008; mention gating `true`. Bot has already sent at least one message in the group. |
| **Steps** | 1. Reply to a bot message with a plain text question (no `@bot` in text). |
| **Expected** | Agent reply appears, showing `is_reply_to_bot` path works independently of `@` mention. |
| **Notes** | E2E-TG-008 only tests the `@bot` path; this covers the `is_reply_to_bot` branch. |

#### E2E-TG-022 — Group command with `@bot` suffix is not treated as built-in command

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Same group as E2E-TG-008; mention gating `true`; bot username known (for example `@my_bot`). |
| **Steps** | 1. Send `/status@my_bot`.<br>2. Send `/reset@my_bot` in a disposable test group. |
| **Expected** | The bot does **not** return built-in command responses (`✅ Bot is running!`, `🔄 Session reset! ...`) for suffix commands. Messages are handled by the normal text/agent path instead. |
| **Notes** | This locks current behavior. If explicit `/command@bot` support is added later, update expected results to require command success. |

#### E2E-TG-023 — Mention gating also applies in private chat

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Private chat with bot; set `only_respond_when_mentioned = true`; restart bot. |
| **Steps** | 1. Send plain text without `@bot`.<br>2. Send text including `@bot` and a question. |
| **Expected** | (1) No agent reply. (2) Agent reply appears for the mentioned message. |
| **Notes** | Gating is implemented globally (not group-only). This case prevents accidental behavior changes between private and group chats. |

---

### P2 — Errors and resilience

#### E2E-TG-010 — LLM or agent failure surfaces to the user

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Temporarily invalid API key or blocked network **in a test environment only**. |
| **Steps** | 1. Send normal text that triggers an agent run. |
| **Expected** | User receives an error text from the bot; process stays up; subsequent `/status` still works after restoring credentials. |
| **Notes** | Restore valid config immediately after the test. |

#### E2E-TG-011 — Unicode and emoji in user text

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | P0 text path works. |
| **Steps** | 1. Send: `Hello 中文 🎉 test`. |
| **Expected** | Bot handles without panic; response or error is delivered; logs show no encoding-related crash. |

#### E2E-TG-014 — Photo with caption does not invoke agent

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Bot is running; `only_respond_when_mentioned` is `false`. |
| **Steps** | 1. Send a photo with a caption containing a question (e.g. "What is this?"). |
| **Expected** | Bot saves the photo and replies with the saved-path message. The caption text does **not** trigger an agent run (Teloxide exposes caption via `msg.caption()`, not `msg.text()`). |
| **Notes** | If the agent is invoked on the caption, it means caption handling was added — update this case accordingly. |

#### E2E-TG-015 — `/reset` on a fresh chat with no checkpoints

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | A chat where the bot has never received a message before (or checkpoints were already cleared). |
| **Steps** | 1. Send `/reset`. |
| **Expected** | Bot replies with success and a count of `0`. No error message. |

#### E2E-TG-016 — Large file download fails gracefully

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Same as E2E-TG-005. |
| **Steps** | 1. Send a file larger than 20 MB (Telegram Bot API download limit). |
| **Expected** | Bot replies with an error message (current UI: `❌ 下载失败: ...`). Process stays up; `/status` still works. |
| **Notes** | Telegram may reject the `getFile` call for files > 20 MB. The bot should surface the error, not panic. |

---

### P2 — Streaming configuration

#### E2E-TG-018 — `show_think_phase = false` hides Think messages

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Set `show_think_phase = false` in `[settings.streaming]`; restart bot. |
| **Steps** | 1. Send a text question that triggers agent reasoning. |
| **Expected** | No Think-phase message appears (no 🤔 header). Act-phase messages (⚡) still appear if `show_act_phase` is `true`. Agent still returns a final answer. |
| **Notes** | Restore `show_think_phase = true` after the test if needed for other cases. |

#### E2E-TG-024 — `show_act_phase = false` hides Act messages

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Set `show_act_phase = false` and `show_think_phase = true` in `[settings.streaming]`; restart bot. |
| **Steps** | 1. Send a text question that triggers at least one tool/action step. |
| **Expected** | Think-phase messages (🤔) appear. No Act-phase message appears (no ⚡ header). Bot still completes the run and sends final output. |
| **Notes** | Restore `show_act_phase = true` after the test. |

#### E2E-TG-026 — `show_think_phase = false` and `show_act_phase = false`

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Set both `show_think_phase = false` and `show_act_phase = false`; restart bot. |
| **Steps** | 1. Send a normal text prompt. |
| **Expected** | Bot does not crash or hang. If no user-visible streaming message appears, treat as current behavior and verify process health via `/status`. |
| **Notes** | This is a resilience/behavior-lock test. If future implementation adds a separate final-answer message path, update expected results accordingly. |

#### E2E-TG-031 — Long response does not silently lose trailing content

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | Use a prompt likely to produce long output (> `settings.streaming.max_think_chars` or > `settings.streaming.max_act_chars`), with default streaming limits enabled. |
| **Steps** | 1. Send a long-form request (e.g. "Write a detailed 20-item checklist with short explanations").<br>2. Observe whether streamed messages are truncated with no continuation/final completion text.<br>3. Compare with logs or rerun with larger limits to confirm whether user-visible content was incomplete. |
| **Expected** | User can access the full answer (either fully streamed, continued in follow-up messages, or via an explicit truncated marker with continuation path). There should be no silent loss where remaining content is invisible to the user. |
| **Notes** | This case comes from a production incident. If silent truncation is still current behavior, mark as **Fail** and track as release-blocking for content integrity. |

#### E2E-TG-032 — Act phase shows tool start/end messages

| Field | Content |
|-------|---------|
| **Priority** | P1 |
| **Preconditions** | `show_act_phase = true`; use a prompt that reliably triggers at least one tool call (for example asking for file/system inspection in an environment where tools are available). |
| **Steps** | 1. Send a tool-triggering prompt.<br>2. Observe the Act-phase message (`⚡ Act #...`) during execution.<br>3. Verify at least one tool line appears (`🔧 ...` in progress and/or `✅/❌ ...` completion). |
| **Expected** | Tool events are visible in Act-phase updates. User can see which tool was called and whether it succeeded/failed. No silent tool activity where Act message stays empty while tools actually ran. |
| **Notes** | This case comes from production feedback: "tool messages not shown in Act". If reproducible, mark as **Fail** and track as release-blocking for observability/transparency. |

---

### P2 — Concurrency

#### E2E-TG-019 — Rapid consecutive messages do not crash the bot

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Bot is running; LLM reachable. |
| **Steps** | 1. Send two different questions in quick succession (< 2 s apart). |
| **Expected** | Both questions eventually get responses. Process stays up. Messages from the two runs may interleave — that is acceptable (no queuing mechanism exists). |
| **Notes** | This is a known-behavior test, not a correctness test. The goal is to confirm no panic, deadlock, or dropped connection. |

---

### P2 — Unhandled message types

#### E2E-TG-020 — Sticker / location / voice are silently ignored

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Bot is running. |
| **Steps** | 1. Send a sticker.<br>2. Send a location.<br>3. (Optional) Send a voice message. |
| **Expected** | Bot does not reply to any of them. No error in logs beyond `debug`-level "Unhandled message kind". Process stays up. |
| **Notes** | Voice, sticker, and location are not handled by `default_handler`. If support is added later, update this case. |

---

### P2 — Startup

#### E2E-TG-021 — Invalid token prevents bot from receiving messages

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Set an obviously invalid token (e.g. `"invalid"`) for one bot in config. |
| **Steps** | 1. Start the process.<br>2. Observe logs for 10 s. |
| **Expected** | Process starts but the bot fails to connect (Teloxide dispatcher errors in logs). If a second bot with a valid token is configured and enabled, it should still work independently. |
| **Notes** | `bot.rs` calls `get_me()` at startup; on failure `bot_username` is set to empty string, and the dispatcher may fail on first poll. Verify the process does not panic. |

#### E2E-TG-027 — Missing env var in token causes startup failure

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Config uses token interpolation (e.g. `token = "${TELOXIDE_TOKEN}"`) but the referenced env var is intentionally unset in test environment. |
| **Steps** | 1. Start `telegram-bot`.<br>2. Observe startup output/logs. |
| **Expected** | Startup fails fast with config interpolation/validation error. No bot enters polling loop. |
| **Notes** | Restore env vars immediately after the test to avoid side effects on subsequent cases. |

#### E2E-TG-028 — No enabled bots fails fast at startup

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Config has at least one `[bots.*]` block, but all have `enabled = false`. |
| **Steps** | 1. Start `telegram-bot`. |
| **Expected** | Process exits early with a startup error (no enabled bots); no long-polling loop starts. |
| **Notes** | This validates deployment safety checks for misconfigured environments. |

---

### P2 — Multi-bot deployment

#### E2E-TG-012 — Two enabled bots in one config

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Two tokens, two `[bots.*]` entries with `enabled = true`. |
| **Steps** | 1. Start one `telegram-bot` process.<br>2. Message bot A in its chat; message bot B in its chat. |
| **Expected** | Each chat gets responses from the correct bot; sessions isolated per `chat_id` (same numeric chat_id in different bots is still different bot accounts — use two private chats). |
| **Notes** | Validates config loading and `run_bots` wiring, not Telegram API limits. |

---

### P2 — Session persistence

#### E2E-TG-017 — Conversation state survives bot restart

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Bot is running; a prior conversation has been established. |
| **Steps** | 1. Send "Remember: the secret word is MANGO".<br>2. Wait for agent reply.<br>3. Stop the bot process (`Ctrl+C` or `docker-compose down`).<br>4. Start the bot again.<br>5. Send "What is the secret word?". |
| **Expected** | Agent reply references `MANGO` (or equivalent), showing Loom SQLite checkpoints persisted across restart. |
| **Notes** | Depends on Loom's checkpoint / memory working correctly. If the model does not recall, check whether the thread ID (`telegram_{chat_id}`) matches before and after restart. |

#### E2E-TG-029 — Transient network outage recovers without process restart

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Bot running and healthy; ability to temporarily block outbound network from host/container. |
| **Steps** | 1. Send `/status` and confirm normal response.<br>2. Block network for ~20-30 s.<br>3. Send a message during outage (expect failure/no response).<br>4. Restore network.<br>5. Send `/status` again. |
| **Expected** | Process stays alive through outage and resumes handling messages after connectivity is restored, without manual restart. |
| **Notes** | Accept temporary message loss/delay during outage; recovery is the assertion. |

---

### P2 — Configuration correctness

#### E2E-TG-030 — `settings.download_dir` is honored by media saves

| Field | Content |
|-------|---------|
| **Priority** | P2 |
| **Preconditions** | Set `settings.download_dir` to a non-default path (e.g. `telegram-bot-downloads-custom`) and restart bot. |
| **Steps** | 1. Send a photo or document.<br>2. Inspect filesystem on the machine running the process. |
| **Expected** | Downloaded file is stored under configured `download_dir/<chat_id>/...`, not hardcoded default paths. |
| **Notes** | If files still go to another directory, record as config-wiring defect. |

---

## Test summary

| ID | Title | Priority |
|----|-------|----------|
| 001 | `/status` responds | P0 |
| 002 | Text → streaming agent | P0 |
| 003 | `/reset` clears checkpoints | P1 |
| 004 | Reply threading context | P1 |
| 005 | Photo download | P1 |
| 006 | Document download | P1 |
| 007 | Video download | P2 |
| 008 | Mention gating suppresses | P1 |
| 009 | Commands bypass mention gate | P1 |
| 010 | LLM failure error message | P2 |
| 011 | Unicode / emoji text | P2 |
| 012 | Multi-bot in one process | P2 |
| 013 | Reply-to-bot without @ | P1 |
| 014 | Caption does not invoke agent | P2 |
| 015 | `/reset` on fresh chat | P2 |
| 016 | Large file download fails gracefully | P2 |
| 017 | Session survives restart | P2 |
| 018 | `show_think_phase = false` hides Think | P2 |
| 019 | Rapid consecutive messages | P2 |
| 020 | Sticker / location / voice ignored | P2 |
| 021 | Invalid token startup behavior | P2 |
| 022 | Group `/command@bot` behavior | P1 |
| 023 | Private-chat mention gating | P1 |
| 024 | `show_act_phase = false` hides Act | P2 |
| 025 | `/reset <arg>` compatibility | P1 |
| 026 | Think+Act both disabled | P2 |
| 027 | Missing token env var on startup | P2 |
| 028 | All bots disabled on startup | P2 |
| 029 | Network outage recovery | P2 |
| 030 | `settings.download_dir` wiring | P2 |
| 031 | Long response completeness | P1 |
| 032 | Act tool visibility | P1 |

**Total: 32 cases** — P0: 2, P1: 11, P2: 19.

## Coverage matrix

Use this table to quickly verify requirement coverage before sign-off.

| Capability / Risk | Covered by test IDs |
|-------------------|---------------------|
| Basic liveness and bot availability | 001, 021 |
| Text message to agent (baseline path) | 002 |
| Session reset and thread lifecycle | 003, 015, 025 |
| Reply-context propagation | 004, 013 |
| Mention gating behavior (group/private) | 008, 009, 013, 022, 023 |
| Media download success paths | 005, 006, 007 |
| Media edge cases and failures | 014, 016 |
| Streaming UI feature flags and stability | 018, 024, 026 |
| Act tool event visibility | 032 |
| Concurrency / interleaving tolerance | 019 |
| Unsupported message kinds | 020 |
| Multi-bot isolation in one process | 012 |
| Startup config validation and fail-fast | 027, 028 |
| Runtime resilience and recovery | 010, 029 |
| Config-to-runtime wiring correctness | 030 |
| Unicode / i18n safety | 011 |
| Long-response content completeness | 031 |

## Execution checklist (release smoke)

Run in order for a minimal gate:

1. E2E-TG-001 — `/status`
2. E2E-TG-002 — Text + streaming
3. E2E-TG-003 — `/reset`
4. E2E-TG-005 or E2E-TG-006 — One media type
5. E2E-TG-008 — Mention gating (if deploying to groups)

Recommended complete gate (before release candidate):

6. E2E-TG-022 — Group `/command@bot` behavior
7. E2E-TG-023 — Private-chat mention gating
8. E2E-TG-024 — `show_act_phase = false`
9. E2E-TG-029 — Network outage recovery
10. E2E-TG-030 — `settings.download_dir` wiring
11. E2E-TG-031 — Long response completeness
12. E2E-TG-032 — Act tool visibility

Record results in the template below (minimal gate) or an expanded table for complete gate.

### Result template

| Date | Commit | Environment | ID | Result | Notes |
|------|--------|-------------|----|--------|-------|
| | | | 001 | | |
| | | | 002 | | |
| | | | 003 | | |
| | | | 005 | | |
| | | | 008 | | |

Copy and fill one table per release or test run.

## Automated E2E (future)

Possible directions (not implemented in-tree at time of writing):

- **Telegram Bot API** integration tests with a test token and controlled chat (CI secrets, rate limits).
- **Contract tests** against a stub Telegram server (high maintenance).

Until then, treat this document as the **authoritative manual E2E suite** for `telegram-bot`.

## Related links

- [Telegram Bot user guide](telegram-bot.md)
- [Loom testing overview](testing.md)
- [bot-runtime README](../../bot-runtime/README.md) — Docker-based runs
