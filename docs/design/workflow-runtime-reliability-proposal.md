# Workflow Runtime Reliability Proposal

**Author:** Implementation proposal (not yet committed)
**Reviewed against:** `agent/tool/tool-workflow/src/{tool.rs,backend.rs,event_bridge.rs,structured_output.rs,workflow_resolver.rs,lib.rs}`, `agent/tool/tool-workflow/tests/{parallel_mapper.rs,terminal_events.rs,builtin_skill.rs,builtin_skill_injection.rs}`, `docs/design/workflow-runtime-improvements.md`
**Scope:** `agent/tool/tool-workflow` crate + `luft_core::contract` event types

> See also docs/design/workflow-instance-model.md - the instance
> model is the curated persistence substrate referenced throughout
> the reliability proposal.

---

## Context

`docs/design/workflow-runtime-improvements.md` defines eight reliability requirements for long-running multi-agent workflows. This proposal maps each requirement to concrete changes in the existing codebase, states explicit reviewer decisions, and identifies what stays out of scope.

**What already works** (do not regress):

| Area | Status |
|---|---|
| Parallel mapper + `prompt` validation propagates as `ToolError` | ✅ `parallel_mapper.rs` + `tool.rs` line 497 |
| Happy-path `agent_started` / `agent_done` pairing | ✅ `terminal_events.rs` `every_agent_started_has_agent_done` |
| Happy-path `agent_done` before `run_done` ordering | ✅ `terminal_events.rs` `successful_run_agent_done_before_run_done` |
| Synthetic `agent_done` helper exists | ✅ `tool.rs` lines 50–94 (`emit_synthetic_agent_done`, `emit_terminal_diagnostics`); abnormal closure coverage is missing |
| Cancellation via `CancellationToken` | ✅ `tool.rs` lines 380–388, `backend.rs` line 123 |
| Structured output validation | ✅ `structured_output.rs` + `backend.rs` lines 54–64 |
| Workflow resolution | ✅ `workflow_resolver.rs` + `tool.rs` lines 333–346 |
| `builtin_skill` injection via `default_extra_tools_provider` | ✅ `lib.rs` line 37, `builtin_skill_injection.rs` |

---

## 1. Parallel Mapper Contract

**Requirement:** `parallel(items, mapper)` mapper must return a complete agent configuration table. Non-conforming mappers must fail before any agent tokens are spent.

**Current behavior:** Validation happens in `luft-runtime/sdk/task.rs` (`build_task()`) and surfaces as a Lua `RuntimeError("agent: missing required 'prompt' field")`. `tool.rs` line 496–498 catches this and wraps it in `ToolError("Workflow failed: ...")`. This is correct.

**Decision 1 — Accept the current contract.**  
The mapper contract is unambiguous: `parallel(items, function(item) return {prompt = ..., name = ...} end)`. Do not support `parallel({{...}, {...}})` as an alternative form. The existing test `parallel_direct_table_without_prompt_fails` confirms the non-mapper form already fails, so drop any documentation that implies it works.

**File to update:** `agent/tool/tool-workflow/src/workflow_skill.md`  
**Change:** Remove any syntax example showing `parallel({...})` with direct table-of-tables. Keep only the mapper form.

**Decision 2 — Validation occurs at invocation, not as a separate preflight pass.**  
Dynamic Lua workflows cannot be globally statically validated — the mapper function's return value depends on runtime input (`_G._args`, global state). Validation therefore occurs inline: `luft-runtime/sdk/task.rs` `build_task()` validates each agent config at the moment it is constructed, before the backend receives any request. This means:

- A `parallel` with a broken mapper fails at zero-agent-token cost — the first invocation of the mapper inside luft's scheduler throws a Lua `RuntimeError`, which `tool.rs` wraps as `ToolError`.
- No separate generic `validate_workflow_plan()` preflight pass is added. Such a pass would either duplicate the existing inline check (wasteful) or need to re-execute the Lua script in a sandbox (equivalent to just letting luft run it).
- The zero-token guarantee is preserved: if any item's mapper call returns a non-table or a table missing `prompt`, execution aborts before the backend is contacted for any agent.

---

## 2. Terminal Event Closure

**Requirement:** Every started agent must have exactly one `agent_done` before `run_done`.

