# CLI Output UX Improvement Plan

## Background

The CLI output system is spread across these core files:

- `cli/src/run/agent.rs` — stream event handling, stderr display callback
- `cli/src/run/display.rs` — state formatting and truncation
- `cli/src/output.rs` — stdout/file output helpers (JSON & text)
- `cli/src/repl.rs` — interactive REPL loop
- `cli/src/display_limits.rs` — truncation constants

Current output relies on `eprintln!` scattered throughout event handlers with only a `verbose` on/off toggle. This leads to either too little information (normal mode) or an overwhelming Debug dump (verbose mode).

## Current Problems

### P1: No progress feedback during LLM thinking

In normal mode, after startup info is printed, the terminal goes completely silent until the reply streams in. For multi-turn agent runs that involve tool calls, this can be 10–30 seconds of perceived "freeze".

Current output (normal mode):
```
agent: dev (project) — Code assistant
loaded tools: bash, read, edit, glob, grep
model: claude-sonnet-4 (200K context)

... nothing for 15 seconds ...

Here is the code you asked for:
```

### P2: stderr info is flat and noisy

Startup info, tool names, LLM stats, and state dumps are all printed with the same visual weight. Users cannot quickly scan for what matters.

### P3: Thinking and reply content are mixed

`print_stream_chunk` sends thinking to stderr and reply to stdout, but there is no visual boundary between them. In verbose mode, thinking content blends into the state dump.

### P4: Verbose mode state dump is unreadable

`format_react_state_display` produces a Rust Debug-style dump. The nested `ReActState { messages: ..., tool_calls: ..., tool_results: ... }` format was designed for developer debugging, not user comprehension.

### P5: Inconsistent LLM usage format across agents

- ReAct: `\nLLM: 2.35s | prefill: 1200t / 0.85s = 1412 t/s | decode: 800t / 1.50s = 533 t/s`
- DUP/TOT/GOT: `\nLLM: prompt=1200, completion=800`
- Final summary: `LLM: 3.50s, 571 tokens/s (prompt: 1200, completion: 800)`

Three different formats for the same concept.

### P6: REPL is minimal

- Bare `>` prompt with no context
- No command history (↑↓ arrows)
- No color differentiation

---

## Improvement Plan

### Phase 1: Progress Indicator (Highest Impact)

**Goal:** Show real-time progress so the user never wonders "is it stuck?"

**Design:**

1. Add a single-line status bar that updates in place using `\r` (carriage return):
   ```
   ⠋ Thinking...
   ⠋ Running tool: bash (echo hello)
   ⠋ Thinking... (turn 2)
   ✓ Done
   ```
2. Spinner cycles through `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` on a 150ms timer.
3. Tool calls show tool name + first argument (truncated to terminal width).
4. When streaming reply starts, the spinner is cleared and reply prints normally.

**Implementation:**

- Add `cli/src/run/spinner.rs` — a simple spinner struct that writes to stderr.
- `Spinner::new(label)` starts the animation.
- `Spinner::update(label)` changes the status text.
- `Spinner::finish()` clears the line.
- The spinner is driven by `on_event_react` / `on_event_dup` etc. using `TaskStart` and `Updates` events.

**Dependency:** No new crate needed. Use a `std::thread::spawn` + `std::sync::mpsc` for the timer, or `tokio::time` interval. The implementation should check if stderr is a TTY before enabling the spinner (fall back to static `eprintln!` when piped).

**Files changed:** `agent.rs` (integrate spinner into event handlers), new file `spinner.rs`.

---

### Phase 2: Structured stderr Panel Format

**Goal:** Make stderr info scannable at a glance.

**Design:**

Use prefixed, categorized lines with consistent format:

```
_AGENT  dev (project) — Code assistant
_TOOLS  bash, read, edit, glob, grep
_MODEL  claude-sonnet-4 (200K context)
```

During execution:
```
_CALL   bash: echo "hello world"
_CALL   read: src/main.rs
_DONE   bash: echo "hello world" ✓
_DONE   read: src/main.rs ✓
```

Usage line (unified format):
```
_USAGE  2.35s | 1.2K in + 800 out = 2.0K @ 850 t/s
```

**Implementation:**

- Add `cli/src/run/format.rs` with helper functions:
  - `format_panel_line(category, message)` — produces `  CATEGORY  message` with ANSI color for the prefix.
  - `format_tool_status(tool_name, args_summary, success)` — produces tool call/done lines.
  - `format_usage_line(duration, prompt_tokens, completion_tokens, prefill_duration, decode_duration)` — unified format.
- Add `--no-color` flag or respect `NO_COLOR` env var.
- Replace all raw `eprintln!` calls in `agent.rs` with these formatters.

**Files changed:** `agent.rs`, new file `format.rs`, `args.rs` (add `--no-color`).

---

### Phase 3: Thinking / Reply Visual Separation

