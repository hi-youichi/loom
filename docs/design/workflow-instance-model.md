# Workflow Tool Refactor Plan: Instance Model + Progressive Disclosure

**Status:** Implementation plan (not yet started)
**Scope:** `agent/tool/tool-workflow` crate (+ incidental path updates in `apps/cli`, `docs`)
**Reviewed against:**
- `agent/tool/tool-workflow/src/{tool.rs, backend.rs, event_bridge.rs, structured_output.rs, workflow_resolver.rs, json_to_lua.rs, lib.rs}`
- `agent/tool/tool-workflow/src/workflow_skill.md` + `references/*.md`
- `agent/tool/tool-workflow/tests/{terminal_events.rs, parallel_mapper.rs, builtin_skill.rs, builtin_skill_injection.rs}`
- `agent/tool/tool-core/src/tool.rs` (`BuiltinSkill`, `Tool` trait)
- Real run artifacts under `.luft/runs/luft-workflow_*` (checkpoint.json + events.jsonl samples)
- `docs/design/workflow-runtime-reliability-proposal.md` (style template)

---

## Motivation

Two problems in the current `workflow` tool design:

1. **Dump-style returns.** `run-status` packs `checkpoint.json` + the entire `events.jsonl` + the full `workflow.lua` source into one JSON payload. A non-trivial workflow produces hundreds of events and several MB of text. `list-runs` returns every past run with no pagination. `run` returns the report value verbatim with no truncation strategy beyond `ToolOutputStrategy::FileRefWithExcerpt`. The LLM caller is forced to ingest everything or nothing.

2. **Skill does not teach tool usage.** `workflow_skill.md` is entirely a Lua DSL reference (207 lines). It assumes the caller already knows *when* to use `list-runs` vs `run-status`, *how* to page through events, or *why* to prefer a summary over full events. The tool spec's action descriptions are one line each. The result: LLMs reach for `run-status` (the heaviest action) by default.

---

## Design Principles

1. **Progressive disclosure.** Every action returns the minimum the LLM needs to decide the next step. Summaries first; raw streams only on explicit follow-up.
2. **Instance as the unifying noun.** Replace `run` in the LLM-facing surface with `instance`. Internal Luft identifiers (`run_id`, `RunHandle`, `RunStatus`) stay untouched; tool-workflow owns the renaming at the serialisation boundary.
3. **Tool-workflow owns its own clean layer.** Luft writes `checkpoint.json` / `events.jsonl` / `workflow.lua` under `<instance_dir>/`. tool-workflow adds `instance.json` (curated summary) and `report.json` (structured report) in the same directory, fixing seven structural defects of the raw checkpoint without modifying the Luft crate.
4. **Path identity.** `.luft/**` → `.loom/**` for Loom-owned artefacts. Workflow scripts and instance records live under the Loom namespace, not the runtime's.
5. **Skill = decision tree first, DSL second.** The main skill file shrinks to a usage decision tree + execution model + minimal skeleton. DSL detail moves to `references/dsl-reference.md`. A new `references/tool-usage.md` documents each action with schemas, examples, and the three-step diagnostic flow.

---

## Target Surface

### Directory layout

```
<working_folder>/
└── .loom/
    ├── workflows/              # user-authored .lua scripts
    │   └── <name>.lua
    └── instances/              # one subdirectory per execution
        └── loom-instance_<unix_ts>/
            ├── instance.json      # NEW — tool-workflow's clean summary
            ├── checkpoint.json    # Luft-written (base_dir points here)
            ├── events.jsonl       # Luft-written
            ├── workflow.lua       # Luft-written script copy
            ├── report.json        # NEW — only when report() yields a non-scalar value
            └── agent-outputs/      # NEW — only when an agent output exceeds 2 KB
                └── <agent_id>.txt
```

Compatibility: on startup `list-instances` also scans `.luft/runs/` (legacy) and tags each entry `"source": "legacy"`. The legacy scan is removed in Phase 2.

### Action surface

| New action           | Old            | Purpose                                              |
| -------------------- | -------------- | ---------------------------------------------------- |
| `execute`            | `run`          | Start a new workflow instance                        |
| `list-workflows`     | (same)         | List `.lua` files under `.loom/workflows/`           |
| `list-instances`     | `list-runs`    | List past instances, paginated, filterable by status |
| `instance-summary`   | `run-status`   | One instance's curated summary (no raw events)       |
| `instance-events`    | (split off)    | Paginated raw event stream with type/agent filters   |
| `instance-source`    | (split off)    | The `workflow.lua` that an instance executed         |

### Tool input schema (full)