**Current behavior:** The happy path is covered: `tool.rs` `track_agent_lifecycle()` (line 38) tracks `in_flight: HashSet<Uuid>`, and `emit_terminal_diagnostics()` (line 84) can emit synthetic `agent_done` values. The abnormal stream-closure, task-join, cancellation, and missing-`RunDone` paths are not yet proven to preserve closure. The investigation in Section 6 must establish their actual behavior before claiming the invariant.

**Decision 3 — Standardize the `status` field on `agent_done`.**  
The synthetic `agent_done` in `emit_synthetic_agent_done()` (tool.rs line 50) sets `status: "Error"`. When luft emits a real `agent_done`, the TUI receives it via `forward_event()` → `luft_event_to_json()` (event_bridge.rs line 35). The luft `AgentDone` variant has a `status` field. Verify this maps correctly to the TUI event schema. Check `luft_core::contract::event::AgentEvent::AgentDone` fields — confirm the JSON serialization matches what the TUI expects (`"status": "Error"` vs `"status": "Ok"`).

**File to check:** `agent/tool/tool-workflow/src/event_bridge.rs` line 35  
**Action:** Add a unit test `event_bridge_maps_agent_done_status` that serializes `AgentDone { status: AgentStatus::Ok }` and `AgentDone { status: AgentStatus::Cancelled }` via `luft_event_to_json()` and asserts the `"status"` field matches the TUI expectation.

**Decision 4 — Add `error_kind` to synthetic `agent_done`.**  
`emit_synthetic_agent_done()` currently omits `error_kind`. When the TUI renders a synthetic agent_done, it cannot distinguish timeout from join failure. Add `error_kind: "agent_join_error"` to the synthetic event JSON in `tool.rs` line 52. This requires a new `error_kind` field in the JSON structure emitted by `emit_synthetic_agent_done`.

**File to update:** `agent/tool/tool-workflow/src/tool.rs` line 52  
**Change:** Append `"error_kind": "agent_join_error"` to the synthetic JSON payload.

---

## 3. Structured Failure Kinds

**Requirement:** Define a stable enum of failure kinds surfaced on terminal events and the final run result.

**Current behavior:** `backend.rs` returns `AgentResult { status: AgentStatus::Ok | AgentStatus::Cancelled }`. `backend.rs` line 136 maps agent errors to `BackendError::Execution`. `tool.rs` line 496–502 converts luft `ScriptError` to `ToolError`.

### Tool-side synthetic failure kind (tool.rs)

**Decision 5a — Add a tool-internal `ToolFailureKind` enum for synthetic events emitted by `tool.rs`.**  
For synthetic `agent_done` events (early exit, channel closure), derive the failure kind locally. Add `ToolFailureKind` in `agent/tool/tool-workflow/src/failure_kind.rs`:

```rust
/// Failure kinds for synthetic events emitted by tool-workflow itself.
/// These cover cases where luft never emitted a real agent_done.
pub enum ToolFailureKind {
    /// Agent was in-flight when the workflow ended (normal exit, cancellation).
    AgentJoinError,
    /// The luft event channel closed before run_done was received.
    EventChannelClosed,
}
```

Wire `ToolFailureKind` into `emit_synthetic_agent_done()` and `emit_terminal_diagnostics()` so the synthetic `error_kind` field reflects the actual root cause.

### Luft contract change for real `agent_done` events (coordinated separately)

**Decision 5b — Request a `failure_kind` field on luft's real `AgentDone` event via a versioned `luft_core::contract` change.**  
This is a luft-side change requiring coordination with the luft team. The tool-workflow crate **proposes** the schema:

```rust
// In luft_core::contract::event::AgentDone:
pub struct AgentDone {
    pub agent_id: Uuid,
    pub status: AgentStatus,
    // NEW:
    pub failure_kind: Option<LuftFailureKind>,
}

pub enum LuftFailureKind {
    AgentProviderError,
    AgentTimeout,
    ToolError,
    Cancelled,
    VerificationFailed,
    Unknown,
}
```

tool-workflow waits for this field to be available in the luft contract before wiring it into `forward_event()` → TUI. Until the luft contract change is merged, synthetic events carry `ToolFailureKind` and real events carry no `failure_kind` (the TUI falls back to the `status` field).

