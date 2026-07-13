# `loom skills inspect <name>` — Phase 1 Audit Report

> Phase 1 of 5. Read-only survey; no source files were modified.

---

## Existing `show` — What It Does, What It Lacks

### What `show` does (confirmed from source)

**File**: `apps/cli/src/subcommands.rs:396-525` (`handle_skills_command`)
**Registry used**: `cli::run::skill_registry::SkillRegistry` (old storage/crafting perspective)
**Output fields**: 6 fixed fields — `name`, `description`, `source` (`Auto`/`Manual`/`Evolved`), `lifecycle` (`Active`/`Stale`/`Archived`), `triggers`, `body`.

Text format:

```
Skill: <name>
════════════════════════════════════════════════════════════
Description: <frontmatter description, first 80 chars>
Source: <Auto | Manual | Evolved>
Lifecycle: <Active | Stale | Archived>
Triggers: <comma-joined triggers>

<raw body (no conditions, no references)>
```

JSON: full `SkillContent` struct via `serde_json::to_string_pretty`.

**Call site**: `apps/cli/src/main.rs:143` — `handle_skills_command(sa, args.json)` (only `json` flag passed, no `output_file`).

### What `show` lacks (per design §1.2)

| Missing field | What inspect adds |
|---|---|
| Discovery path (`Project`/`User`/`Agent`/`Data`/`Builtin`) | `source` via `SkillSource::label()` |
| `base_path` for locating files | `path` field |
| `embedded_content` for builtin skills | `is_builtin`, `embedded_references` |
| Readiness check (missing env vars, platform) | `readiness` object |
| `category` / `category_desc` | `category`, `category_desc` |
| `conditions.requires_tools`, `fallback_for_*`, `requires_toolsets` | `conditions` block |
| `metadata.tags`, `related_skills`, `required_env_vars` | dedicated fields |
| `prerequisites.commands` | `prerequisites` block |
| Supporting directories (`references/`/`templates/`/`scripts/`/`assets/`) | `supporting_files` |
| `embedded_files` from builtin skills | `embedded_references` with `byte_size` |
| Usage telemetry from `~/.loom/data/skills/.usage.json` | `usage` block |

---

## New APIs Needed

### 1. `agent::skill::discovery::SkillRegistry` (read-only consumer)

**File**: `agent/skill/src/discovery.rs`

| Item | Signature | Purpose | Line refs |
|---|---|---|---|
| `SkillRegistry::discover(working_folder, extra_dirs)` | `fn discover(&Path, &[PathBuf]) -> Result<Self, SkillDiscoveryError>` | Bootstrap registry from 5 disk paths | §96–135 |
| `SkillRegistry::add_builtin(name, desc, content, triggers, requires_tools, references)` | `fn add_builtin(&mut self, &str, &str, &str, Vec<String>, Vec<String>, Vec<(String,String)>)` | Inject WorkflowTool builtin; no-op on name collision | §163–204 |
| `SkillRegistry::list()` | `fn list(&self) -> &[SkillEntry]` | Iterate all entries | §206–208 |
| `SkillRegistry::load_skill_with_dir(name)` | `fn load_skill_with_dir(&self, &str) -> Result<(String, PathBuf), SkillDiscoveryError>` | Get body (parses frontmatter) + base_path | §215–283 |
| `SkillRegistry::apply_filters(...)` | `fn apply_filters(&mut self, ...)` | **NOT called** by inspect — inspect shows raw registry | §285–318 |
| `SkillRegistry::apply_toolset_filters(...)` | `fn apply_toolset_filters(&mut self, ...)` | **NOT called** by inspect | §320–355 |
| `SkillEntry` fields | `metadata: SkillMetadata`, `base_path: PathBuf`, `skill_file: PathBuf`, `source: SkillSource`, `embedded_content: Option<String>`, `embedded_files: Option<Vec<(String, String)>>` | Core data structure | §28–44 |
| `SkillSource` enum + `label()` | `enum SkillSource { Project, ProfileDir, User, Agent, Data, Builtin }` + `fn label(&self) -> &'static str` | Maps to `"Project"`/`"Builtin"` etc. | §46–71 |

**Key confirmation**: `SkillEntry::embedded_files` is `Option<Vec<(String, String)>>` — tuple `(name, content)`, **not** `(name, byte_size)`. Byte size must be computed as `content.len() as u64`.

### 2. `WorkflowTool::builtin_skill()` — workflow builtin provider

**File**: `agent/tool/tool-workflow/src/tool.rs:476-510`