```json
{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": [
        "execute",
        "list-workflows",
        "list-instances",
        "instance-summary",
        "instance-events",
        "instance-source"
      ],
      "default": "execute"
    },
    "script":         { "type": "string",  "description": "(execute) Inline Lua source." },
    "workflow":       { "type": "string",  "description": "(execute) Name or path of a .lua workflow file." },
    "args":           { "type": "object",  "additionalProperties": true,
                        "description": "(execute) Exposed as _G._args inside the script." },
    "concurrency":     { "type": "integer", "minimum": 1, "maximum": 64, "default": 4 },
    "limit":          { "type": "integer", "minimum": 1, "maximum": 100, "default": 20,
                        "description": "(list-instances) Page size." },
    "cursor":         { "type": ["string","null"],
                        "description": "(list-instances) Opaque cursor from the previous page's next_cursor." },
    "status_filter":  { "type": ["string","null"], "enum": ["completed","failed","cancelled", null],
                        "description": "(list-instances) Optional status filter." },
    "instance_dir":   { "type": "string",
                        "description": "(instance-*) Instance directory name from list-instances." },
    "offset":         { "type": "integer", "minimum": 0, "default": 0,
                        "description": "(instance-events) Skip the first N matching events." },
    "events_limit":   { "type": "integer", "minimum": 1, "maximum": 500, "default": 50,
                        "description": "(instance-events) Page size." },
    "types":          { "type": ["array","null"], "items": { "type": "string" },
                        "description": "(instance-events) Filter by event type." },
    "agent_id":       { "type": ["string","null"],
                        "description": "(instance-events) Filter by agent_id." }
  }
}
```

`run_dir` is removed. `instance_dir` replaces it. `limit` is shared by `list-instances` (max 100) and `instance-events` (via the `events_limit` alias, max 500) — separate names avoid ambiguity across actions.

### Tool description (text)

```
Execute or inspect multi-agent workflows stored under .loom/.

Actions:
- execute (default): Run a workflow. Provide `script` (inline Lua) or `workflow` (name/path).
  Returns an instance summary; full report is written to .loom/instances/<dir>/report.json when large.
- list-workflows: List .lua files in .loom/workflows/.
- list-instances: List past instances (paginated). Start here when debugging.
- instance-summary: Get the curated summary of one instance — status, agents, phase spans,
  event stats. Read this BEFORE instance-events.
- instance-events: Page through the raw event stream with type/agent filters. Use after instance-summary.
- instance-source: Get the workflow.lua that an instance executed.

For the full action guide and the Lua DSL reference, load the `workflow` skill.
```

---

## Instance model (the clean layer)

### `instance.json` schema

Written by tool-workflow after every `execute` (success or failure). Schema versioned (`schema_version: 1`).

```rust
#[derive(Serialize)]
struct InstanceMeta {
    schema_version: u32,                       // = 1
    instance_id: String,                        // from checkpoint.run_id
    instance_dir: String,                       // directory name
    workflow: WorkflowRef,
    status: String,                             // completed | failed | cancelled
    created_at: u64,                            // checkpoint.created_at
    completed_at: u64,                          // checkpoint.updated_at
    total_tokens: u64,                          // checkpoint.total_tokens (Luft's sum is correct)
    total_elapsed_ms: u64,                      // SUM of agent_done.elapsed_ms across agents
    agent_count: u32,
    agents: Vec<AgentSummary>,
    phase_spans: Vec<PhaseSpan>,                // replayed from events
    event_stats: EventStats,                    // counts per event type
    report: ReportRef,                          // inline for small reports, file ref otherwise
    checkpoint_hash: String,                    // sha256 of checkpoint.json bytes (audit aid)
}

#[derive(Serialize)]
struct WorkflowRef {
    kind: &'static str,                        // "file" | "inline"
    name: Option<String>,                       // workflow argument or null for inline
    path: Option<String>,                      // resolved path or null for inline
}

#[derive(Serialize)]
struct AgentSummary {
    agent_id: String,
    phase_id: i32,
    status: String,                             // ok | error | cancelled | timed_out
    tokens: u64,
    elapsed_ms: u64,
    name: Option<String>,                       // from agent_started/agent_done events
    description: Option<String>,
    role: Option<String>,
    output_type: &'static str,                  // "json" | "text"
    output_size: u64,                           // bytes
    output_preview: String,                     // first 400 chars
    output_ref: Option<String>,                 // "agent-outputs/<aid>.txt" when size > AGENT_OUTPUT_INLINE_LIMIT
}

#[derive(Serialize)]
struct PhaseSpan {
    span_id: i64,
    name: String,
    parent_id: Option<i64>,
    depth: u32,
    planned: u64,                               // from phase_span_started.planned
    started_at: Option<String>,                 // ISO 8601 from the event ts
    ended_at: Option<String>,
}

#[derive(Serialize)]
struct EventStats {
    total: u64,
    by_type: BTreeMap<String, u64>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ReportRef {
    Inline(Value),                              // when serialised size <= REPORT_INLINE_LIMIT
    File { r#ref: String, preview: String, value_type: String, size_bytes: u64 },
    Empty,
}
```

### Constants