**Action for tool-workflow:** Add a tracking comment in `event_bridge.rs` marking the integration point with the expected field name. Do not block on the luft change.

---

## 4. Checkpoint / Resume

**Requirement:** Checkpoint per work item. Provide explicit recovery modes: `resume`, `retry <agent-id>`, `rerun-phase`.

**Current behavior:** `tool.rs` `handle_list_runs()` (line 206) reads `checkpoint.json` from `{run_dir}/checkpoint.json`. `handle_run_status()` (line 277) returns the full checkpoint + events + workflow source. The checkpoint format is a `Value` (arbitrary JSON) — the actual structure comes from luft.

**Decision 6 — Typed checkpoint writer and recovery logic belong in luft-core, not tool-workflow.**  
tool-workflow acts as a thin facade: it exposes the `recovery_mode` parameter, reads checkpoint status via `handle_run_status()`, and forwards recovery intent to luft. The typed `Checkpoint` struct, `RecoveryMode` enum, `should_skip_item()` logic, and checkpoint file writing are all implemented in `luft_core`. Adding them to tool-workflow duplicates logic and creates sync divergence.

**tool-workflow responsibilities (this proposal):**
- Add `"recovery_mode": {"type": "string", "enum": ["resume", "retry", "rerun_phase"], "default": null}` to the input schema in `spec()`.
- Add `"phase": {"type": "string", "default": null}` for `rerun_phase` mode.
- Add `"agent_id": {"type": "string", "default": null}` for `retry` mode.
- Pass `recovery_mode` as a string enum to `LuftBuilder::recovery_mode()` (requires corresponding luft-core builder method — coordinate with luft team).
- `handle_run_status()` continues to read the raw checkpoint JSON from `{run_dir}/checkpoint.json` and return it as-is. The caller (agent or user) is responsible for interpreting it.

** Luft-core responsibilities (coordinated separately):**
- `LuftBuilder::recovery_mode(RecoveryMode)` builder method.
- `Checkpoint` struct with typed phase/items/prompt_hash/outputs fields.
- `should_skip_item()` logic using `prompt_hash` comparison.
- Checkpoint file writing on phase boundaries.
- Stale checkpoint detection (orphaned runs).

**Decision 7 — No checkpoint schema versioning in tool-workflow.**  
Since tool-workflow never writes the checkpoint, there is no versioning concern on the tool side. The luft team handles checkpoint format evolution. tool-workflow treats the checkpoint as an opaque `Value` returned by `handle_run_status()`.

---

## 5. Concurrency and Workspace Locking

**Requirement:** Prevent concurrent workflow processes from writing the same workspace. Write-set inference is deferred because arbitrary Lua cannot be reliably classified as read-only before execution.

**Current behavior:** `LuftBuilder::concurrency()` (tool.rs line 365) controls agent parallelism. No write-set analysis. No workspace lock.

**Decision 8 — Cross-process file locking, default-on for every run.**  
`tokio::sync::RwLock` is single-process only. Workspace locking must survive process death (crash, SIGKILL), so it requires cross-process coordination via a lock file with owner metadata.

**Lock file format** (`{working_folder}/.luft/workflow.lock`):

```json
{
  "version": 2,
  "owner_nonce": "uuid-v7",
  "generation": 1,
  "owner": {
    "pid": 12345,
    "session_id": "sess_1749000000",
    "started_at": "2025-07-11T10:00:00Z"
  }
}
```

**Acquisition algorithm:**
1. Acquire an OS-level exclusive advisory lock on `{working_folder}/.luft/workflow.lock` and keep its file handle open for the run. The lock file's JSON metadata is diagnostic only; exclusivity comes from the OS lock, not `O_EXCL` alone.
2. If the exclusive lock cannot be acquired, read the owner metadata and return `ToolSourceError::InvalidInput("Workflow already running in this workspace (owner: sess_..., pid: ...)")`.
3. On normal exit, cancellation, or unwinding, release the OS lock by dropping the guard and close the handle. Remove the metadata file only when its `owner_nonce` still matches the guard's nonce.
4. Do not overwrite an existing lock file after a PID check. A process can acquire the lock between the check and overwrite, so that sequence is unsafe.

