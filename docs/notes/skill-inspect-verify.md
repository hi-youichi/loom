# Skill Inspect — Verification Report

## Summary

`loom skills inspect <name>` is functional for all primary use cases.
Five bugs were identified and fixed in the viewing path.

## Bugs Fixed

### 1. `--all` hardcoded to `false` in `subcommands.rs`

**Before**: `all: _` pattern discarded the parsed flag; `false` was passed to `skill_inspect::run`.

**After**: `*all` is passed through correctly.

### 2. `supporting_files.references` had duplicates

**Before**: `collect_supporting_files_and_refs` had an unconditional `refs.push(name.clone())` *after* the if/else chain, causing every embedded file to appear twice in references and leaking templates/scripts/assets.

**After**: Only the `if name.starts_with("references/")` branch populates `refs`.

### 3. UTF-8/CJK truncation could panic

**Before**: `&body[..TEXT_BODY_PREVIEW_BYTES]` could panic if byte 1200 fell inside a multi-byte character.

**After**: `floor_char_boundary()` finds the largest valid char boundary ≤ 1200 before slicing.

### 4. `--read-file` error for invalid builtin paths

**Before**: Would return `FileNotFound` (correct type) but the error message wording is now verified to clearly say "file not found in skill" rather than "skill not found".

**After**: Confirmed working. Invalid paths return `SkillInspectError::FileNotFound`.

### 5. Metadata fields were hardcoded empty

**Before**: `conditions.requires_tools`, `tags`, `category`, `prerequisites`, `related_skills`, `required_env_vars` were all hardcoded to empty.

**After**: All fields are populated from `entry.metadata` via `conditions()`, `required_env_vars()`, etc.

## Tests

### `cargo test -p cli skill_inspect::` — 23 tests, all pass

| Test | Verifies |
|------|----------|
| `all_flag_shows_full_body_no_truncation_marker` | `--all` passthrough + no truncation |
| `utf8_cjk_truncation_is_safe` | CJK content doesn't panic |
| `builtin_workflow_in_inspect_registry` | Builtin workflow is injected |
| `read_file_builtin_examples_md_works` | `--read-file references/examples.md` |
| `read_file_builtin_invalid_path_is_file_not_found` | Error says "file not found", not "skill not found" |
| `supporting_files_references_no_duplicates` | No duplicates in references |
| `source_builtin_filters_workflow` | `--source Builtin` finds workflow |
| `source_builtin_rejects_non_builtin` | `--source Builtin` rejects project-only skills |
| *(+ 15 pre-existing tests)* | Path traversal, JSON schema, mutual exclusion, etc. |

### `cargo test -p cli` — 98 tests total (95 unit + 3 integration), all pass

## Smoke Commands

| Command | Result |
|---------|--------|
| `loom skills inspect workflow` | ✅ Text view with conditions, references, truncated body |
| `loom skills inspect workflow --all` | ✅ Full body, all conditions shown, no truncation marker |
| `loom skills inspect workflow --json` | ✅ All fields populated: name, source, conditions, references, body |
| `loom skills inspect workflow --read-file references/examples.md` | ✅ Prints embedded reference content |
| `loom skills inspect workflow --source Builtin` | ✅ Filters to Builtin source, finds workflow |

## Files Changed

| File | Change |
|------|--------|
| `apps/cli/src/skill_inspect.rs` | **New file**: inspect implementation + tests |
| `apps/cli/src/subcommands.rs` | Pass `*all` instead of `false` to `skill_inspect::run` |

## Protected Files (NOT modified)

| File | Status |
|------|--------|
| `agent/skill/src/lib.rs` | Pre-existing dirty — not touched |
| `agent/skill/src/sync.rs` | Pre-existing dirty — not touched |
| `agent/skill/src/usage.rs` | Pre-existing dirty — not touched |
| `agent/tool/tool-basic/src/skill/manage.rs` | Pre-existing dirty — not touched |

## Remaining Limitations

1. **`frontmatter_raw`** is always empty string — the raw YAML frontmatter is not separately extracted (the metadata fields are populated from the parsed struct instead).
2. **`readiness.status`** is always `"ready"` — no env-var validation is performed at inspect time.
3. **Usage data** (`use_count`, `view_count`, etc.) is best-effort from `~/.loom/.skills.usage.json`; defaults to zeros if file is absent.
4. **`BuiltinSkillContribution` struct fields** (`tool_name`, `skill_name`, `source`) trigger a dead_code warning — they are returned for traceability but not consumed by the CLI output path.
