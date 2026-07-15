# Workflow Instance Model — Loom Worktree Task Plan

**Source plan:** [`workflow-instance-model.md`](./workflow-instance-model.md)
**Purpose:** Split the refactor into many small, self-contained specs that Loom
can execute in isolated git worktrees. Each spec below is copy-pasteable as the
task prompt for `loom --worktree -m "<spec>" -w C:\Users\heycj\dev\loom`.

## How to use this document

1. Read the dependency graph and the batch plan first.
2. For each task, copy the **Loom prompt** block verbatim into a Loom session
   started in a fresh worktree.
3. Each task ends with a strict acceptance block — Loom must satisfy every item
   before claiming completion.
4. Merge order is enforced by the dependencies; do not skip ahead.

## Branch naming convention

Each task owns branch `wf/instance-T<NN>-<slug>` where `<NN>` is the task
number in this document. Worktrees should be created by Loom's
`--worktree` flag (it manages the branch automatically) or manually via
`git worktree add ../loom-T<NN> -b wf/instance-T<NN>-<slug>`.

## Dependency graph

```
T-01 instance.rs module ......... [independent]
T-08a skill md (3 files) ......... [independent]
T-09a docs commit + changelog .... [independent]

T-02 paths + action rename ........ [depends on: nothing, but conflicts with
                                       any task that edits tool.rs → start before
                                       batch 3]

T-03 execute wiring ............... [depends on: T-01, T-02]
T-04 list-instances pagination .... [depends on: T-02]
T-05 instance-summary handler ..... [depends on: T-01, T-02]
T-06 instance-events handler ...... [depends on: T-02]
T-07 instance-source handler ...... [depends on: T-02]
T-08b tool.rs builtin_skill wiring. [depends on: T-02, T-08a]

T-09b cli audit + final acceptance . [depends on: all above merged]
```

**File-conflict warning (read before scheduling):**
`Batch 3` tasks (T-03 through T-07 + T-08b) all edit
`agent/tool/tool-workflow/src/tool.rs`. They cannot run concurrently in separate
worktrees without producing hard merge conflicts. The recommended strategy is
serial execution within one worktree per task, **merging back to `main` after
each task** and **rebasing the next task's worktree onto the updated `main`**
before starting. See "Scheduling" below for the concrete pipeline.

## Scheduling

### Batch 1 — run concurrently (3 worktrees, no file overlap)

| Task  | Branch                          | Files touched (new only)                                       |
| ----- | ------------------------------- | ------------------------------------------------------------- |
| T-01  | `wf/instance-T01-instance-mod`  | `agent/tool/tool-workflow/src/instance.rs` (+1 line in `lib.rs`) |
| T-08a | `wf/instance-T08a-skill-md`     | `workflow_skill.md`, `references/tool-usage.md`, `references/dsl-reference.md` |
| T-09a | `wf/instance-T09a-docs`          | `docs/design/workflow-instance-model.md`, `docs/changelog/instance-model.md` |

### Batch 2 — single worktree (must finish before Batch 3)

| Task | Branch                       | Files touched                     |
| ---- | ---------------------------- | --------------------------------- |
| T-02 | `wf/instance-T02-paths-actions` | `tool.rs`, `workflow_resolver.rs`, existing tests |

### Batch 3 — serial (1 worktree at a time, rebase between tasks)

Recommended serial order to minimise merge friction:

1. T-08b  (small, isolated constants + `builtin_skill()` Vec)
2. T-07   (read-only handler, lowest risk)
3. T-06   (events handler, isolated filters)
4. T-05   (summary handler, depends on instance.rs from T-01)
5. T-04   (list-instances pagination)
6. T-03   (execute wiring, touches most-used path)

### Batch 4 — final sweep

| Task  | Branch                          | Files touched                          |
| ----- | ------------------------------- | -------------------------------------- |
| T-09b | `wf/instance-T09b-cli-audit`    | `apps/cli/src/*`, `docs/changelog/*`   |

---

# Task T-01 — instance.rs clean-layer module

**Branch:** `wf/instance-T01-instance-mod`
**Depends on:** nothing.

## Loom prompt