**Stale recovery:** OS advisory locks are released when the owning process dies, so a stale metadata file alone must not block a new run. If the platform lock API cannot be used, automatic reclaim is not safe; return a diagnostic that includes the owner metadata and require an explicit `break_workspace_lock` action that verifies an owner nonce before removal. Test both process-death release and the explicit break-lock path.

**Default-on:** `handle_run()` acquires the lock for every workflow run because dynamic Lua prevents reliable write-effect inference. A caller may opt out only by explicitly setting `allow_concurrent_read_only: true`; this is a caller assertion, not automatic detection, and must be recorded in the run metadata.

**File to update:** `agent/tool/tool-workflow/src/tool.rs` `spec()`, `handle_run()`  
**New symbols:**
- `struct WorkspaceLockGuard { path: PathBuf, owner_nonce: String }` — RAII lock releaser; verifies nonce before metadata-file removal
- `async fn acquire_workspace_lock(path: &Path, session_id: &str) -> Result<WorkspaceLockGuard, ToolSourceError>`  
- `async fn break_workspace_lock(path: &Path, expected_nonce: &str) -> Result<(), ToolSourceError>` — explicit stale-lock removal; refuses if nonce has changed
- platform-specific exclusive-lock implementation (Windows locking API; Unix `flock`/equivalent), behind one `WorkspaceLockGuard` interface

**Decision 9 — Write-set detection is deferred.**  
Write-set inference from agent output declarations requires the `outputs` feature (Section 5 in original). Defer until `outputs` is implemented.

---

## 6. Root-Cause Investigation: Stream Closure and RunDone

This section documents the investigation into `tool.rs` event stream closure bugs identified during design review. These are the root causes that Decisions 3–4 address.

### 6.1 The `tool.rs` stream closure path

When luft closes the event stream (via `Receiver` dropping or channel closure), `tool.rs` must handle three cases:

1. **Graceful completion:** luft sends `RunDone` followed by channel close. `forward_event()` returns `None`, the loop exits, and `handle_run()` returns normally.
2. **Early channel closure:** the channel closes before `RunDone` is received. `forward_event()` returns `None` on the next poll. The loop exits, but `run_done` was never emitted.
3. **Join failure:** an agent task panics or fails to join. `track_agent_lifecycle()` may have the agent in `in_flight` but never receives `agent_done`.

**Case 2** is the primary gap: `tool.rs` line 449 has a comment `// Error path` but no explicit handling. When `forward_event()` returns `None` (channel closed) and we have not yet seen `RunDone`, the function currently falls through to a `ToolError` without emitting synthetic diagnostics for any remaining in-flight agents.

**Instrumentation tests required (before the behavioral fix):**

| Test | File | What it verifies |
|---|---|---|
| `channel_closes_before_run_done_produces_run_done_synthetic` | `terminal_events.rs` | When `forward_event` returns `None` with in-flight agents, synthetic `run_done` is emitted |
| `channel_closes_with_no_in_flight_produces_run_done` | `terminal_events.rs` | Even with no in-flight agents, channel closure produces a `run_done` |
| `join_error_on_agent_task_emits_synthetic_agent_done` | `terminal_events.rs` | A panicked agent task results in `emit_synthetic_agent_done` with `ToolFailureKind::AgentJoinError` |
| `run_done_received_after_all_agent_done_is_ordered_correctly` | `terminal_events.rs` | When channel closes with `RunDone` last, ordering is preserved |

### 6.2 `RunDone` handling

`tool.rs` currently does not explicitly check for a `RunDone` event in the event loop. It relies on `forward_event()` — if `luft_event_to_json()` returns `None` for `RunDone`, the loop exits and `run_done` is never emitted to the TUI.

**Fix:** After the event loop exits, check whether `run_done` was set. If not, emit a synthetic `run_done` with `status: "Failed"` and `failure_kind: "EventChannelClosed"`. This is a terminal safety net — it guarantees the TUI always receives `run_done` regardless of how the channel closed.

**Instrumentation test:**
| Test | File | What it verifies |
|---|---|---|
| `missing_run_done_triggers_synthetic_run_done` | `terminal_events.rs` | When loop exits without a real `RunDone`, synthetic `RunDone` with `status: "Failed"` is emitted |

### 6.3 `join()` on agent tasks

`tool.rs` spawns agent tasks via `tokio::spawn` (or equivalent) and must `join()` them to drain panics. If `join()` is not called, panics are silently swallowed. Current code path:

1. `handle_run()` spawns luft → gets `Receiver<LuftEvent>`
2. Event loop consumes `Receiver`
3. On loop exit, spawned tasks may still be running
4. If tasks are not explicitly joined, any panic in a task leaks

**Instrumentation test:**
| Test | File | What it verifies |
|---|---|---|
| `agent_task_panic_is_drained_on_run_done` | `terminal_events.rs` | A panicking agent task does not prevent `run_done` from being emitted; panic is logged |
| `all_agent_tasks_joinable_before_run_completes` | `terminal_events.rs` | All spawned agent tasks have been joined by the time `run_done` is emitted |

**Note:** These instrumentation tests are added to `terminal_events.rs` to establish baseline behavior before the behavioral fixes in Decisions 3–4 are applied. If a test fails, it reveals whether the root cause has been fixed.

---

## 7. Test Strategy — Existing Coverage and Gaps

**What the existing tests cover:**

| Test file | Coverage |
|---|---|
| `parallel_mapper.rs` | Mapper missing `prompt` → ToolError; syntax error; successful run |
| `terminal_events.rs` | `agent_done` before `run_done`; 1:1 started/done; failed run emits `run_done`; report-only emits `run_done` |
| `builtin_skill.rs` | `builtin_skill()` exposes skill; injects into registry; overrides disk skill |
| `builtin_skill_injection.rs` | `build_react_config` with provider registers workflow tool + skill |
| `tool.rs` `#[cfg(test)]` | Concurrency bounds; `extract_user_args`; `inject_args_globals` |
| `workflow_resolver.rs` `#[cfg(test)]` | Resolution from `.luft/workflows/`; working folder; absolute path; not found |
| `structured_output.rs` `mod tests` | Valid JSON; invalid JSON; empty schema; spec name |

**Exact test gaps (not "coverage exists" — these are missing):**

| Gap | File to add to | Impact if missed |
|---|---|---|
| No test for `parallel` with empty items `parallel({}, fn)` | `parallel_mapper.rs` | Edge case: empty parallel produces zero agents — does it loop, error, or hang? |
| No test for cancellation path emitting `agent_done` | `terminal_events.rs` | `emit_synthetic_agent_done` on cancellation may not fire |
| No test for channel closure before `RunDone` | `terminal_events.rs` | Case 2 in §6.1 — no synthetic `run_done` emitted |
| No test for `agent_done` status field mapping in `event_bridge.rs` | `event_bridge.rs` | TUI may see wrong `status` string |
| No test for absolute path rejection without `.lua` extension | `workflow_resolver.rs` | Security: line 4 only checks `.lua` suffix |
| No test for default-on lock blocking concurrent runs | new `workspace_lock.rs` | Lock may not actually block |
| No test for lock acquisition succeeding when workspace is free | new `workspace_lock.rs` | False-positive lock failures |
| No test for `allow_concurrent_read_only=true` skipping lock | new `workspace_lock.rs` | Opt-out may not work |
| No test for synthetic `agent_done` with `error_kind` | `terminal_events.rs` | TUI cannot distinguish error types |
| No test for synthetic `run_done` emitted when channel closes early | `terminal_events.rs` | §6.2 — TUI hangs waiting for `run_done` |
| No test for agent task panic draining before `run_done` | `terminal_events.rs` | §6.3 — panic may leak |
| No test for process-death OS lock release | new `workspace_lock.rs` | Stale metadata blocks next run if OS lock not used |
| No test for explicit break-lock nonce verification | new `workspace_lock.rs` | Break-lock may delete an active owner's lock |
| No test for stale-lock diagnostic when OS advisory lock unavailable | new `workspace_lock.rs` | Silent failure or unsafe reclaim on platforms lacking advisory locks |
| No test for `RecoveryMode::Retry` schema parsing | `tool.rs` | Malformed retry request may panic |

**Decision 10 — Add 15 new test cases.** All are unit or integration tests using the existing test infrastructure. No new test binaries required. See §8 Rollout for phase assignment.

**Decision 11 — Coverage claims are not made for luft-core internals.**  
`luft-runtime/sdk/task.rs` `build_task()` validation is not tested from tool-workflow. That is luft's responsibility. tool-workflow's test `parallel_mapper_missing_prompt_fails_with_tool_error` covers the error propagation path, which is sufficient.