**Goal:** User can clearly distinguish thinking from the final answer.

**Design:**

Normal mode:
```
⠋ Thinking...
<thinking content streamed here, dimmed/gray>
────────────────────
<reply content, normal brightness>
```

Verbose mode:
```
[THINKING]
<thinking content, possibly multi-line>
[/THINKING]
[REPLY]
<reply content>
[/REPLY]
```

**Implementation:**

- Modify `print_stream_chunk`:
  - For thinking chunks: if TTY, wrap in dim ANSI (`\x1b[2m...\x1b[0m`). If not TTY, prefix with `[thinking] `.
  - For reply chunks: print as-is in normal mode, or prefix with `[reply] ` in verbose.
- Add a separator line when transitioning from thinking to reply (track state in `EventState`).

**Files changed:** `agent.rs` (modify `print_stream_chunk` and `EventState`).

---

### Phase 4: Unified LLM Usage Format

**Goal:** One consistent format for all agents and contexts.

**Format:**
```
_USAGE  2.35s | 1.2K↓ 800↑ = 2.0K @ 850 t/s
```

With prefill/decode detail (verbose only):
```
_USAGE  2.35s | prefill: 1.2K/0.85s=1.4K t/s | decode: 800/1.50s=533 t/s | total: 2.0K @ 850 t/s
```

**Implementation:**

- Extract a shared `format_usage_line(...)` function into `format.rs`.
- Replace the three different `eprintln!` patterns in `on_event_react`, `on_event_dup`, `on_event_tot`, `on_event_got`, and the final summary in `run_agent_wrapper` with calls to this function.
- Each event handler calls the same function with available parameters (prefill/decode are `Option`).

**Files changed:** `agent.rs` (all four `on_event_*` functions + final summary).

---

### Phase 5: REPL Enhancement

**Goal:** Make interactive mode feel like a proper chat interface.

**Design:**

1. Rich prompt: `loom (react) > ` showing current agent mode.
2. Color-coded output:
   - User input: dim
   - Assistant reply: normal/default
   - Tool results: yellow
   - Errors: red
3. Command history with ↑↓ arrow keys.
4. Multi-line input with `\` continuation.

**Implementation:**

- Add `rustyline` dependency to `cli/Cargo.toml`.
- Replace the raw `BufReader::new(stdin()).lines()` loop in `repl.rs` with `rustyline::Editor`.
- Wrap output in ANSI colors based on message type.
- Fall back to current simple implementation if `rustyline` fails to initialize (e.g., non-TTY).

**Dependency:** `rustyline = "14"` (or latest).

**Files changed:** `repl.rs`, `cli/Cargo.toml`.

---

### Phase 6: Output Verbosity Levels (Replaces --verbose)

**Goal:** Replace the binary verbose flag with a 3-level system.

**Levels:**

| Flag | stderr | Progress | State dump | Usage detail |
|---|---|---|---|---|
| `--quiet` / `-q` | none | none | none | none |
| (default) | panel format | spinner + tool summary | none | summary only |
| `--verbose` / `-v` | panel format | spinner + tool detail | structured state | prefill/decode |

**Implementation:**

- Add `Verbosity` enum: `Quiet`, `Normal`, `Verbose`.
- Replace `opts.verbose: bool` with `opts.verbosity: Verbosity` throughout.
- `--quiet` / `-q` sets `Quiet`, `--verbose` / `-v` sets `Verbose`, default is `Normal`.
- Backward compatible: `-v` still works as before.

**Files changed:** `args.rs`, `agent.rs`, `display_limits.rs`, and any file reading `opts.verbose`.

---

## Priority & Effort

| Phase | Impact | Effort | Risk |
|---|---|---|---|
| Phase 1: Progress Indicator | ★★★★★ | 1–2 days | Low |
| Phase 2: Structured Panel | ★★★★ | 1 day | Low |
| Phase 3: Thinking/Reply Separation | ★★★ | 0.5 day | Low |
| Phase 4: Unified Usage Format | ★★★ | 0.5 day | Low |
| Phase 5: REPL Enhancement | ★★★ | 1–2 days | Medium (new dep) |
| Phase 6: Verbosity Levels | ★★ | 1 day | Medium (refactor) |

Recommended order: Phase 1 → Phase 2 → Phase 4 → Phase 3 → Phase 6 → Phase 5.

Phases 1–4 can be shipped together as a single "UX v2" release. Phases 5–6 are follow-ups.

---

## Compatibility Notes

- All ANSI color output must check `isatty(stderr)` / `isatty(stdout)` and respect `NO_COLOR` env var.
- JSON mode (`--json`) is unaffected — all changes only apply to text mode.
- `--verbose` remains backward compatible in Phase 6 (still works, maps to `Verbose` level).
- Streaming chunk behavior (thinking → stderr, reply → stdout) is preserved for pipe compatibility.