```
You are working in your own git worktree on branch
`wf/instance-T01-instance-mod`. Your only goal is to add the clean-layer
instance module to the tool-workflow crate. Follow the spec in
docs/design/workflow-instance-model.md §Instance model exactly.

Do NOT touch any existing handler, change tool.rs dispatch, or rename any
action. You are ONLY adding a new file plus one line in lib.rs.

Files to create or modify:
- NEW: agent/tool/tool-workflow/src/instance.rs
- MODIFY: agent/tool/tool-workflow/src/lib.rs — add `mod instance;` and
  `pub use instance::{InstanceMeta, AgentSummary, PhaseSpan, EventStats,
  ReportRef, WorkflowRef, build_instance_meta, write_instance_artifacts};`

instance.rs must expose (signature-by-signature match with the design doc):

pub struct InstanceMeta {
    pub schema_version: u32,
    pub instance_id: String,
    pub instance_dir: String,
    pub workflow: WorkflowRef,
    pub status: String,
    pub created_at: u64,
    pub completed_at: u64,
    pub total_tokens: u64,
    pub total_elapsed_ms: u64,
    pub agent_count: u32,
    pub agents: Vec<AgentSummary>,
    pub phase_spans: Vec<PhaseSpan>,
    pub event_stats: EventStats,
    pub report: ReportRef,
    pub checkpoint_hash: String,
}
pub struct WorkflowRef { pub kind: &'static str, pub name: Option<String>, pub path: Option<String> }
pub struct AgentSummary { ... design doc fields ... }
pub struct PhaseSpan { ... }
pub struct EventStats { pub total: u64, pub by_type: BTreeMap<String, u64> }
pub enum ReportRef { Inline(Value), File { r#ref: String, preview: String, value_type: String, size_bytes: u64 }, Empty }

Constants:
  REPORT_INLINE_LIMIT = 800
  AGENT_OUTPUT_INLINE_LIMIT = 2048
  SOURCE_INLINE_LIMIT = 32768  // declared here too; tool.rs will `pub use` if needed

Functions:

pub fn build_instance_meta(
    checkpoint: &serde_json::Value,
    events: &[serde_json::Value],
    workflow_src: Option<&str>,
    workflow_ref: &WorkflowRef,
    instance_dir: String,
    checkpoint_bytes: &[u8],
) -> InstanceMeta;

pub fn write_instance_artifacts(
    dir: &Path,
    meta: &InstanceMeta,
    report_value: Option<&serde_json::Value>,
) -> std::io::Result<()>;

// Internal helpers — pub(crate) so tests can reach them:
pub(crate) fn classify_output(raw: &str) -> (&'static str, Value, String, u64, Option<String>);
pub(crate) fn replay_phase_spans(events: &[Value]) -> Vec<PhaseSpan>;
pub(crate) fn parse_workflow_meta_from_lua(src: &str) -> Option<serde_json::Value>;
pub(crate) fn summarise_event_types(events: &[Value]) -> EventStats;

Behaviour rules — read the design doc §Relationship to the raw checkpoint for
the field mapping table and implement every row. Key points:

  * instance_id = checkpoint.run_id
  * workflow = workflow_ref (no reading checkpoint.task)
  * total_elapsed_ms = sum of agent_done.elapsed_ms across all agents in events
  * agents ordered by agent_started event order, each entry carries output_type
    in {"json","text"} (try serde_json::from_str — success → json),
    output_size = raw byte len, output_preview = first 400 chars,
    output_ref = Some("agent-outputs/<aid>.txt".into()) when output_size
    > AGENT_OUTPUT_INLINE_LIMIT, else None
  * report = ReportRef::Inline(v) when serde_json size ≤ REPORT_INLINE_LIMIT,
    ReportRef::File{...} when larger, ReportRef::Empty when run_done.report is
    null/missing (failed/cancelled run)
  * phase_spans replayed from phase_span_started + phase_span_ended event pairs
    (ended_at = the ended event ts; None if no matching ended event)
  * event_stats counted across every event; by_type uses event["type"]
  * checkpoint_hash = hex(sha256(checkpoint_bytes))

write_instance_artifacts must:
  1. Write pretty-printed instance.json to dir/instance.json.
  2. When report is ReportRef::File and report_value is Some(v), write
     pretty-printed report.json to dir/report.json.
  3. For each agent whose output > AGENT_OUTPUT_INLINE_LIMIT, create
     dir/agent-outputs/ (if missing) and write the raw agent output string
     to dir/agent-outputs/<agent_id>.txt. NEVER overwrite if the file
     already exists.
  4. Be idempotent — calling twice with same args must not corrupt.

Tests (write inline #[cfg(test)] mod tests plus a tests/instance_module.rs
integration test):

  - build_meta_single_agent_success
  - build_meta_multi_agent_success          (use the .luft/runs/luft-workflow_1783786025 fixture)
  - build_meta_failed_run                   (use .luft/runs/luft-workflow_1783784203)
  - build_meta_cancellation_propagates_status_cancelled
  - classify_output_text_vs_json
  - classify_output_above_limit_produces_output_ref
  - replay_phase_spans_pairs_started_ended
  - replay_phase_spans_missing_ended_leaves_ended_at_null
  - parse_workflow_meta_from_lua_extracts_table
  - parse_workflow_meta_from_lua_returns_none_on_garbage
  - summarise_event_types_counts_by_type_key
  - write_instance_artifacts_writes_instance_json
  - write_instance_artifacts_writes_report_json_when_report_large
  - write_instance_artifacts_writes_agent_outputs_when_large
  - write_instance_artifacts_idempotent
  - checkpoint_hash_matches_known_value      (sha256 of the fixture's
    checkpoint.json bytes equals the expected hex string — hard-code it)

Use the existing real artefacts under .luft/runs/luft-workflow_1783783769
and .luft/runs/luft-workflow_1783786025 as test fixtures by reading the
files via an env var LOOM_TEST_RUNS_DIR or with `std::fs::read_to_string`
relative to CARGO_MANIFEST_DIR (.. / .. / .luft / runs / ...). Skip the
integration tests with `#[ignore]` if the fixture path is missing.

Do not run any commands that mutate the git state. Do not commit. Do not
modify any test fixtures.

When done:
  - `cargo test -p tool-workflow --lib` must pass (unit tests only).
  - `cargo clippy -p tool-workflow -- -D warnings` clean (excluding pre-existing warnings).
  - Do not run `cargo test` for the whole workspace; other crates are not
    your concern.
Report which tests are ignored and why.
```

## Acceptance

- [ ] `agent/tool/tool-workflow/src/instance.rs` exists with all required items.
- [ ] `lib.rs` adds exactly `mod instance;` + the `pub use` line.
- [ ] `cargo test -p tool-workflow --lib` green.
- [ ] `cargo clippy -p tool-workflow -- -D warnings` clean for new code.
- [ ] `tool.rs` untouched.
- [ ] No `println!`, `dbg!`, or FIXME leftover.

---

# Task T-02 — paths `.luft` → `.loom` + action rename

**Branch:** `wf/instance-T02-paths-actions`
**Depends on:** nothing (Batch 2 isolated worktree; must end before Batch 3).

## Loom prompt

```
You are working in your own git worktree on branch
`wf/instance-T02-paths-actions`. Two related changes, both in
tool-workflow crate:

# Part A — path migration

Replace every Loom-facing reference to `.luft/**` with `.loom/**` for
Loom-owned artefacts. Luft-internal identifiers (run_id, RunHandle,
RunStatus, run_started/done event types) are NOT changed.