```rust
const REPORT_INLINE_LIMIT: usize = 800;        // bytes (UTF-8)
const AGENT_OUTPUT_INLINE_LIMIT: usize = 2048;  // bytes (UTF-8)
const SOURCE_INLINE_LIMIT: usize = 32_768;      // bytes (UTF-8)
const DEFAULT_LIST_LIMIT: usize = 20;
const MAX_LIST_LIMIT: usize = 100;
const DEFAULT_EVENTS_LIMIT: usize = 50;
const MAX_EVENTS_LIMIT: usize = 500;
```

### `report.json`

Written only when `status == "completed"` and `report()` yielded a value whose `serde_json::to_string` length exceeds `REPORT_INLINE_LIMIT`. Contains the pure pretty-printed JSON of the report value. `InstanceMeta.report` carries `ReportRef::File { ref, preview, value_type, size_bytes }` so the LLM knows where to find the full content and what type it is.

### `agent-outputs/<agent_id>.txt`

Written per agent whose `output` string exceeds `AGENT_OUTPUT_INLINE_LIMIT`. `AgentSummary.output_preview` still carries the first 400 chars for at-a-glance inspection; `output_ref` points at the file.

### Relationship to the raw checkpoint

`instance.json` is built `from(checkpoint, events, workflow_src)`:

| Raw checkpoint field      | instance.json handling                                          |
| ------------------------- | --------------------------------------------------------------- |
| `run_id`                  | renamed to `instance_id`                                        |
| `task` (`"luft workflow"`) | dropped; `workflow` field carries real name/path               |
| `status`                  | passed through (lowercase)                                       |
| `current_phase`           | dropped (always 0 on completion, not meaningful)                |
| `completed_phases` (empty)| replaced by `phase_spans` replayed from events                  |
| `agent_results[].output`  | classified (json/text), previewed, optionally file-backed        |
| `agent_results[].findings`| dropped from the summary (per-agent findings are not consumed)   |
| `findings` (workflow-level)| dropped (always empty in observed runs)                         |
| `total_tokens`            | passed through and cross-checked against `agent_results` sum     |
| `created_at`/`updated_at` | renamed to `created_at`/`completed_at`                           |
| `completed_spans` (empty) | replaced by `phase_spans` from events                            |
| `workflow_meta` (null)    | parsed out of `workflow.lua` header when possible; null on miss |
| `started_agent_ids`       | dropped; `agents[]` ordered by `agent_started` event order      |

`report` is extracted from the `run_done` event in `events.jsonl`, *not* the checkpoint (the checkpoint never carries it). `total_elapsed_ms` is the sum of `agent_done.elapsed_ms` values (the checkpoint has none). `checkpoint_hash` is `sha256(checkpoint.json)` for audit/diffing.

### Legacy compatibility

When an `instance_dir` under `.luft/runs/` is queried and has `checkpoint.json` + `events.jsonl` but **no** `instance.json`, tool-workflow executes the same `build_instance_meta` pass on-the-fly and caches the result by writing `instance.json` into the legacy directory. `list-instances` tags these as `"source": "legacy"` until the next minor removes the legacy scan.

---

## Action specifications

### `execute`

**Params:** `script` | `workflow`, `args?`, `concurrency?`.

**Behaviour:**
1. Validate nesting depth (`ctx.depth < 3`).
2. Resolve `workflow` path (or use `script`). Inject `_G._args`.
3. `luft_runtime::validate_workflow(&lua_source)`.
4. Pick the instance directory name: `loom-instance_<unix_ts>`. Create it under `.loom/instances/`.
5. Build `LoomAgentBackend`, `LuftBuilder::base_dir(&instance_dir)`, `start_script`.
6. Run the event loop (existing logic — track in-flight, forward events, bridge cancellation).
7. After `run_handle.join()`: read `checkpoint.json` + `events.jsonl` + `workflow.lua` from `instance_dir`, build `InstanceMeta`, write `instance.json`. Optionally write `report.json` + `agent-outputs/`.
8. Return a compact JSON:

```json
{
  "instance_id": "019f51cc-...",
  "instance_dir": "loom-instance_1783783769",
  "status": "completed",
  "workflow": { "kind": "file", "name": "refactor", "path": "src/refactor.lua" },
  "agent_count": 3,
  "total_tokens": 41181,
  "total_elapsed_ms": 19000,
  "report_ref": "instance/report.json",
  "report_preview": "<first 800 chars>"
}
```

`report_ref` is `null` when the report was inlined. `report_preview` is the first 800 chars of the serialised report (or `null` on failure/cancellation). Errors during join or channel closure follow the existing `ToolError` / `InvalidInput` distinctions unchanged.

### `list-workflows`

Unchanged except `directory` path → `.loom/workflows`.

### `list-instances`

**Params:** `limit?`, `cursor?`, `status_filter?`.