---

## 8. Bounded Context and Output

**Requirement:** Phase-level context injection, token budgets, early truncation.

**Current behavior:** No token budget enforcement. Full prompts sent to every agent. No phase-level context aggregation.

**Decision 12 — Non-goal for this proposal.**  
This is an optimization and UX improvement, not a reliability requirement. It touches luft-core's prompt assembly (outside this crate's boundary). Defer.

---

## 9. Rollout

### Phase 1 — Reliability fixes (tool-workflow only, no luft dependency)

- `failure_kind.rs`: `ToolFailureKind` enum + wiring into `emit_synthetic_agent_done()` and `emit_terminal_diagnostics()`
- `error_kind` field on synthetic `agent_done` (Decision 4)
- Synthetic `run_done` safety net after event loop exit (§6.2)
- default-on cross-process workspace lock with OS-level release semantics and explicit read-only opt-out (Decision 8)
- Instrumentation tests for §6.1–6.3 + Decisions 4, 8
- `event_bridge_maps_agent_done_status` test (Decision 3)
- `workspace_lock.rs` test file (15 gap tests, including allow_concurrent_read_only, atomic reclaim, stale-lock diagnostic)
- `workflow_skill.md` cleanup (Decision 1)
- `workflow_resolver.rs` absolute-path security test

### Phase 2 — Luft coordination (waits for luft-side contract change)

- `LuftBuilder::recovery_mode()` builder method (luft-side)
- Typed `Checkpoint` struct + `RecoveryMode` in luft (luft-side)
- `failure_kind` field on real `AgentDone` in `luft_core::contract` (luft-side)
- `forward_event()` wiring for `LuftFailureKind` → TUI (tool-workflow side, gated on field presence)

### Phase 3 — Deferred features

- Declarative `outputs`/`verify`
- Write-set detection
- Token budgets and bounded context

---

## 10. Compatibility

**Decision 13 — No breaking changes to the tool input schema.**  
All new recovery fields have `null` defaults. Workspace locking is intentionally default-on for all workflow runs; callers that require concurrent read-only inspection must explicitly assert `allow_concurrent_read_only: true`.

**Decision 14 — Checkpoint schema versioning is handled by luft.**  
tool-workflow does not write checkpoints. The luft team owns versioning.

---

## 11. Non-Goals

Explicitly out of scope for this proposal:

1. **Typed checkpoint writer in tool-workflow** — belongs in luft-core (Section 4, Decision 6).
2. **`LuftFailureKind` on real `agent_done` events** — belongs in `luft_core::contract` (Section 3, Decision 5b).
3. **`LuftBuilder::recovery_mode()` builder method** — belongs in luft-core (Section 4).
4. **Declarative `outputs` and `verify`** — requires DSL changes (Section 5 in original, deferred).
5. **Write-set inference** — depends on `outputs` feature (Section 6 in original, deferred).
6. **Bounded context and token budgets** — optimization, not reliability (Section 7 in original, deferred).
7. **Changes to the TUI** — event format changes (new fields on synthetic events) must be communicated to the opencode TUI team before merging Phase 1.
8. **Generic preflight validation pass** — dynamic Lua cannot be globally statically validated; inline validation at agent invocation is sufficient (Section 1, Decision 2).

---

## 12. Explicit Reviewer Decisions