```
fn builtin_skill(&self) -> Option<BuiltinSkill>
  returns Some(BuiltinSkill {
    name: "workflow",
    description: "Lua DSL reference for writing multi-agent workflows",
    content: WORKFLOW_SKILL (full embedded SKILL.md),
    triggers: ["workflow", "multi-agent", "lua script"],
    requires_tools: ["workflow"],
    references: [
      ("references/architecture-header.md", REF_ARCH_HEADER),
      ("references/agent-prompts.md", REF_AGENT_PROMPTS),
      ("references/task-decomposition.md", REF_DECOMPOSITION),
      ("references/adversarial-verification.md", REF_ADVERSARIAL),
      ("references/examples.md", REF_EXAMPLES),
    ]
  })
```

**Note**: `WorkflowTool::new(AgentConfig::default())` is the constructor pattern used in existing tests.

### 3. `SkillUsageStore::get(name)` + `SkillUsage` struct

**File**: `agent/skill/src/usage.rs`

| Item | Signature | Purpose | Line refs |
|---|---|---|---|
| `SkillUsageStore::new(base_dir: &Path)` | `fn new(&Path) -> Self` | Constructor; appends `.usage.json` internally | §135–139 |
| `SkillUsageStore::get(name)` | `fn get(&str) -> Option<SkillUsage>` | Read telemetry for one skill | §482–484 |
| `SkillUsage` fields | `name`, `use_count`, `view_count`, `patch_count`, `last_used_at`, `last_viewed_at`, `last_patched_at`, `created_at`, `created_by`, `state: Lifecycle`, `pinned`, `archived_at`, `absorbed_into` | All fields map directly to JSON schema `usage` block | §15–41 |

**Note**: `SkillUsageStore::new` takes a **directory**, not a path to `.usage.json`. The `.usage.json` suffix is appended internally. Callers must pass `~/.loom/data/skills/` (without trailing `.usage.json`).

### 4. `apps/cli/src/output.rs::write_json_output`

**File**: `apps/cli/src/output.rs:15-35`

```
pub(crate) fn write_json_output(
    value: &Value,
    file: Option<&Path>,   // None = stdout, Some(path) = write to file
    pretty: bool,          // true = to_string_pretty
) -> Result<(), Box<dyn std::error::Error>>
```

Signatures matches design §9.4 exactly. `handle_skills_command` currently does not pass `output_file` to JSON output — this is one of the changes needed.

### 5. `agent/tool/tool-basic/src/skill/view.rs::view_sub_file` — path traversal pattern

**File**: `agent/tool/tool-basic/src/skill/view.rs:220-264`

The canonicalize + `starts_with` pattern to replicate in `skill_inspect.rs`:

```rust
let target = skill_dir.join(file_path);
let canonical_skill = skill_dir.canonicalize()?;
let canonical_target = target.canonicalize()?; // must succeed (not found = error)
if !canonical_target.starts_with(&canonical_skill) {
    return Err(PathTraversal);
}
```

Key points confirmed:
- `canonicalize` is called on both skill dir and target.
- The target's `canonicalize` **must succeed** (errors are "file not found", not silently skipped).
- `starts_with` comparison is done on canonical paths.
- No `is_file()` check — `canonicalize` failure already covers "not found".
- `canonical_target.is_dir()` check exists separately (line 245–250), returns an error for directory access.

### 6. `apps/cli/Cargo.toml` — existing dependencies

**File**: `apps/cli/Cargo.toml`

```
agent  = { path = "../../agent/agent-core" }       # line 18
skill  = { path = "../../agent/skill" }              # line 21
tool-workflow = { path = "../../agent/tool/tool-workflow" }  # line 24
```

**All three required crates are already dependencies.** No new crates needed.

---

## Protected File List

> These files have pending changes in `git status` (status `M`). They are OUT OF
> SCOPE for all phases of this workflow. Do NOT modify, format, reset, or stage
> them.

- `agent/skill/src/lib.rs`
- `agent/skill/src/sync.rs`
- `agent/skill/src/usage.rs`
- `agent/tool/tool-basic/src/skill/manage.rs`

---

## Per-Phase Touch List

### Phase 2 — `apps/cli/src/args.rs` + `apps/cli/src/subcommands.rs` (CLI plumbing)