**Behaviour:**
- Enumerate `instance_dir` entries under `.loom/instances/` **and** `.luft/runs/` (legacy).
- For each, prefer reading `instance.json` (O(1) summary). For legacy entries without `instance.json`, read `checkpoint.json` (existing path) and synthesise the minimal fields.
- Sort by `created_at` descending.
- Apply `status_filter` if provided.
- Implement cursor pagination: key = `(created_at, instance_dir)`. The cursor is the `instance_dir` of the last item on the current page; the next page scans entries strictly less than that key. `next_cursor` is `null` when the page was the last.
- Cap `limit` to `[1, 100]`, default `20`.

**Return:**

```json
{
  "instances": [
    {
      "instance_id": "...",
      "instance_dir": "loom-instance_<ts>",
      "status": "completed",
      "workflow": { "kind": "file", "name": "refactor" },
      "created_at": 1783783769,
      "completed_at": 1783783772,
      "total_tokens": 13400,
      "agent_count": 1,
      "source": "current"
    }
  ],
  "count": 20,
  "next_cursor": "loom-instance_<ts>" | null
}
```

### `instance-summary`

**Params:** `instance_dir` (required).

**Behaviour:** Read `instance.json`. If absent, build it on the fly from `checkpoint.json` + `events.jsonl` (legacy upgrade path) and persist.

**Return:** the `InstanceMeta` payload verbatim (without raw events).

### `instance-events`

**Params:** `instance_dir` (required), `offset?`, `events_limit?`, `types?`, `agent_id?`.

**Behaviour:**
- Stream-read `events.jsonl` line by line.
- Apply filters cumulatively: `types` (set membership), `agent_id` (string equality).
- Skip `offset` matching events, return at most `events_limit` (capped at 500).
- `total_matching` = the full filtered count (needs one pass over the file; file sizes are modest, this is acceptable). `next_offset` = `offset + returned_count` when more remain, else `null`.

**Return:**

```json
{
  "instance_dir": "...",
  "offset": 0,
  "events_limit": 50,
  "total_matching": 312,
  "next_offset": 50,
  "events": [ { /* one parsed JSONL object */ }, ... ]
}
```

### `instance-source`

**Params:** `instance_dir` (required).

**Behaviour:** Read `workflow.lua`. If size ≤ `SOURCE_INLINE_LIMIT`, inline. Otherwise return `source_ref` + `source_preview` (first 4 KB).

**Return:**

```json
{
  "instance_dir": "...",
  "workflow": { "kind": "file", "name": "refactor", "path": "..." },
  "source": "<full Lua text>",
  "source_ref": null,
  "source_preview": null
}
```

or, when over the limit:

```json
{
  "instance_dir": "...",
  "workflow": { ... },
  "source": null,
  "source_ref": "instance/workflow.lua",
  "source_preview": "<first 4 KB>"
}
```

---

## Skill restructure

### `workflow_skill.md` (main, ~80 lines)

Three sections only:

**§1 When to use which action** — a decision table mapping intent → action, one-line rationale, minimum args example. Roughly:

```
| Intent                                          | Action            | Minimum args                                      |
|------------------------------------------------|-------------------|---------------------------------------------------|
| Run a new multi-agent task                     | execute           | {workflow:"refactor"} or {script:"..."}           |
| See what scripts exist                         | list-workflows    | {}                                                |
| Review past executions / debug                 | list-instances    | {limit:20}; filter {status_filter:"failed"}      |
| Inspect one execution (always start here)      | instance-summary | {instance_dir:"loom-instance_<ts>"}               |
| Drill into the event timeline                  | instance-events  | {instance_dir, types:["agent_done","run_done"]}  |
| Read the script that ran                       | instance-source  | {instance_dir}                                    |
```

Plus a one-paragraph "progressive disclosure" rule: always `list-instances` → `instance-summary` → `instance-events` in that order; never jump to `instance-events` first.

**§2 Execution model** — the existing two paragraphs ("pure orchestrator", "sandbox disables io/os/require", "report() first-call-wins") kept verbatim.

**§3 Minimal skeleton** — the existing skeleton block kept verbatim.

Closing line:

> For the full Lua DSL reference (primitives, schema rules, error handling), see `references/dsl-reference.md`. For per-action schemas, return shapes, and the three-step diagnostic flow, see `references/tool-usage.md`. Load these via the `skill` tool when needed.

### `references/dsl-reference.md` (new, ~150 lines)

Cut-and-paste of the existing Primitives section (lines 47–182 of the current `workflow_skill.md`): `agent`, `parallel`, `pipeline`, `phase`, `phase_begin`/`phase_end`, `workflow`, `report`, `log`, `budget`, `json.encode/decode`, Globals, Error Handling, Rules. Content unchanged, only the file location moves.

### `references/tool-usage.md` (new, ~170 lines)

One section per action with: full params table, return shape table, one example args JSON, one example return snippet. Plus two dedicated sections:

**Three-step diagnostic flow** — the canonical recipe for investigating a failed workflow:
1. `list-instances` with `{status_filter:"failed"}` → pick the most recent `instance_dir`.
2. `instance-summary` → check `status`, `agents[].status`, `event_stats.by_type` for anomalies (e.g. `agent_done` with `status:"error"`).
3. `instance-events` with `{types:["agent_done","run_done","parallel_done"]}` → inspect the failure points; widen `types` only if needed.

**Big payloads** — explains when `report_ref`, `output_ref`, and `source_ref` appear and how to fetch the full content with the `read` tool using the path `.loom/instances/<instance_dir>/<ref>`.

### `references/` ordering (updated in `BuiltinSkill::references`)

```
references/
├── tool-usage.md              # NEW (first — usage beats DSL when debugging)
├── dsl-reference.md           # NEW (moved from main file)
├── architecture-header.md     # unchanged
├── agent-prompts.md           # unchanged
├── task-decomposition.md      # unchanged
├── adversarial-verification.md# unchanged
└── examples.md                # unchanged
```

### Skill frontmatter

```yaml
triggers:
  - workflow
  - multi-agent
  - lua script
  - list-instances
  - instance-summary
  - debug workflow
  - workflow failed
  - workflow status
tags:
  - workflow
  - orchestration
  - lua
  - instance
```

`requires_tools: [workflow]` unchanged.

---

## File-by-file change list

### `tool-workflow/src/tool.rs`

1. Add `mod instance;` — new module hosting `InstanceMeta`, `AgentSummary`, `PhaseSpan`, `EventStats`, `ReportRef`, `WorkflowRef`, and `build_instance_meta(checkpoint: &Value, events: &[Value], workflow_src: Option<&str>, workflow_ref: &WorkflowRef) -> InstanceMeta`. Plus `write_instance_artifacts(dir: &Path, meta: &InstanceMeta, report_value: Option<&Value>) -> Result<(), io::Error>`.
2. Replace `runs_dir()` with `instances_dir()` returning `.loom/instances`. Add `workflows_dir()` update to `.loom/workflows`. `WorkflowTool::{workflows_dir, instances_dir}` use `config_template.working_folder`.
3. `handle_run` renamed `handle_execute`. At the end, after `run_handle.join()`, call `build_instance_meta` + `write_instance_artifacts` before returning. The returned JSON uses `instance_id`/`instance_dir`/`report_ref`/`report_preview` keys.
4. `handle_list_runs` renamed `handle_list_instances` — add `limit`/`cursor`/`status_filter` parsing, dual-scan (`.loom/instances` + `.luft/runs/`), tag `source`.
5. `handle_run_status` renamed `handle_instance_summary` — read `instance.json` (build on the fly if missing).
6. Add `handle_instance_events` — streaming JSONL parse with filters.
7. Add `handle_instance_source`.
8. `call()` dispatch table updated to the six new action names.
9. `spec()` description + input_schema rewritten per the schemas above.
10. `builtin_skill()` returns updated `references` Vec (7 entries) and triggers via the embedded `WORKFLOW_SKILL` content (which is the new thin file).
11. `include_str!` constants — add `REF_TOOL_USAGE`, `REF_DSL_REFERENCE`. Remove the inlined DSL from `WORKFLOW_SKILL`.
12. Naming constants: add `REPORT_INLINE_LIMIT`, `AGENT_OUTPUT_INLINE_LIMIT`, `SOURCE_INLINE_LIMIT`, `DEFAULT_LIST_LIMIT`, `MAX_LIST_LIMIT`, `DEFAULT_EVENTS_LIMIT`, `MAX_EVENTS_LIMIT`, `INSTANCE_DIR_PREFIX = "loom-instance_"`.

### `tool-workflow/src/workflow_resolver.rs`

- Search paths: `.luft/workflows` → `.loom/workflows`; `~/.config/luft/workflows` → `~/.config/loom/workflows`.
- Keep the absolute path + literal path fallbacks unchanged.
- Add a migration helper `migrate_legacy_workflows()` invoked once on first `WorkflowTool::new` (logs a warning, does not auto-move files — only suggests).

### `tool-workflow/src/lib.rs`

- No public API change. `default_workflow_tool_provider` returns `WorkflowTool::new(config)` as before.

### `tool-workflow/src/workflow_skill.md`

Replaced with the thin ~80-line file described in the Skill restructure section.

### `tool-workflow/src/references/tool-usage.md`

New file.

### `tool-workflow/src/references/dsl-reference.md`

New file (content moved from `workflow_skill.md`).

### `tool-workflow/src/instance.rs`

New module. Contains the structs + `build_instance_meta` + `write_instance_artifacts` + helpers (`parse_workflow_meta_from_lua`, `replay_phase_spans`, `classify_output`, `summarise_event_types`). Unit-tested in isolation.

### `tool-workflow/tests/`

