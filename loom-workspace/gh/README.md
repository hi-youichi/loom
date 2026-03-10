# gh

GitHub webhook handling and issue API for the Loom agent (see workspace `loom` crate).

This crate lets you:

- Receive and verify GitHub **webhooks** (issues event)
- Parse payloads into typed structs
- Call GitHub **issue API** (comments, close, labels) via [octocrab](https://github.com/XAMPPRocky/octocrab)
- Build **Loom `RunOptions`** from an `IssuesEvent` and spawn (or inject) agent runs

It is used by the `gh-webhook` binary and by workflows that trigger the Loom agent from GitHub issue events.

---

## Library

### Webhook

- **Signature verification**: `verify_signature(secret, body, x_hub_signature_256)` — constant-time HMAC-SHA256 check.
- **Payload types**: `IssuesEvent`, `IssuePayload`, `RepoRef`, `LabelPayload`, `SenderPayload`.
- **Parsing**: `parse_issues_event(json)` → `Result<IssuesEvent, WebhookError>`.

### Issue API (octocrab)

- `octocrab_from_token(token)` — build `Octocrab` from a PAT (e.g. `GITHUB_TOKEN`).
- `create_comment`, `close_issue`, `add_labels` — by `(owner, repo, issue_number)`.
- `IssuesEvent` also has convenience methods: `.create_comment(crab, body)`, `.close_issue(crab)`, `.add_labels(crab, labels)`.

### Loom agent integration

- **`run_options_from_issues_event(ev, delivery_id)`** — builds `loom::RunOptions` from an issues webhook (message from action/repo/issue/title/body; `thread_id` from delivery id or `issue-{owner/repo}-{number}`; optional `working_folder` / `model` from env).
- **`webhook_router(secret, run_agent)`** — Axum `Router` with `POST /webhook`. On valid `issues` event it returns 200 and either calls `run_agent(RunOptions)` (e.g. in tests) or spawns a real Loom agent run.
- **`spawn_agent_run(opts)`** — spawns `loom::run_agent_with_options(opts, RunCmd::React, None)` in a background task.

### Example (library)

```rust
use gh::{
    parse_issues_event, verify_signature, webhook_router,
    octocrab_from_token, run_options_from_issues_event,
};

// Verify and parse
let sig = headers.get("x-hub-signature-256").and_then(|v| v.to_str().ok()).unwrap_or("");
if !verify_signature(secret.as_bytes(), body, sig) {
    return Err("invalid signature");
}
let ev = parse_issues_event(body)?;

// Build Loom options and run (e.g. in your own server)
let opts = run_options_from_issues_event(&ev, delivery_id);
gh::spawn_agent_run(opts);

// Or use the built-in router (production: run_agent = None)
let app = webhook_router(secret, None);
// axum::serve(listener, app).await?;
```

---

## Binary: gh-webhook

Standalone HTTP server that listens for GitHub webhooks and, on valid **issues** events, spawns a Loom agent run.

```bash
cargo run -p gh --bin gh-webhook
```

**Options** (CLI overrides environment variables)

| Option | Short | Env fallback | Default | Description |
|--------|-------|--------------|---------|-------------|
| `--secret` | `-s` | `GITHUB_WEBHOOK_SECRET` | — | Webhook secret for `X-Hub-Signature-256`. If unset (and env unset), verification will fail. |
| `--port` | `-p` | `GH_WEBHOOK_PORT` | `8080` | Port to listen on. |
| `--bind` | `-b` | `GH_WEBHOOK_BIND` | `0.0.0.0` | Bind address (e.g. `127.0.0.1` for local only). |

Examples:

```bash
gh-webhook --port 9000
gh-webhook -b 127.0.0.1 -p 8080
gh-webhook --secret "$GITHUB_WEBHOOK_SECRET"
```

Use `gh-webhook --help` for full help.

**Environment** (used when option not passed)

| Variable | Description |
|----------|-------------|
| `GITHUB_WEBHOOK_SECRET` | Webhook secret (prefer env in production to avoid shell history). |
| `GH_WEBHOOK_PORT` | Port (default 8080). |
| `GH_WEBHOOK_BIND` | Bind address (default 0.0.0.0). |
| `GITHUB_TOKEN` | Not used by the server; set if Loom or other tools need it for GitHub API. |
| `WORKING_FOLDER` | Passed to Loom `RunOptions` as working directory. |
| `MODEL` / `OPENAI_MODEL` | Passed to Loom for model selection. |

**Endpoint**

- `POST /webhook` — expects GitHub delivery with headers:
  - `X-Hub-Signature-256`: HMAC-SHA256 signature
  - `X-Github-Event`: must be `issues` to trigger agent
  - `X-Github-Delivery`: optional delivery ID (used as `thread_id` when present)

Other events are ignored with 200. Invalid signature or bad payload returns 401 or 400.

---

## Tests

```bash
cargo test -p gh
```

Integration tests cover signature verification, payload parsing, and the webhook HTTP handler (with an optional `run_agent` callback to capture `RunOptions` without running the real agent).