| File | Changes |
|---|---|
| `apps/cli/src/args.rs` | Add `Inspect { name: String, all: bool, read_file: Option<String>, source: Option<String> }` variant to `SkillsCommand` enum (after line 507). Uses `#[arg(long)]` for flags, `value_enum` for `source`. |
| `apps/cli/src/subcommands.rs` | Add `Inspect` arm to `match &skills_args.command` (around line 525). Change `handle_skills_command` signature to accept `output_file: Option<&Path>` as 3rd param. Route `Inspect` to `skill_inspect::run(...)`. |
| `apps/cli/src/main.rs` | Add `mod skill_inspect;` declaration (line ~138). Update `handle_skills_command` call to pass `args.file.as_deref()` as 3rd arg. |

**Does NOT touch**: `agent/skill/src/lib.rs`, `agent/skill/src/sync.rs`, `agent/skill/src/usage.rs`, `agent/tool/tool-basic/src/skill/manage.rs`.

### Phase 3 — `apps/cli/src/skill_inspect.rs` (core implementation, NEW FILE)

| Content | Description |
|---|---|
| `struct BuiltinSkillContribution` | Internal struct tracking which builtin was injected (tool_name, skill_name, source). |
| `fn build_inspect_registry(working_folder, extra_dirs)` | Calls `SkillRegistry::discover`, constructs `WorkflowTool::new(AgentConfig::default())`, calls `builtin_skill()`, calls `registry.add_builtin(...)`. Returns `(SkillRegistry, Vec<BuiltinSkillContribution>)`. |
| `enum SkillInspectError` | Custom error type covering: `NotFound`, `Ambiguous(Vec<SkillEntry>)`, `PathTraversal(String)`, `FileNotFound(String, io::Error)`, `InvalidSkillDir(io::Error)`, `MutualExclusion`. Maps to exit codes 2/3. |
| `fn resolve_entry(entries, name, source_filter)` | Resolver from §7.2: given all entries matching `name`, apply optional `source` filter. Returns 0/1/N candidates. Emits `Ambiguous` error with all candidates + hint if N > 1 and no filter. |
| `fn safe_join_under(skill_dir, file_path)` | Path traversal guard: canonicalize both, `starts_with` check, return `PathTraversal` error on escape. See §8.3. |
| `struct InspectOutput` | Serialisable struct matching JSON schema (§5.2). Fields: `name`, `source`, `source_raw`, `path`, `skill_file`, `is_builtin`, `readiness`, `category`, `category_desc`, `description`, `triggers`, `tags`, `conditions`, `required_env_vars`, `prerequisites`, `related_skills`, `supporting_files`, `embedded_references`, `usage`, `body`, `frontmatter_raw`. |
| `fn render_text(output, all)` | Formats `InspectOutput` to stdout per §5.1 text layout. Truncates body to 1.2 KB or 30 lines in default mode; `--all` shows full body. |
| `fn render_json(output, output_file, pretty)` | Serialises `InspectOutput` via `serde_json` and writes via `write_json_output`. Body always full-length in JSON. |
| `fn scan_supporting_files(base_path)` | Scans `references/`, `templates/`, `scripts/`, `assets/` subdirs under `base_path`. Returns `SupportFiles` struct. Builtin returns empty with a note. |
| `pub fn run(name, all, read_file, source, json, output_file) -> Result<(), Box<dyn Error>>` | Main entry point. Checks mutual exclusions (§4.4). Builds registry via `build_inspect_registry`. Resolves entry. Handles `--read-file` branch separately (outputs subfile content directly, no text/json schema). Builds `InspectOutput`. Calls render. |
| `#[cfg(test)] mod tests` | Unit tests per §10.1/§10.2: body truncation, `--all` full body, JSON schema completeness, `--read-file` path traversal rejection, prefix collision rejection, `--source` filter, ambiguous errors, builtin injection, disk-overrides-builtin, mutual exclusions. |

### Phase 4 — `apps/cli/tests/skill_inspect.rs` (integration tests, NEW FILE)

| Content | Description |
|---|---|
| `cli_inspect_workflow_shows_builtin` | `loom skills inspect workflow --json` → `source == "Builtin"`, `embedded_references` ≥ 5 items. |
| `cli_inspect_with_global_file_writes_to_disk` | `--json --file /tmp/x.json` writes to disk, content contains `"Builtin"`. |
| Smoke test helper functions | For manual verification: path traversal rejection, `--source` flag, default vs `--all` output shapes. |

Note: `apps/cli/tests/` directory may not exist yet — create if absent.

### Phase 5 — Review / refinement pass