| Existing test file         | Change                                                                        |
| -------------------------- | ----------------------------------------------------------------------------- |
| `terminal_events.rs`       | Update assertions to the new `action` names and `instance_dir` field.        |
| `parallel_mapper.rs`       | Same rename pass.                                                            |
| `builtin_skill.rs`         | Assert the new `references` Vec length (7) and the new frontmatter triggers.|
| `builtin_skill_injection.rs` | Update the substring assertions for the thinner main skill body.            |

New test files:

| Test file                       | Cases                                                                                          |
| ------------------------------- | ---------------------------------------------------------------------------------------------- |
| `instances_path.rs`             | `path_uses_dot_loom_not_dot_luft`, `workflows_dir_is_dot_loom_workflows`, `instances_dir_is_dot_loom_instances` |
| `execute_artifacts.rs`           | `execute_writes_instance_json`, `execute_big_report_to_file`, `agent_output_big_to_file`, `checkpoint_hash_matches` |
| `instance_summary.rs`           | `summary_excludes_raw_events`, `summary_legacy_builds_on_the_fly`, `phase_spans_replayed_from_events` |
| `instance_events.rs`            | `events_pagination_offset`, `events_type_filter`, `events_agent_filter`, `events_limit_clamped_to_500` |
| `instance_source.rs`            | `source_inlined_when_small`, `source_ref_when_over_32kb`                                       |
| `list_instances.rs`             | `list_default_limit_20`, `list_cursor_next_page`, `list_status_filter_failed`, `list_legacy_source_tag` |

### `apps/cli/src/`

Audit for any hard-coded `.luft` references (paths, completion suggestions, TUI labels). Replace with `.loom`. Update any CLI flag names that mention `run` to mention `instance` only if they are user-facing; internal flag aliases keep backwards compatibility with a deprecation warning.

### `docs/`

- New file: `docs/design/workflow-instance-model.md` (this document, committed).
- Update `docs/design/workflow-runtime-reliability-proposal.md` cross-reference to point at the instance model as the persistence substrate for checkpoint/resume work.
- Changelog entry under `docs/changelog/` (create directory if absent).

---

## Implementation order

The plan is split into eight steps. Each step is independently committable and leaves the crate building and tests green. Do not batch steps.

### Step 1 — Path migration (low risk, no behaviour change)

**Scope:** `.loom` paths + resolver.

**Files:**
- `tool.rs`: `workflows_dir()`, `instances_dir()` (renamed from `runs_dir()`), all call sites.
- `workflow_resolver.rs`: search path update.
- `tests/*`: path assertions.

**Acceptance:**
- `cargo test -p tool-workflow` green.
- No string `.luft` remains in `tool-workflow/src` (grep check).
- `list-workflows` reads from `.loom/workflows`; `list-instances` (still named `list-runs` at this step pending Step 2) reads from `.loom/instances` with legacy fallback.

### Step 2 — Action rename + param `instance_dir`

**Scope:** rename `action` enum values and the `run_dir` parameter; no new actions yet, no return shape changes.

**Files:** `tool.rs` (dispatch table, spec schema, description), tests.

**Acceptance:**
- `action: "execute"` replaces `"run"`; `action: "list-instances"` replaces `"list-runs"`.
- `instance_dir` parameter replaces `run_dir` in `instance-summary` (formerly `run-status`).
- Legacy action names accepted with a deprecation warning (return `ToolError` with a message pointing at the new name) for one minor cycle — keeps external callers operational.
- All tests updated; green.

### Step 3 — The `instance` module

**Scope:** add `instance.rs` with `InstanceMeta` and `build_instance_meta` + `write_instance_artifacts`. Pure logic, no integration into `handle_execute` yet.

**Files:** `instance.rs` (new), `tool.rs` (`mod instance;`), tests for the module.

**Acceptance:**
- `build_instance_meta` consumes a real `checkpoint.json` + `events.jsonl` pair (use the `.luft/runs/luft-workflow_1783783769` artefacts as a fixture) and returns the expected struct.
- `write_instance_artifacts` writes `instance.json` (+ `report.json`, `agent-outputs/` when applicable) to a temp dir; round-trips through `serde_json`.
- Unit tests for `parse_workflow_meta_from_lua`, `replay_phase_spans`, `classify_output`, `summarise_event_types`.

### Step 4 — Wire `execute` to write `instance.json`

**Scope:** `handle_execute` calls `build_instance_meta` + `write_instance_artifacts` after `run_handle.join()`. Return shape updated to the compact JSON.

**Files:** `tool.rs`, `execute_artifacts.rs` test.

**Acceptance:**
- After a successful `execute`, the instance directory contains `instance.json` and (when applicable) `report.json` + `agent-outputs/`.
- The returned JSON has `instance_id`, `instance_dir`, `status`, `workflow`, `agent_count`, `total_tokens`, `total_elapsed_ms`, `report_ref`, `report_preview`.
- Failed `execute` also writes `instance.json` with `status:"failed"`.
- `cargo test` green.

### Step 5 — `list-instances` pagination + status filter + legacy tag

**Scope:** rewrite `handle_list_instances` with `limit` / `cursor` / `status_filter`.