Files:
  - agent/tool/tool-workflow/src/tool.rs
      * WorkflowTool::runs_dir()  → renamed WorkflowTool::instances_dir()
        returning working_folder.join(".loom").join("instances")
      * WorkflowTool::workflows_dir() → returns
        working_folder.join(".loom").join("workflows")
      * All call sites of runs_dir() updated to instances_dir().
      * The string instances directory name prefix (if any) changes from
        `luft-workflow_<ts>` to `loom-instance_<ts>`.
        Check whether LuftBuilder lets you pass a custom directory name.
        If not, pre-create `loom-instance_<ts>` under .loom/instances and
        pass that absolute path as base_dir so Luft writes into it.
  - agent/tool/tool-workflow/src/workflow_resolver.rs
      * Search path order becomes:
        1. absolute path with .lua extension
        2. {working_folder}/.loom/workflows/{name}.lua
        3. {home}/.config/loom/workflows/{name}.lua  (was luft)
        4. {working_folder}/{name}.lua
        5. {name} as literal path
  - tests/* — update any path assertion.

DO NOT remove the legacy read fallback for .luft/runs in
handle_list_runs/list-instances yet. That fallback is added in T-04. In
T-02 just change the primary path to .loom; existing list-runs reads from
.loom/instances now.

# Part B — action rename

Replace the action enum values and the run_dir parameter with the new
names. Keep legacy aliases (deprecated) for one minor cycle.

In tool.rs::call() dispatch table:
  "run"          → "execute"         (handler fn renamed handle_execute)
  "list-runs"    → "list-instances"  (handler renamed handle_list_instances)
  "run-status"   → "instance-summary"(handler renamed handle_instance_summary)
  "list-workflows" stays the same.

If the LLM passes a legacy action name, accept it, run the new handler,
and in the returned JSON include an extra top-level field
`"deprecation": "<oldname> is now <newname>; update your calls."`.

Input schema (spec()):
  - Rename the "run_dir" parameter to "instance_dir".
  - Update the "action" enum to the six new values: execute,
    list-workflows, list-instances, instance-summary, instance-events
    (forward-declare even if not implemented yet), instance-source
    (same).
  - Description text updated per the design doc §Tool description.
  - Limit/concurrency/args/script/workflow fields unchanged in this task.

Tests:
  - Update all existing tool-workflow test files (terminal_events.rs,
    parallel_mapper.rs, builtin_skill.rs, builtin_skill_injection.rs) to
    use the new action names and `instance_dir` parameter.
  - Add a new test in tests/legacy_action_alias.rs: an `action:"run"`
    call returns a `deprecation` field and otherwise behaves like
    `execute` (when given `script:"...a trivial inline lua..."` that
    just calls `report({ok=true})`).

Keep work scoped to this task. Do NOT add new actions like
instance-events or instance-source handlers — they are T-06/T-07. Do
NOT touch the handler bodies beyond what the rename requires; the
execute wiring is T-03, list-instances pagination is T-04,
instance-summary content is T-05.

When done:
  - `cargo test -p tool-workflow` green.
  - `rg "\.luft" agent/tool/tool-workflow/src` returns no hits.
  - `rg "run_dir|action.*\"run\"" agent/tool/tool-workflow/src` returns
    no hits (except comments explaining legacy alias).
  - `rg "loom-instance_" agent/tool/tool-workflow/src` finds the new
    prefix constant.
```

## Acceptance

- [ ] `cargo test -p tool-workflow` green.
- [ ] `rg "\.luft" agent/tool/tool-workflow/src` empty.
- [ ] `rg "runs_dir|handle_run|\"run-status\"" agent/tool/tool-workflow/src` empty (except legacy-alias comments).
- [ ] `rg "\.loom" agent/tool/tool-workflow/src` finds both `workflows` and `instances` references.
- [ ] Legacy `action:"run"` / `action:"list-runs"` / `action:"run-status"` still accepted with `deprecation` field.

---

# Task T-03 — wire `execute` to write `instance.json`

**Branch:** `wf/instance-T03-execute-wiring`
**Depends on:** T-01 merged, T-02 merged.

## Loom prompt

```
Worktree branch: `wf/instance-T03-execute-wiring`. The main branch already
contains T-01 (instance.rs) and T-02 (.loom paths + execute rename).
Your job is to make `handle_execute` write the instance artefacts after
each run and return the compact JSON summary.

Edit agent/tool/tool-workflow/src/tool.rs::handle_execute only. Steps
at the end of the function, AFTER run_handle.join() resolves and BEFORE
returning ToolCallContent:

  1. Determine instance_dir (the directory Luft wrote to — recover it from
     the configured base_dir you passed to LuftBuilder; do NOT rely on
     scanning).
  2. Read instance_dir/checkpoint.json into a serde_json::Value. If the
     read fails, return ToolError("checkpoint missing after execute").
  3. Stream-parse instance_dir/events.jsonl into Vec<Value> by reading
     line-by-line and serde_json::from_str each non-empty line.
  4. Read instance_dir/workflow.lua as Option<String> (Ok if missing →
     None).
  5. Compute checkpoint_bytes = the raw file bytes you read in step 2.
     Compute checkpoint_hash via sha256 (reuse instance:: helpers — you
     may need to expose a `pub(crate) fn sha256_hex(bytes: &[u8]) ->
     String` inside instance.rs IF it's not already exported; otherwise
     add a small local helper in tool.rs).
  6. Build WorkflowRef from the execute arguments (kind: "file" if the
     user passed `workflow`, "inline" if they passed `script`; name/path
     accordingly).
  7. Call instance::build_instance_meta(&checkpoint, &events,
     workflow_src.as_deref(), &workflow_ref, instance_dir_name,
     &checkpoint_bytes) → InstanceMeta.
  8. Extract the report Value from the run_done event in `events`
     (the first event with type=="run_done" and a non-null `report`
     field; else None).
  9. Call instance::write_instance_artifacts(instance_dir, &meta,
     report_value.as_ref()). On Err, log a tracing::warn! and continue
     (the run still succeeded for the LLM's purpose).
 10. Build the compact return JSON exactly per design doc §execute:

    {
      "instance_id": meta.instance_id,
      "instance_dir": meta.instance_dir,
      "status": meta.status,
      "workflow": { "kind": meta.workflow.kind, "name": meta.workflow.name,
                    "path": meta.workflow.path },
      "agent_count": meta.agent_count,
      "total_tokens": meta.total_tokens,
      "total_elapsed_ms": meta.total_elapsed_ms,
      "report_ref": match meta.report {
          Inline(_) => null,
          File{ref,..} => ref.as_str(),
          Empty => null,
      },
      "report_preview": match meta.report {
          Inline(v) => v stringified truncated to 800 chars,
          File{preview,..} => preview,
          Empty => null,
      }
    }

For the cancelled path (already returns "Workflow cancelled." today),
still write instance.json with status="cancelled" and report=Empty
before returning the cancellation text.

For the failed path (run_status=Failed/Errored, or out.result is Err),
still write instance.json with status="failed" and report=Empty, WITHOUT
changing the existing ToolError error chain — the LLM still needs a
tool error so that ReAct can react. The instance.json is for later
querying via instance-summary.

Tests in tests/execute_artifacts.rs:
  - execute_writes_instance_json_success
  - execute_writes_instance_json_failed_status
  - execute_writes_instance_json_cancelled
  - execute_writes_report_json_when_report_large
    (use a workflow whose report() returns a >800-char string)
  - execute_returns_compact_json_with_instance_id
  - execute_returns_null_report_ref_for_inline_small_report
  - agent_output_big_to_file
    (workflow where the sub-agent's reply is >2048 bytes)

Use `tempfile::TempDir` for working_folder so the .loom/instances tree
is created under a temp dir. Inject a minimal `script` Lua like:

  meta = { reasoning = "test", phases = { { label = "p" } } }
  function main() report({ ok = true, msg = "hello" }) end

For the "large report" case use:

  function main() report({ blob = string.rep("x", 1500) }) end

When done:
  - `cargo test -p tool-workflow` green.
  - Manual sanity check: run the minimal workflow in a temp working
    folder and verify .loom/instances/loom-instance_<ts>/instance.json
    exists and is valid JSON.
```

## Acceptance

- [ ] `cargo test -p tool-workflow` green, including `execute_artifacts.rs`.
- [ ] `rg "handle_execute" agent/tool/tool-workflow/src/tool.rs` shows the new tail logic.
- [ ] Manual run produces `.loom/instances/<dir>/instance.json`.
- [ ] Cancelled and failed runs also produce an `instance.json`.

---

# Task T-04 — `list-instances` pagination, status filter, legacy tag

**Branch:** `wf/instance-T04-list-instances`
**Depends on:** T-02.

## Loom prompt

```
Worktree branch: `wf/instance-T04-list-instances`. Rewrite
`handle_list_instances` in agent/tool/tool-workflow/src/tool.rs. Keep
all other handlers untouched.

New parameters (parse defensively with sensible defaults):
  limit        : int   in [1,100]  default 20
  cursor       : str|null         default null
  status_filter: str|null         enum completed|failed|cancelled  default null

Behaviour:
  1. Enumerate instance_dir entries under two roots:
       a. <working_folder>/.loom/instances/   (primary, tag "current")
       b. <working_folder>/.luft/runs/        (legacy, tag "legacy")
     On each, prefer reading instance.json (O(1) summary). If missing, fall
     back to parsing checkpoint.json (same shape as before T-01/T-03).
     If neither exists, skip the entry silently.
  2. Build the entry record per design doc §list-instances:
       { instance_id, instance_dir, status, workflow (kind+name only), 
         created_at, completed_at, total_tokens, agent_count, source }
  3. Apply status_filter (exact match, case-insensitive) if provided.
  4. Sort by (created_at desc, instance_dir desc).
  5. Pagination:
       - If cursor is null: start from the beginning.
       - If cursor is provided: skip every entry whose
         (created_at, instance_dir) tuple is <= the cursor's tuple
         (cursor resolved by looking it up in the full list; if not
         found, error ToolError("cursor not found")).
       - Return at most `limit` entries.
       - next_cursor = the instance_dir of the last returned entry, or
         null if we've exhausted the list.
  6. Return:

       {
         "instances": [ ... ],
         "count": N,
         "next_cursor": "<dir>" or null,
         "has_more": bool
       }

Tests in tests/list_instances.rs:
  - list_default_limit_is_20
  - list_limit_clamped_to_100
  - list_cursor_returns_next_page
  - list_cursor_null_on_last_page
  - list_status_filter_failed_excludes_completed
  - list_status_filter_invalid_returns_invalid_input_error
  - list_legacy_luft_runs_tagged_legacy
  - list_current_instances_tagged_current
  - list_invalid_cursor_returns_error
  - list_empty_when_directory_missing

Create test fixtures by writing synthetic checkpoint.json files into a
tempdir-based instances_dir; do not rely on the real .luft/runs/ tree
(although you MAY use it as a smoke read).

When done:
  - `cargo test -p tool-workflow` green.
  - `rg "handle_list_instances" agent/tool/tool-workflow/src/tool.rs`
    shows the new dispatch + pagination function.
```

## Acceptance

- [ ] All ten `list_instances.rs` tests pass.
- [ ] Legacy `.luft/runs/` entries appear with `"source":"legacy"`.
- [ ] `next_cursor` is `null` on the last page.
- [ ] Invalid `status_filter` returns `ToolSourceError::InvalidInput`.

---

# Task T-05 — `instance-summary` handler

**Branch:** `wf/instance-T05-summary`
**Depends on:** T-01, T-02.

## Loom prompt

```
Worktree branch: `wf/instance-T05-summary`. Rewrite
`handle_instance_summary` in tool.rs to return the curated InstanceMeta
payload, NOT the raw checkpoint+events dump.

Steps:
  1. Resolve instance path = instances_dir() joined with the
     `instance_dir` parameter; error InvalidInput if not found.
  2. If instance.json exists inside, read it and return its content as
     ToolCallContent::Text (pretty JSON).
  3. If instance.json is missing (legacy .luft/runs/ entry or a new
     run from before T-03 wiring):
       a. Read checkpoint.json + events.jsonl + workflow.lua.
       b. Build WorkflowRef with kind="legacy", name=instance_dir.
       c. Call instance::build_instance_meta(...) on the fly.
       d. Write instance.json into the legacy directory (persist for
          future queries). On write failure, log warn and still return
          the meta.
       e. Return the meta pretty-printed.
  4. The returned JSON must NOT include a top-level `events` array — only
     `event_stats` (total + by_type).

Tests in tests/instance_summary.rs:
  - summary_reads_existing_instance_json
  - summary_builds_on_the_fly_when_missing
  - summary_persists_instance_json_after_build
  - summary_excludes_raw_events_array
  - summary_event_stats_present
  - summary_legacy_dir_builds_and_persists
  - summary_invalid_instance_dir_returns_invalid_input

Use the same fixture style as T-01/T-03 (tempdir). For
summary_legacy_dir_builds_and_persists, pre-populate a fake
.luft/runs/loom-instance_<ts>/ with checkpoint.json + events.jsonl and
assert instance.json gets written after the call.

When done:
  - `cargo test -p tool-workflow --test instance_summary` green.
  - `cargo clippy -p tool-workflow -- -D warnings` clean for new code.
```

## Acceptance

- [ ] Seven `instance_summary.rs` tests pass.
- [ ] `rg "\"events\":" agent/tool/tool-workflow/src/tool.rs` within `handle_instance_summary` returns nothing (summary never includes events).
- [ ] Legacy directory persists `instance.json` after first summary call.

---

# Task T-06 — `instance-events` handler

**Branch:** `wf/instance-T06-events`
**Depends on:** T-02.

## Loom prompt

```
Worktree branch: `wf/instance-T06-events`. Add a new action
`instance-events` to tool.rs. Implement handle_instance_events. No
other handlers changed.

Parameters:
  instance_dir : str (required)
  offset       : int  default 0, min 0
  events_limit : int  default 50, max 500  (clamp transparently)
  types        : array of str|null  default null (no filter)
  agent_id     : str|null          default null (no filter)

Algorithm:
  1. Resolve instance path; InvalidInput if missing.
  2. Open events.jsonl buffered. If file missing return:
       { instance_dir, offset:0, events_limit, total_matching:0,
         events:[], next_offset:null }
  3. Single pass through file:
       - parse each non-empty line as serde_json::Value
       - apply filters: types (set membership by event["type"]),
                       agent_id (event.get("agent_id").map(|s| s == agent_id).unwrap_or(false))
       - count filtered events as total_matching
       - if filtered-count > offset and returned < events_limit, push
         to the returned events Vec
  4. Build next_offset:
       - returned_count = events.len()
       - if offset + returned_count < total_matching: next_offset = Some(offset + returned_count)
         else next_offset = None
  5. Return:

       {
         "instance_dir": <dir>,
         "offset": <int>,
         "events_limit": <int>,
         "total_matching": <int>,
         "next_offset": <int or null>,
         "events": [ ... ]
       }

Add to the call() dispatch table under "instance-events".
Add to the spec() input_schema the four parameter fields per the design
doc.

Tests in tests/instance_events.rs:
  - events_default_limit_50
  - events_limit_clamped_to_500
  - events_offset_skips_matching
  - events_type_filter_includes_only_matching
  - events_type_filter_with_multiple_types
  - events_agent_filter_includes_only_matching_agent
  - events_next_offset_null_on_last_page
  - events_next_offset_set_when_more_remain
  - events_missing_instance_dir_invalid_input
  - events_missing_events_jsonl_returns_empty_array
  - events_unparseable_line_skipped_silently

Use tempdir with a fake events.jsonl file containing ~20 lines of
synthetic JSONL events. Build the JSONL by hand:

  {"type":"run_started","run_id":"r1","ts":"2026-07-01T00:00:00Z"}
  {"type":"agent_started","agent_id":"a1","prompt_preview":"p","model":null}
  {"type":"agent_progress","agent_id":"a1","delta":{"kind":"message","text":"x"}}
  {"type":"agent_done","agent_id":"a1","status":"Ok","tokens":{},"elapsed_ms":0}
  ... a few more

When done:
  - `cargo test -p tool-workflow --test instance_events` green.
```

## Acceptance

- [ ] Eleven `instance_events.rs` tests pass.
- [ ] `events_limit` is always ≤ 500 on return.
- [ ] `next_offset` present only when more matching events remain.

---

# Task T-07 — `instance-source` handler

**Branch:** `wf/instance-T07-source`
**Depends on:** T-02.

## Loom prompt

```
Worktree branch: `wf/instance-T07-source`. Add new action
`instance-source` to tool.rs. Implement handle_instance_source. No
other handlers changed.

Parameters:
  instance_dir : str (required)

Behaviour:
  1. Resolve instance path; InvalidInput if missing.
  2. Read workflow.lua. If file missing, return ToolError("workflow.lua
     not found for instance <dir>").
  3. Compute size = bytes.len().
  4. If size <= SOURCE_INLINE_LIMIT (32768):
       Return {
         "instance_dir": <dir>,
         "workflow": { "kind": "file" | "inline", name?, path? }
           // recover from the source's first comment if possible,
           // else kind="legacy"
         "source": <full text>,
         "source_ref": null,
         "source_preview": null,
         "size_bytes": <int>
       }
  5. Else:
       preview = first 4096 chars (safe UTF-8 boundary).
       Return {
         "instance_dir": <dir>,
         "workflow": { ... },
         "source": null,
         "source_ref": "instance/workflow.lua",
         "source_preview": <preview>,
         "size_bytes": <int>
       }

Add to the call() dispatch table under "instance-source".
Update spec() input_schema (instance_dir already added in T-02).

Tests in tests/instance_source.rs:
  - source_inlined_when_small       (file < 32KB → "source" populated)
  - source_ref_when_over_32kb       (file = 40KB → "source" null, ref set, preview = first 4KB)
  - source_preview_at_utf8_boundary
  - source_missing_returns_tool_error
  - source_invalid_instance_dir_invalid_input

For the over-32KB test, generate the file content programmatically:
  std::fs::write(path, &"-- ".repeat(16384))  // > 32KB

When done:
  - `cargo test -p tool-workflow --test instance_source` green.
  - No other test files changed.
```

## Acceptance

- [ ] Five `instance_source.rs` tests pass.
- [ ] `SOURCE_INLINE_LIMIT = 32768` reused from `instance.rs`; do not duplicate the constant.

---

# Task T-08a — skill markdown rewrite (3 files only)

**Branch:** `wf/instance-T08a-skill-md`
**Depends on:** nothing (Batch 1).

## Loom prompt

```
Worktree branch: `wf/instance-T08a-skill-md`. Rewrite the workflow skill
documentation. You are NOT allowed to touch any .rs file. Documentation
only.

Files to create or replace:

1. REPLACE agent/tool/tool-workflow/src/workflow_skill.md with the thin
   ~80-line file described in the design doc §Skill restructure §1.

   Required structure (frontmatter + three sections + closing line):

   --- frontmatter ---
   name: workflow
   description: Lua DSL reference for writing multi-agent workflows
   triggers:
     - workflow
     - multi-agent
     - lua script
     - list-instances
     - instance-summary
     - debug workflow
     - workflow failed
     - workflow status
   metadata:
     conditions:
       requires_tools:
         - workflow
   tags:
     - workflow
     - orchestration
     - lua
     - instance
   ---

   # Workflow DSL Reference
   Use this when writing or debugging Lua workflow scripts passed to the
   `workflow` tool.

   ## §1 When to use which action
   (Decision table — 6 rows covering execute / list-workflows /
   list-instances / instance-summary / instance-events / instance-source.
   Columns: Intent | Action | Minimum args. One example args JSON line
   per row. Add a one-paragraph progressive-disclosure rule: always
   list-instances → instance-summary → instance-events; never jump to
   events first.)

   ## §2 Execution model
   (Copy verbatim the existing two paragraphs about Lua sandbox,
   pure orchestrator, report() first-call-wins.)

   ## §3 Minimal skeleton
   (Copy verbatim the existing skeleton block.)

   Closing line:
   > For the full Lua DSL reference (primitives, schema rules, error
   handling), see `references/dsl-reference.md`. For per-action schemas,
   return shapes, and the three-step diagnostic flow, see
   `references/tool-usage.md`. Load these via the `skill` tool when
   needed.

   Total size target ≤ 100 lines. Hard requirement: the file MUST NOT
   contain the primitive tables (agent/parallel/pipeline/phase/...).
   These move to dsl-reference.md.

2. CREATE agent/tool/tool-workflow/src/references/tool-usage.md

   ~170 lines. One section per action with:
     - Full params table (field | type | required | description).
     - Return shape table (field | type | description).
     - One example args JSON.
     - One example return snippet.
   Plus two dedicated sections:
     - "Three-step diagnostic flow":
         1. list-instances with {status_filter:"failed"} → pick most recent.
         2. instance-summary → check status, agents[].status, event_stats.
         3. instance-events with {types:["agent_done","run_done","parallel_done"]}
            → widen only if needed.
     - "Big payloads": documents report_ref, output_ref, source_ref —
       when each appears (REPORT_INLINE_LIMIT=800,
       AGENT_OUTPUT_INLINE_LIMIT=2048, SOURCE_INLINE_LIMIT=32768) and
       how to fetch full content with the read tool using
       .loom/instances/<instance_dir>/<ref>.

3. CREATE agent/tool/tool-workflow/src/references/dsl-reference.md

   Move the existing Primitives section verbatim from the current
   workflow_skill.md (lines 47–182 in the pre-T-08 source). Same
   content, same tables, same code blocks. Add a one-line intro:

   # Lua DSL Reference
   This is the detailed reference moved out of the main workflow skill.
   For action selection see workflow_skill.md §1; for per-action schemas
   see tool-usage.md.

Validate:
  - markdown is well-formed (prefer no raw HTML; tables in GitHub style).
  - the workflow skill main file ≤ 100 lines.
  - dsl-reference.md contains all primitive headings (agent, parallel,
    pipeline, phase, phase_begin, phase_end, workflow, report, log,
    budget, json.encode/decode).
  - tool-usage.md has all six action sections.

When done, report the line counts of each file. No tests to run (docs
only). No .rs files changed.
```

## Acceptance

- [ ] `workflow_skill.md` ≤ 100 lines, contains no primitive tables.
- [ ] `references/tool-usage.md` exists with all six action sections + two dedicated sections.
- [ ] `references/dsl-reference.md` exists with all primitive headings.
- [ ] `rg "include_str!" agent/tool/tool-workflow/src/` shows the existing three `include_str!("references/...md")` constants (T-08b will rewire these; T-08a does not).
- [ ] No `.rs` file modified.

---

# Task T-08b — wire new skill files into `tool.rs`

**Branch:** `wf/instance-T08b-skill-wiring`
**Depends on:** T-02 merged, T-08a merged.

## Loom prompt

```
Worktree branch: `wf/instance-T08b-skill-wiring`. Update tool.rs to
register the new skill files added by T-08a. No markdown edits.

Edits in agent/tool/tool-workflow/src/tool.rs:

  1. Adjust include_str! constants block (lines ~17–22 in current code):
       const WORKFLOW_SKILL: &str = include_str!("workflow_skill.md");
       const REF_TOOL_USAGE: &str = include_str!("references/tool-usage.md");
       const REF_DSL_REFERENCE: &str = include_str!("references/dsl-reference.md");
       const REF_ARCH_HEADER: &str = include_str!("references/architecture-header.md");
       const REF_AGENT_PROMPTS: &str = include_str!("references/agent-prompts.md");
       const REF_DECOMPOSITION: &str = include_str!("references/task-decomposition.md");
       const REF_ADVERSARIAL: &str = include_str!("references/adversarial-verification.md");
       const REF_EXAMPLES: &str = include_str!("references/examples.md");

  2. Update builtin_skill() at the bottom of the Tool impl:
       references: vec![
         ("references/tool-usage.md".to_string(),            REF_TOOL_USAGE.to_string()),
         ("references/dsl-reference.md".to_string(),         REF_DSL_REFERENCE.to_string()),
         ("references/architecture-header.md".to_string(),   REF_ARCH_HEADER.to_string()),
         ("references/agent-prompts.md".to_string(),        REF_AGENT_PROMPTS.to_string()),
         ("references/task-decomposition.md".to_string(),   REF_DECOMPOSITION.to_string()),
         ("references/adversarial-verification.md".to_string(), REF_ADVERSARIAL.to_string()),
         ("references/examples.md".to_string(),             REF_EXAMPLES.to_string()),
       ],
       length == 7.

  3. The triggers list inside builtin_skill() is built from WORKFLOW_SKILL
     frontmatter — keep parsing it at runtime from WORKFLOW_SKILL (or, if
     the existing code hardcodes the triggers Vec, update the Vec to
     match the new triggers list from T-08a:
       ["workflow","multi-agent","lua script","list-instances",
        "instance-summary","debug workflow","workflow failed",
        "workflow status"]).
     Inspect built code to decide.

  4. tags must include "instance": ["workflow","orchestration","lua","instance"].

Tests:
  - tests/builtin_skill.rs:
      assert_eq!(references.len(), 7);
      assert!(references[0].0.contains("tool-usage"));
      assert!(references[1].0.contains("dsl-reference"));
      assert!(triggers.contains(&"list-instances".to_string()));
      assert!(triggers.contains(&"instance-summary".to_string()));
      assert!(triggers.contains(&"debug workflow".to_string()));
      assert!(tags.contains(&"instance".to_string()));
  - tests/builtin_skill_injection.rs:
      assert that the injected skill body contains the §1 decision
      table marker (e.g. "When to use which action") and does NOT
      contain any primitive heading from the DSL (e.g. assert that the
      string "pipeline{ items=, stages=, max_inflight= }" is absent
      from the main skill content).

When done:
  - `cargo test -p tool-workflow --test builtin_skill --test
    builtin_skill_injection` green.
  - `cargo clippy -p tool-workflow -- -D warnings` clean for new code.
```

## Acceptance

- [ ] Seven `references` entries including `tool-usage.md` and `dsl-reference.md`.
- [ ] New triggers and `instance` tag asserted in tests.
- [ ] Existing `terminal_events` / `parallel_mapper` tests still green.

---

# Task T-09a — commit design doc + create changelog stub

**Branch:** `wf/instance-T09a-docs`
**Depends on:** nothing (Batch 1).

## Loom prompt

```
Worktree branch: `wf/instance-T09a-docs`. Documentation-only task.

1. The file docs/design/workflow-instance-model.md already exists in
   your worktree at the new branch HEAD (it was committed in the main
   branch as part of planning). If it is somehow missing from this
   worktree (check with `git log --oneline -- docs/design/workflow-instance-model.md`),
   fetch it from main:
     git checkout main -- docs/design/workflow-instance-model.md

   Do NOT modify workflow-instance-model.md. It is the source plan and
   must remain untouched as a historical record.

2. Create the changelog directory if it does not exist:
     mkdir -p docs/changelog

3. Create docs/changelog/instance-model.md with this content:

   # Instance Model Refactor (workflow tool)

   This changelog tracks the refactor described in
   docs/design/workflow-instance-model.md. It is a living checklist —
   tick boxes as each task ships to main.

   ## breaking changes
     - workflow tool action `run` renamed `execute` (legacy alias kept
       for one minor, returns a `deprecation` field).
     - `list-runs` renamed `list-instances`.
     - `run-status` renamed `instance-summary` and now returns the
       curated InstanceMeta payload instead of the raw
       checkpoint+events dump.
     - New actions `instance-events` and `instance-source` added.
     - `run_dir` parameter renamed `instance_dir`.
     - Workflow storage path moves from `.luft/{workflows,runs}/` to
       `.loom/{workflows,instances}/`. Legacy `.luft/runs/` entries
       remain readable via `list-instances` with `source:"legacy"`
       until the next minor.

   ## task tracking
     - [ ] T-01 instance.rs clean-layer module
     - [ ] T-02 paths + action rename
     - [ ] T-03 execute wiring writes instance.json
     - [ ] T-04 list-instances pagination + status filter + legacy tag
     - [ ] T-05 instance-summary handler
     - [ ] T-06 instance-events handler
     - [ ] T-07 instance-source handler
     - [ ] T-08a skill markdown rewrite
     - [ ] T-08b skill wiring in tool.rs
     - [ ] T-09a docs + changelog (this entry)
     - [ ] T-09b CLI audit + final acceptance

   ## migration for users
     - Move `.luft/workflows/*.lua` files to `.loom/workflows/`. The
       resolver only looks under `.loom/workflows/` from now on.
     - Past runs under `.luft/runs/` are auto-discovered by
       `list-instances` for one minor release; copy them to
       `.loom/instances/` and rename `luft-workflow_<ts>` →
       `loom-instance_<ts>` if you want them treated as current.

4. Update docs/design/workflow-runtime-reliability-proposal.md to add a
   single cross-reference line near the top:

   > See also docs/design/workflow-instance-model.md — the instance
   > model is the curated persistence substrate referenced throughout
   > the reliability proposal.

5. Do NOT change any other docs, READMEs, or .rs files. Markdown edits
   only. Run no commands other than git inspection and your markdown
   edits.

When done, report the diff summary.
```

## Acceptance

- [ ] `docs/changelog/instance-model.md` committed.
- [ ] `docs/design/workflow-instance-model.md` byte-identical to main.
- [ ] `docs/design/workflow-runtime-reliability-proposal.md` has the cross-reference line.
- [ ] No `.rs` files changed in this task.

---

# Task T-09b — CLI audit + final acceptance sweep

**Branch:** `wf/instance-T09b-cli-audit`
**Depends on:** T-02 merged (everything else merged ideally).

## Loom prompt

```
Worktree branch: `wf/instance-T09b-cli-audit`. Final sweep.

1. Audit apps/cli/src/ for any hard-coded references to `.luft` or
   `runs_dir` or `run_dir`:
     rg "\.luft|runs_dir|run_dir" apps/cli/src/

   Replace each with the `.loom` / `instances_dir` / `instance_dir`
   equivalent. Preserve backwards-compat CLI flags only if they are
   already documented external; otherwise rename and emit a
   deprecation note to stderr.

2. Update any TUI labels (search for "Run" capitalised in UI strings):
     rg "Run (|Runs " apps/cli/src/

   Convert to "Instance" / "Instances" where they refer to a workflow
   execution. Leave "Run" alone where it refers to "run a command".

3. Update existing CLI tests: rg "run_dir|action.*\"run\"|\"list-runs\""
   in apps/cli/tests/. Update assertions.

4. Tick every checkbox in docs/changelog/instance-model.md so the
   released changelog reflects shipped state.

5. Update README.md (if it mentions `.luft/workflows` or the old
   `run-status` action) to point at the new layout and actions.

6. Verify:
     cargo test -p tool-workflow
     cargo test -p cli
     cargo clippy -p tool-workflow -p cli -- -D warnings
   All must be green (or warnings-only on pre-existing issues).

7. Acceptance checklist from the design doc — go through it:
     - rg "\.luft" agent/tool/tool-workflow/src → no hits
     - rg "\.luft" apps/ → no hits outside legacy test fixtures
     - workflow_skill.md ≤ 100 lines
     - references/ contains exactly 7 files
     - builtin_skill() references length == 7
   Run each grep yourself and report the result.

Do NOT merge back to main automatically. Finish with a summary of
passes/failures and a recommended merge order.
```

## Acceptance

- [ ] `cargo test -p tool-workflow -p cli` green.
- [ ] `cargo clippy -p tool-workflow -p cli -- -D warnings` clean for new code.
- [ ] `rg "\.luft" apps/` returns no hits outside legacy test fixtures.
- [ ] All checkboxes in `docs/changelog/instance-model.md` ticked.

---

# Summary table

| Task   | Branch                          | Batch | Dependencies       | New files | Tests file                         |
| ------ | ------------------------------- | ----- | ------------------ | --------- | ---------------------------------- |
| T-01   | `wf/instance-T01-instance-mod`  | 1     | —                  | 1 + 1 lib | `instance_module.rs`               |
| T-08a  | `wf/instance-T08a-skill-md`     | 1     | —                  | 3 md      | —                                  |
| T-09a  | `wf/instance-T09a-docs`          | 1     | —                  | 2 md      | —                                  |
| T-02   | `wf/instance-T02-paths-actions`  | 2     | —                  | —         | `legacy_action_alias.rs`           |
| T-08b  | `wf/instance-T08b-skill-wiring`  | 3     | T-02, T-08a        | —         | existing `builtin_skill.rs`        |
| T-07   | `wf/instance-T07-source`         | 3     | T-02               | —         | `instance_source.rs`               |
| T-06   | `wf/instance-T06-events`         | 3     | T-02               | —         | `instance_events.rs`               |
| T-05   | `wf/instance-T05-summary`        | 3     | T-01, T-02         | —         | `instance_summary.rs`              |
| T-04   | `wf/instance-T04-list-instances` | 3     | T-02               | —         | `list_instances.rs`                |
| T-03   | `wf/instance-T03-execute-wiring` | 3     | T-01, T-02         | —         | `execute_artifacts.rs`             |
| T-09b  | `wf/instance-T09b-cli-audit`     | 4     | T-02 (better: all) | —         | existing CLI tests                 |

## Terraforming the worktrees (manual alternative to `--worktree`)

If Loom's `--worktree` auto-management is unavailable on your platform,
bootstrap each worktree manually:

```powershell
cd C:\Users\heycj\dev\loom
git worktree add ../loom-T01 -b wf/instance-T01-instance-mod
# start Loom in that worktree:
loom --worktree -w C:\Users\heycj\dev\loom-T01 -m "<paste T-01 prompt>"
```

Wait, that's wrong — `--worktree` creates its own worktree. To run Loom
in YOUR worktree (without `--worktree` flag), use `-w` to point at the
worktree directory:

```powershell
cd C:\Users\heycj\dev\loom-T01
loom -m "<paste T-01 prompt>"
```

Loom inherits the working folder as CWD; it'll commit to the current
branch (`wf/instance-T01-instance-mod`) if asked. For Batch 3 tasks
that need the latest main, rebase before starting:

```powershell
git fetch origin
git rebase origin/main
```

Merge cadence for Batch 3: merge one task branch → rebase the next
 Batch 3 worktree onto main → start Loom. Never run two Batch 3 worktrees
 at the same time without rebasing — tool.rs will conflict.