| # | Decision | Rationale |
|---|---|---|
| 1 | Accept mapper-only contract for `parallel` | Existing test confirms non-mapper form already fails; eliminates ambiguity |
| 2 | No generic preflight validation pass; inline validation at agent invocation is zero-token | Dynamic Lua mapper output depends on runtime input; any preflight either duplicates the existing `build_task()` check or requires a full Lua re-execution |
| 3 | Verify `agent_done` status maps correctly to TUI | Current code relies on luft JSON serialization; add `event_bridge_maps_agent_done_status` test |
| 4 | Add `error_kind` to synthetic `agent_done` via `ToolFailureKind` | TUI needs to distinguish timeout vs join error vs channel closure |
| 5a | `ToolFailureKind` enum in tool-workflow for synthetic events | Synthetic events are tool-workflow's responsibility |
| 5b | `LuftFailureKind` on real `AgentDone` via luft contract change | Real events originate in luft; coordinate separately |
| 6 | Checkpoint writer/recovery in luft-core; tool-workflow only exposes/reads status | Duplicating checkpoint logic in tool-workflow creates sync divergence and versioning complexity |
| 7 | No checkpoint schema versioning in tool-workflow | Since tool-workflow never writes checkpoints, it has no versioning concern |
| 8 | Cross-process file locking with owner metadata, default-on for every run | Dynamic Lua cannot be pre-classified by write effects; OS locks release on process death |
| 9 | Defer write-set detection | Depends on `outputs` feature |
| 10 | Add 15 instrumentation + gap tests across 3 test files | Exact gaps identified in §7; no inflated coverage claims |
| 11 | Coverage claims are limited to tool-workflow | luft-core internals are not claimed as covered by tool-workflow tests |
| 12 | Defer bounded context and token budgets | Optimization, not reliability |
| 13 | No breaking changes to input schema | Backward compatibility via defaults |
| 14 | Luft owns checkpoint schema versioning | Out of scope for tool-workflow |

---

## 13. File Map

```
agent/tool/tool-workflow/src/
  tool.rs                    # Decisions 4, 8 — synthetic error_kind, default-on lock,
                             #   allow_concurrent_read_only, break_workspace_lock, spec updates
  backend.rs                # No changes (error mapping unchanged)
  event_bridge.rs           # Decision 3 — test AgentDone status mapping; tracking comment for LuftFailureKind
  failure_kind.rs           # Decision 5a — new file: ToolFailureKind enum
  checkpoint.rs             # NOT CREATED — checkpoint writer stays in luft-core
  lib.rs                    # Export failure_kind module
  workflow_resolver.rs      # Security test: reject absolute paths without .lua
  structured_output.rs      # No changes
  json_to_lua.rs           # No changes

agent/tool/tool-workflow/src/workflow_skill.md  # Decision 1 — remove non-mapper syntax

agent/tool/tool-workflow/tests/
  parallel_mapper.rs        # Decision 10 — empty items test
  terminal_events.rs        # Decision 10 — synthetic error_kind, cancellation, channel close,
                            #             missing run_done, agent panic draining
  builtin_skill.rs          # No changes
  builtin_skill_injection.rs # No changes
  workspace_lock.rs         # Decision 10 — new file: default-on lock blocking, lock free,
                             #   allow_concurrent_read_only opt-out, process-death release,
                             #   break-lock nonce verification, stale-lock diagnostic


luft_core (coordinated separately):
  luft_core/src/contract/event.rs   # Decision 5b — add failure_kind to AgentDone
  luft_core/src/builder.rs          # Decision 6 — recovery_mode() builder method
  luft_core/src/checkpoint.rs       # Decision 6 — typed Checkpoint, RecoveryMode, should_skip_item
```

---

## 14. Verification Plan

After Phase 1 implementation, run:

```powershell
cargo test -p tool-workflow
cargo test -p tool-workflow -- --ignored  # for integration tests
```

Expected: all test files pass, plus 15 new cases (5 in `terminal_events.rs`, 1 in `parallel_mapper.rs`, 6 in `workspace_lock.rs`, 1 in `event_bridge.rs`, 1 in `workflow_resolver.rs`, 1 in `tool.rs`). No regressions in existing tests.

**Reviewers must confirm before merge:**
- [ ] `emit_synthetic_agent_done` includes `ToolFailureKind` variant in `error_kind` field
- [ ] `emit_terminal_diagnostics` includes `ToolFailureKind::EventChannelClosed` when channel closes early
- [ ] Synthetic `run_done` with `status: "Failed"` is emitted if the event loop exits without a real `RunDone`
- [ ] A default workflow run blocks a second concurrent run to the same workspace
- [ ] Process death releases the OS lock; stale metadata alone does not block the next run
- [ ] `allow_concurrent_read_only: true` is the only opt-out and is recorded in run metadata
- [ ] Explicit break-lock refuses to remove metadata whose owner nonce changed
- [ ] All 15 gap tests pass (see §7 table)
- [ ] TUI team has been briefed on new fields in synthetic event payloads (`error_kind`, `ToolFailureKind`)
- [ ] Luft team has been briefed on the proposed `LuftFailureKind` schema (for Phase 2 coordination)