**Files:** `tool.rs`, `list_instances.rs` test.

**Acceptance:**
- Default page size 20; capped at 100.
- `next_cursor` is `null` on the last page, otherwise the last entry's `instance_dir`.
- `status_filter:"failed"` returns only failed instances.
- Legacy `.luft/runs/` entries appear with `"source":"legacy"`.

### Step 6 — `instance-summary`, `instance-events`, `instance-source`

**Scope:** wire the three new handlers.

**Files:** `tool.rs`, test files.

**Acceptance:**
- `instance-summary` returns `InstanceMeta` without raw events.
- `instance-events` paginates and filters by `types` / `agent_id`; `events_limit` clamped to 500; `total_matching` correct; `next_offset` correct.
- `instance-source` inlines when ≤ 32 KB, refs otherwise.
- Legacy `instance_dir` queried via `instance-summary` triggers on-the-fly `instance.json` generation.
- All tests green.

### Step 7 — Skill restructure

**Scope:** rewrite `workflow_skill.md`, add `references/tool-usage.md` + `references/dsl-reference.md`, update `builtin_skill()` in `tool.rs`.

**Files:**
- `workflow_skill.md` (replace).
- `references/tool-usage.md` (new).
- `references/dsl-reference.md` (new).
- `tool.rs`: `include_str!` constants + `references` Vec + `triggers` in the embedded frontmatter.
- `tests/builtin_skill.rs`, `tests/builtin_skill_injection.rs`.

**Acceptance:**
- `builtin_skill.rs` asserts `references.len() == 7` and the new ordering.
- Main skill body ≤ 100 lines.
- `builtin_skill_injection.rs` asserts the agent system prompt contains the §1 decision table (or a stable substring) and does NOT contain the full DSL primitive tables (they now live in `dsl-reference.md`).
- Triggers include `list-instances`, `instance-summary`, `debug workflow`.

### Step 8 — Docs + CLI audit + changelog

**Scope:** cross-references, CLI hard-coded paths, changelog.

**Files:**
- `docs/design/workflow-instance-model.md` (commit this very document).
- `apps/cli/src/` (grep + replace `.luft` → `.loom`, label strings).
- `docs/changelog/instance-model.md` (new).

**Acceptance:**
- `rg "\.luft" apps/` returns no hits outside `docs/` and `tests/` legacy fixtures.
- Changelog entry present.
- CI green.

---

## Out of scope

- **Luft contract changes.** No `luft_core` or `luft` crate edits. The `run_id` / `RunStatus` / `RunHandle` names persist inside Luft. The `total_elapsed_ms` aggregation and `workflow_meta` parsing are done in tool-workflow from events, not requested from Luft.
- **Resume / checkpoint replay.** This plan produces `instance.json` as a curated read-only summary. It does not implement resume-from-checkpoint; that belongs to the reliability proposal and depends on Luft-side checkpoint primitives.
- **Cross-process workspace locking.** Mentioned in the reliability proposal; not touched here.
- **Workflow file migration automation.** Users move `.luft/workflows/*.lua` → `.loom/workflows/` by hand. The resolver does not auto-migrate.
- **`agent-outputs/` retrieval action.** The LLM fetches full agent outputs via the existing `read` tool using the path in `output_ref`; no dedicated action is added.
- **TUI rendering changes.** The CLI/TUI consumes the stream events unchanged; the instance model is a tool-level artefact, not a new event type.

---

## Risks and mitigations

| Risk                                                              | Mitigation                                                                                                                                                                                              |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Luft `LuftBuilder` does not expose a custom `run_dir` name pattern | tool-workflow pre-creates `loom-instance_<ts>` under `.loom/instances/` and passes it as `base_dir`. Luft writes its three files inside that directory regardless of the prefix it would have chosen.   |
| Legacy `.luft/runs/` entries lack `instance.json`                  | `instance-summary` builds one on the fly and persists it; subsequent queries are O(1). `list-instances` tolerates the absence and synthesises the minimal fields from `checkpoint.json`.               |
| `events.jsonl` is large and `total_matching` requires a full pass  | File sizes are bounded by workflow duration; a single sequential read is acceptable. `instance-events` opens the file once for counting and once for paging, or holds a buffered reader and rewinds — the simpler single-pass + offset approach is fine. |
| `report()` value exchanged between Lua and Rust is a Lua table     | Luft serialises `report` as `serde_json::Value` in the `run_done` event already (confirmed in the `luft-workflow_1783784203` sample where `report: null` appears). tool-workflow decodes it directly.    |
| Breaking callers that pass `action:"run"`                           | Step 2 keeps a deprecation alias: an `action:"run"` request is accepted, mapped to `execute`, and the response carries a `deprecation: "run is now execute"` field for one minor cycle. Removed in Phase 2. |
| Skill body too thin, LLM does not load references                  | The §1 decision table fits in the main file and is sufficient for action selection. DSL writing still works because the skeleton + execution model remain. The `triggers` expansion ensures the skill loads on debugging intents. |