| Task | Description |
|---|---|
| Clippy | Run `cargo clippy --all-targets` in `apps/cli/`. Expect 0 new warnings. |
| Cargo doc | Run `cargo doc -p cli` in `apps/cli/`. Expect 0 broken links. |
| Manual smoke | Run the checklist from §10.5: `inspect workflow`, `inspect workflow --json`, `inspect workflow --all`, `inspect workflow --read-file references/examples.md`, `inspect workflow --read-file ../../../etc/passwd` (expect exit 2), `inspect nonexistent` (exit 2), `inspect workflow --source Builtin`, `inspect workflow --json --file /tmp/x.json`. |
| Regression | Confirm `loom skills show workflow` output unchanged. |
| Help text | Verify `loom skills inspect --help` lists all flags, mutual exclusion rules, and `--source` enum values. |

---

## Risks / Open Questions

### 1. `SkillUsageStore::get` consumes the entry (ownership issue)

`SkillUsageStore::get` (line 482–484) calls `self.load()?.remove(name)` — it **removes** the entry from the HashMap. Subsequent calls to `get` for the same skill in the same process will return `None`. If `inspect` ever needs to read usage data more than once per invocation (e.g., both text render and JSON render), this is a problem. **Mitigation**: call `get` once, clone the result, discard the store.

### 2. `SkillUsageStore::new` takes a directory, not `.usage.json` path

The `new(base_dir: &Path)` constructor appends `.usage.json` internally. Callers must pass `~/.loom/data/skills/` as the base dir. This is mildly confusing but workable. The design doc §5.4 implies this correctly.

### 3. `SkillEntry::embedded_files` contains `(String, String)` tuples — byte_size must be derived

The tuple is `(name, content)`, not `(name, byte_size)`. The JSON schema §5.2 specifies `embedded_references` items as `{name, byte_size}`. Implementations must use `content.len() as u64` for `byte_size`. Confirmed consistent: `SkillViewTool` also stores full content for embedded refs.

### 4. `handle_skills_command` signature change ripples through call site

Changing `handle_skills_command` from `(skills_args, json)` to `(skills_args, json, output_file)` requires updating `main.rs:143`. The design doc §9.4 shows this clearly. Since this is in `apps/cli/src/main.rs` (not a protected file), it's in scope for phase 2.

### 5. `CliArgs::cwd` existence vs `std::env::current_dir()`

Design doc §14, open question #1: `CliArgs` may already have a `cwd: Option<PathBuf>` field (mentioned as `args.rs:27`). Implementation should check `args.cwd` before falling back to `std::env::current_dir()`. This was not verified in this audit — needs confirmation before phase 3.

### 6. `SkillSource::label()` returns `"Profile"` for `SkillSource::ProfileDir`

`SkillSource::label()` (discovery.rs:61–69) maps `ProfileDir` → `"Profile"` (not `"ProfileDir"`). The `--source` flag values should display `"Profile"` as the user-facing label, matching `label()`. Clap's `value_enum` display should use this. The JSON `source` field will also be `"Profile"`.

### 7. `name:ns` disambiguation — v1 NOT implementing

The design doc §7.4 says v1 does NOT implement `name:ns`. Only `--source` is the v1 disambiguation mechanism. The test plan (§10.1) includes `name_ns_disambiguates` — this test should be **excluded** from phase 3 (or marked `#[ignore]`) since it's a future-compatibility item.

### 8. `SkillInspectError::Ambiguous` needs full candidate listing

The error type must carry all N candidates so the formatter can print the hint table (one per candidate with `Source:` and `path=`). The design doc §7.3 gives the exact format:

```
error: ambiguous skill 'workflow': found 2 matches
  1. workflow (Source: Builtin)  path=(embedded)
  2. workflow (Source: Project)  path=/home/u/proj/.loom/skills/workflow
hint: use --source <Source> (e.g. --source Builtin)
```

### 9. Body truncation: 1.2 KB OR 30 lines (first to trigger)

Design doc §5.5: truncate at whichever comes first — **bytes** vs **newline count**. Implementation must track both independently rather than just byte length. `String::chars().take(...)` won't work for byte truncation; use `str::encode_utf16` for a char-based approach or track bytes and newlines simultaneously.

### 10. Builtin skill `--read-file` requires exact `==` match, not prefix

Design doc §8.4: for builtin skills (where `embedded_content.is_some()`), `--read-file` must look up the filename in `embedded_files` via **exact equality** (`==`), not prefix match. This prevents `references/architecture` from matching `references/architecture-header.md`. For filesystem skills, `safe_join_under` naturally enforces this.