---

## Phase 2 (deferred)

- Remove legacy `.luft/runs/` scan in `list-instances`.
- Remove `action:"run"` / `action:"list-runs"` / `action:"run-status"` deprecation aliases.
- Remove the `"source"` field from `list-instances` entries.
- Pin `schema_version` and document the migration path for future bumps.

## Phase 3 (when Luft lands contract support)

- Push `phase_spans`, `total_elapsed_ms`, `workflow_meta` replay into Luft's own checkpoint writer; `instance.json` becomes a thin renaming view over a richer checkpoint.
- Coordinate `LuftFailureKind` in the reliability proposal with the `agents[].status` field of `InstanceMeta` so failure classifications are consistent.

---

## Tool split — 6 specialised tools (supersedes the action-dispatched `workflow` design above)

The original design above envisioned one tool that dispatched on an `action`
enum. After Phase 1 review, the LLM-facing surface was split into six
specialised tools so each one carries a focused schema, a focused
description, and a focused output hint. The public `WorkflowTool` and
its action dispatcher are removed.

### Tool naming

| New tool name         | Replaces action  | Runs in foreground? |
| --------------------- | ---------------- | ------------------- |
| `workflow_start`      | `execute`        | No — returns immediately with `{ instance_dir, status: "running" }` |
| `workflow_status`     | `instance-summary` (also slow-path rebuild from `checkpoint.json`) | No |
| `workflow_list`       | `list-instances` | No |
| `workflow_events`     | `instance-events` | No |
| `workflow_source`     | `instance-source` | No |
| `workflow_files`      | (new)            | No |

`workflow_files` provides workflow discovery through a dedicated tool;
workflow definitions are loaded by name or path inside `workflow_start`.

### Background `workflow_start`

After `start_script` succeeds, the foreground handler does **not** wait
for the run to terminate. It:

1. Captures an owned copy of the runtime + config into a closure.
2. Spawns `tokio::spawn(background_finalize(...))` which waits for
   `RunDone`, drains the run handle, and finalises `instance.json` /
   `report.json` / `agent-outputs/`.
3. Returns `{ instance_dir, status: "running" }` to the caller.

The caller polls with `workflow_status`. To wait a few seconds before
polling, run a shell tool with `sleep 5` (or PowerShell
`Start-Sleep -Seconds 5`) and then call `workflow_status`.

### `workflow_status` lookup order

`workflow_status` does **not** read or write `status.json`. Lookup order:

1. `.loom/instances/<dir>/instance.json` → return the sanitised
   terminal summary.
2. `checkpoint.json` only (no `instance.json`) → rebuild an in-memory
   `InstanceMeta` from raw artefacts and return it (slow path; no
   write-back).
3. `.loom/instances/<dir>/` exists with neither → return
   `{ instance_dir, status: "running" }`.
4. `.luft/runs/<dir>/` exists with no `checkpoint.json` → return
   an incomplete-instance error.

### Public sanitisation

`workflow_status` strips internal file references from the
`InstanceMeta` before returning it. The following fields are removed:

- `workflow.path`
- `agents[].output_ref`
- `report.ref` (file-backed reports keep `preview`)
- `checkpoint_hash`

Error messages never expose absolute filesystem paths (e.g.
`"not found"` rather than `/Users/me/.loom/instances/foo`). The same
sanitisation rules are mirrored by the public normaliser
`sanitize_instance_for_public`.

### Output hints

All six tools use `ToolOutputStrategy::Inline` so the output
normaliser never falls back to `FileRefWithExcerpt` for workflow
payloads.

### Backwards compatibility

This is a breaking change for the workflow tool surface:

- `WorkflowTool` and the action dispatcher are removed.
- `run`, `list-runs`, and `run-status` aliases are removed.
- `register_workflow_tool` becomes `register_workflow_tools`.
- `default_workflow_tool_provider` keeps its name and signature and now
  returns all six specialised tools.

### Builtin skill wiring

Only `WorkflowStartTool` exposes the builtin `workflow` skill (the
others' `builtin_skill()` returns `None`). The skill's
`requires_tools` is updated to `["workflow_start", "workflow_status"]`,
and the skill body now opens with the six-tool decision table from
`workflow_skill.md §1`.

---

## Acceptance checklist (end of plan)

- [x] `cargo test -p tool-workflow` green with the migrated test files.
- [ ] `cargo clippy -p tool-workflow --all-targets -- -D warnings` clean; currently blocked by the existing `agent/agent-core/src/run/types.rs:51` doc lint.
- [x] Public workflow responses do not expose filesystem references.
- [x] `workflow_start` returns immediately and `workflow_status` supports running/terminal states.
- [ ] Manual run: start a small workflow, wait with shell sleep, poll `workflow_status`, then inspect events/source through their tools.
- [ ] Changelog entry committed.