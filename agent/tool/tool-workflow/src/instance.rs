//! Instance model — tool-workflow's clean-layer summary view over a Luft run.
//!
//! Writes `instance.json` (this struct), `report.json` (when the report is too
//! large to inline) and `agent-outputs/<aid>.txt` (when individual agent
//! outputs exceed [`AGENT_OUTPUT_INLINE_LIMIT`]). Operates on the raw
//! `checkpoint.json` + `events.jsonl` already produced by Luft; it never
//! touches the Luft contracts or the runtime events directly.
//!
//! The design lives in `docs/design/workflow-instance-model.md`. This module
//! is intentionally pure logic — no I/O except the writers below — so that
//! `cargo test -p tool-workflow --lib` exercises everything without spinning
//! up the workflow runtime.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Serialised JSON cutoff (in bytes) for inlining a report value inside
/// `instance.json`. Above this size the report is moved to a sibling
/// `report.json` file referenced by [`ReportRef::File`].
pub const REPORT_INLINE_LIMIT: usize = 800;

/// Raw-output cutoff (in bytes) for inlining an agent's output text inside
/// `instance.json`. Above this size the full text is moved to
/// `agent-outputs/<agent_id>.txt`.
pub const AGENT_OUTPUT_INLINE_LIMIT: usize = 2048;

/// Cutoff for the `instance-source` action (declared here so the constant is
/// present in the clean layer; `tool.rs` may re-export it for downstream use).
#[allow(dead_code)] // used by the planned `instance-source` tool action
pub const SOURCE_INLINE_LIMIT: usize = 32_768;

/// On-disk schema version for `instance.json`.
pub const SCHEMA_VERSION: u32 = 1;

// ============================================================================
// Data types
// ============================================================================

/// Clean-layer summary of one workflow instance. Serialised verbatim to
/// `instance.json`.
#[derive(Debug, Clone, Serialize)]
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

/// What the user invoked: a `.lua` file by name/path, or an inline script.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowRef {
    pub kind: &'static str, // "file" | "inline"
    pub name: Option<String>,
    pub path: Option<String>,
}

/// Per-agent rollup. `output_ref` points at `agent-outputs/<aid>.txt` when
/// `output_size > AGENT_OUTPUT_INLINE_LIMIT`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub phase_id: i32,
    pub status: String, // "ok" | "error" | "cancelled" | "timed_out"
    pub tokens: u64,
    pub elapsed_ms: u64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub role: Option<String>,
    pub output_type: &'static str, // "json" | "text"
    pub output_size: u64,
    pub output_preview: String, // first 400 chars
    pub output_ref: Option<String>,
}

/// Structural phase span replayed from `phase_span_started` +
/// `phase_span_done` (a.k.a. `phase_span_ended`) event pairs. `ended_at` is
/// `None` if the matching ended event was never observed (run cancelled or
/// crashed before the pair completed).
#[derive(Debug, Clone, Serialize)]
pub struct PhaseSpan {
    pub span_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub depth: u32,
    pub planned: u64,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

/// Histogram of every event seen in `events.jsonl`, keyed by `event.type`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct EventStats {
    pub total: u64,
    pub by_type: BTreeMap<String, u64>,
}

/// `Inline(Value)` ⇒ the report fits inside [`REPORT_INLINE_LIMIT`] and was
/// embedded verbatim. `File { ... }` ⇒ the full JSON was written to
/// `report.json`; only the preview is embedded. `Empty` ⇒ the run never
/// produced a report (failed / cancelled / no `report()` call).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ReportRef {
    Inline(Value),
    File {
        #[serde(rename = "ref")]
        r#ref: String,
        preview: String,
        value_type: String,
        size_bytes: u64,
    },
    Empty,
}

// ============================================================================
// Public API
// ============================================================================

/// Build an [`InstanceMeta`] from a Luft `checkpoint.json` + `events.jsonl`
/// pair. Pure function - no I/O.
///
/// When the checkpoint contains pre-computed summary fields (`agent_results`,
/// `completed_spans`, `event_stats`, `report`), those are used directly -
/// they are the engine's own bookkeeping maintained incrementally by
/// `update_from_event()`. The event-based derivation functions serve as
/// fallback for legacy checkpoints that lack these fields.
#[allow(clippy::too_many_arguments)]
pub fn build_instance_meta(
    checkpoint: &Value,
    events: &[Value],
    _workflow_src: Option<&str>,
    workflow_ref: &WorkflowRef,
    instance_dir: String,
    checkpoint_bytes: &[u8],
) -> InstanceMeta {
    let instance_id = checkpoint
        .get("run_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let created_at = checkpoint
        .get("created_at")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let completed_at = checkpoint
        .get("updated_at")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = checkpoint
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Status preference: `run_done.status` from events is the runtime's
    // authoritative verdict (a successful run that is then cancelled shows
    // up here as "cancelled", which we MUST propagate). The checkpoint's
    // `status` field is the same value in normal operation, but events are
    // immutable (append-only) while the checkpoint could be stale if the
    // cancellation happened after the last checkpoint flush.
    let status = locate_run_done_status(events).unwrap_or_else(|| {
        checkpoint
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string())
    });

    // Agents: prefer checkpoint's `agent_results` (engine's own per-agent cache).
    let agents = build_agent_summaries_from_checkpoint(checkpoint)
        .unwrap_or_else(|| build_agent_summaries(events, checkpoint));
    let total_elapsed_ms: u64 = agents.iter().map(|a| a.elapsed_ms).sum();
    let agent_count = agents.len() as u32;

    // Phase spans: prefer checkpoint's `completed_spans`.
    let phase_spans = replay_phase_spans_from_checkpoint(checkpoint)
        .unwrap_or_else(|| replay_phase_spans(events));

    // Event stats: prefer checkpoint's `event_stats`.
    let event_stats = summarise_event_types_from_checkpoint(checkpoint)
        .unwrap_or_else(|| summarise_event_types(events));

    // Report: prefer checkpoint's `report`.
    let report =
        build_report_ref_from_checkpoint(checkpoint).unwrap_or_else(|| build_report_ref(events));

    let checkpoint_hash = sha256_hex(checkpoint_bytes);

    InstanceMeta {
        schema_version: SCHEMA_VERSION,
        instance_id,
        instance_dir,
        workflow: workflow_ref.clone(),
        status,
        created_at,
        completed_at,
        total_tokens,
        total_elapsed_ms,
        agent_count,
        agents,
        phase_spans,
        event_stats,
        report,
        checkpoint_hash,
    }
}

/// Build agent summaries directly from the checkpoint's `agent_results` map.
/// Returns `None` when the checkpoint lacks this field (legacy format).
fn build_agent_summaries_from_checkpoint(checkpoint: &Value) -> Option<Vec<AgentSummary>> {
    let results = checkpoint.get("agent_results")?;
    let results_map = results.as_object()?;

    // Preserve insertion order by using the checkpoint's `started_agent_ids`
    // if available; otherwise fall back to the map's natural iteration order.
    let ordered_ids: Vec<String> = checkpoint
        .get("started_agent_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| results_map.keys().cloned().collect());

    let mut out = Vec::new();
    for agent_id in &ordered_ids {
        let entry = match results_map.get(agent_id) {
            Some(e) => e,
            None => continue,
        };

        let phase_id = entry
            .get("phase_id")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32)
            .unwrap_or(0);
        let status = entry
            .get("status")
            .and_then(|v| v.as_str())
            .map(normalise_agent_status)
            .unwrap_or_else(|| "unknown".to_string());
        let tokens = entry.get("tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let elapsed_ms = entry
            .get("elapsed_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let name = entry.get("name").and_then(|v| v.as_str()).map(String::from);
        let description = entry
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let role = entry.get("role").and_then(|v| v.as_str()).map(String::from);

        let raw = entry
            .get("output")
            .map(value_to_raw_string)
            .unwrap_or_default();
        let (output_type, _parsed, output_preview, output_size, _marker) = classify_output(&raw);
        let output_ref = if output_size > AGENT_OUTPUT_INLINE_LIMIT as u64 {
            Some(format!("agent-outputs/{agent_id}.txt"))
        } else {
            None
        };

        out.push(AgentSummary {
            agent_id: agent_id.clone(),
            phase_id,
            status,
            tokens,
            elapsed_ms,
            name,
            description,
            role,
            output_type,
            output_size,
            output_preview,
            output_ref,
        });
    }

    Some(out)
}

/// Build phase spans directly from the checkpoint's `completed_spans` array.
/// Returns `None` when the checkpoint lacks this field.
fn replay_phase_spans_from_checkpoint(checkpoint: &Value) -> Option<Vec<PhaseSpan>> {
    let spans = checkpoint.get("completed_spans")?.as_array()?;

    let out: Vec<PhaseSpan> = spans
        .iter()
        .map(|span| {
            let span_id = span.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let name = span
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let parent_id = span.get("parent_id").and_then(|v| v.as_i64());
            let depth = span.get("depth").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let planned = span.get("planned").and_then(|v| v.as_u64()).unwrap_or(0);
            // `started_at` and `completed_at` are Unix timestamps (u64) in
            // the checkpoint; convert to ISO 8601 strings for PhaseSpan.
            let started_at = span
                .get("started_at")
                .and_then(|v| v.as_u64())
                .map(unix_to_iso);
            let ended_at = span
                .get("completed_at")
                .and_then(|v| v.as_u64())
                .map(unix_to_iso);

            PhaseSpan {
                span_id,
                name,
                parent_id,
                depth,
                planned,
                started_at,
                ended_at,
            }
        })
        .collect();

    Some(out)
}

/// Build event stats directly from the checkpoint's `event_stats` map.
/// Returns `None` when the checkpoint lacks this field.
fn summarise_event_types_from_checkpoint(checkpoint: &Value) -> Option<EventStats> {
    let stats = checkpoint.get("event_stats")?.as_object()?;

    let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
    let mut total: u64 = 0;
    for (key, val) in stats {
        let count = val.as_u64().unwrap_or(0);
        by_type.insert(key.clone(), count);
        total += count;
    }

    Some(EventStats { total, by_type })
}

/// Build the report reference directly from the checkpoint's `report` field.
/// Returns `None` when the checkpoint lacks this field.
fn build_report_ref_from_checkpoint(checkpoint: &Value) -> Option<ReportRef> {
    let report = checkpoint.get("report")?;

    let value = match report {
        Value::Null => return Some(ReportRef::Empty),
        v => v.clone(),
    };

    let serialised = serde_json::to_string(&value).unwrap_or_default();
    if serialised.len() <= REPORT_INLINE_LIMIT {
        return Some(ReportRef::Inline(value));
    }

    let preview: String = serialised.chars().take(800).collect();
    let value_type = match &value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
    }
    .to_string();

    Some(ReportRef::File {
        r#ref: "report.json".to_string(),
        preview,
        value_type,
        size_bytes: serialised.len() as u64,
    })
}

/// Convert a Unix timestamp (seconds) to an ISO 8601 string.
fn unix_to_iso(ts: u64) -> String {
    // Simple conversion: use the system's UTC formatting.
    // If chrono is available we'd use it, but to avoid adding a dependency
    // we produce a RFC-3339-like string manually.
    // For now, store as a stringified number; consumers that need real ISO
    // dates can upgrade this later.
    format!("{ts}")
}

/// Write `instance.json`, optional `report.json`, and per-agent
/// `agent-outputs/<aid>.txt` files to `dir`.
///
/// Idempotent:
/// - `instance.json` and `report.json` are always overwritten (their content
///   is deterministic for a given meta + report value).
/// - `agent-outputs/<aid>.txt` is created on the first call and left
///   untouched on subsequent calls (file is "owned" by the first writer).
///
/// `raw_agent_outputs` is `&[(agent_id, raw_output_text)]`; only entries for
/// agents that need file-backing (i.e. whose `output_size` exceeds
/// [`AGENT_OUTPUT_INLINE_LIMIT`]) are actually written.
pub fn write_instance_artifacts(
    dir: &Path,
    meta: &InstanceMeta,
    report_value: Option<&Value>,
    raw_agent_outputs: &[(String, String)],
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;

    // 1) instance.json — pretty-printed.
    let instance_json = serde_json::to_string_pretty(meta).map_err(std::io::Error::other)?;
    fs::write(dir.join("instance.json"), instance_json)?;

    // 2) report.json — only when the meta says File AND the caller passed the
    //    value back. We never invent a report file from the inlined copy.
    if matches!(meta.report, ReportRef::File { .. }) {
        if let Some(v) = report_value {
            let report_json = serde_json::to_string_pretty(v).map_err(std::io::Error::other)?;
            fs::write(dir.join("report.json"), report_json)?;
        }
    }

    // 3) agent-outputs/<aid>.txt — only for agents whose output_ref is Some
    //    AND whose output exceeds the inline limit. create_new(true) is the
    //    idempotency guard: the second call sees EEXIST and returns Ok.
    let needs_files: Vec<&str> = meta
        .agents
        .iter()
        .filter(|a| a.output_ref.is_some())
        .map(|a| a.agent_id.as_str())
        .collect();

    if !needs_files.is_empty() {
        fs::create_dir_all(dir.join("agent-outputs"))?;
        for (agent_id, raw_output) in raw_agent_outputs {
            if !needs_files.contains(&agent_id.as_str()) {
                continue;
            }
            let path = dir.join("agent-outputs").join(format!("{agent_id}.txt"));
            write_if_absent(&path, raw_output.as_bytes())?;
        }
    }

    Ok(())
}

/// `Ok(())` when the bytes were written, `Ok(())` when they weren't because
/// the file already exists, `Err(_)` only on real I/O failure.
fn write_if_absent(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::ErrorKind;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(bytes)?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

// ============================================================================
// Public(crate) helpers — small, focused, unit-testable in isolation
// ============================================================================

/// Classify a raw agent-output string and produce the four fields the
/// [`AgentSummary`] needs. The fifth element is a marker — `Some(_)`
/// whenever the output exceeds [`AGENT_OUTPUT_INLINE_LIMIT`]; the actual
/// file path with the agent's UUID is composed by the caller.
pub(crate) fn classify_output(raw: &str) -> (&'static str, Value, String, u64, Option<String>) {
    let (output_type, parsed) = match serde_json::from_str::<Value>(raw) {
        Ok(v) => ("json", v),
        Err(_) => ("text", Value::String(raw.to_string())),
    };
    let preview: String = raw.chars().take(400).collect();
    let size = raw.len() as u64;
    let output_ref = if size > AGENT_OUTPUT_INLINE_LIMIT as u64 {
        // Caller substitutes the agent_id when constructing the public
        // AgentSummary — we don't know it here.
        Some(String::from("agent-outputs/<aid>.txt"))
    } else {
        None
    };
    (output_type, parsed, preview, size, output_ref)
}

/// Replay `phase_span_started` / `phase_span_done` (a.k.a. `phase_span_ended`)
/// event pairs into a Vec of [`PhaseSpan`] in start-order. `ended_at` is
/// `None` for spans whose ended event was never observed.
pub(crate) fn replay_phase_spans(events: &[Value]) -> Vec<PhaseSpan> {
    let mut spans: Vec<PhaseSpan> = Vec::new();
    let mut index: HashMap<i64, usize> = HashMap::new();

    for ev in events {
        let ev_type = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match ev_type {
            "phase_span_started" => {
                let span_id = ev.get("span_id").and_then(|v| v.as_i64()).unwrap_or(0);
                let name = ev
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let parent_id = ev.get("parent_id").and_then(|v| v.as_i64());
                let depth = ev.get("depth").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let planned = ev.get("planned").and_then(|v| v.as_u64()).unwrap_or(0);
                let started_at = ev.get("ts").and_then(|v| v.as_str()).map(String::from);

                let idx = spans.len();
                index.insert(span_id, idx);
                spans.push(PhaseSpan {
                    span_id,
                    name,
                    parent_id,
                    depth,
                    planned,
                    started_at,
                    ended_at: None,
                });
            }
            "phase_span_done" | "phase_span_ended" => {
                let span_id = ev.get("span_id").and_then(|v| v.as_i64()).unwrap_or(0);
                let ended_at = ev.get("ts").and_then(|v| v.as_str()).map(String::from);
                if let Some(&idx) = index.get(&span_id) {
                    if let Some(span) = spans.get_mut(idx) {
                        span.ended_at = ended_at;
                    }
                }
                // Spans we never saw the start of are dropped — keep them out
                // of the surface to avoid confusing the LLM.
            }
            _ => {}
        }
    }

    spans
}

/// Extract a JSON `meta` table from the leading comments of a `workflow.lua`
/// source. Returns `None` when no `@meta` marker is present or the JSON is
/// malformed. The real fixtures don't carry this marker, so they return
/// `None` — that's the documented "null on miss" path.
#[allow(dead_code)] // surfaced for tool.rs; only exercised by unit tests today
pub(crate) fn parse_workflow_meta_from_lua(src: &str) -> Option<Value> {
    for (i, raw_line) in src.lines().enumerate() {
        if i > 30 {
            break;
        }
        let trimmed = raw_line.trim_start();
        // Accept any number of `-` then `-@meta` markers in a line comment.
        let Some(without_dashes) = trimmed.strip_prefix('-') else {
            continue;
        };
        let stripped = without_dashes
            .trim_start()
            .trim_start_matches('-')
            .trim_start();
        let Some(after_meta) = stripped.strip_prefix("@meta") else {
            continue;
        };
        // `@meta:{...}` or `@meta {...}` — strip optional leading colon/space.
        let body = after_meta.trim_start().trim_start_matches(':').trim_start();
        if body.is_empty() {
            continue;
        }
        if let Some(v) = parse_lua_table_as_json(body) {
            return Some(v);
        }
        if let Ok(v) = serde_json::from_str::<Value>(body) {
            return Some(v);
        }
    }
    None
}

/// Accept Lua `{}` literals and lift them to JSON. Supports two shapes:
/// - Object form: every entry is `key = value` (recursive).
/// - Sequence form: entries are bare scalars (`"a","b"` or `1,2,3`).
///   Strings use `'…'` or `"…"`. This is intentionally tiny — good enough for
///   `@meta` headers users write by hand.
#[allow(dead_code)] // only exercised by parse_workflow_meta_from_lua for now
fn parse_lua_table_as_json(body: &str) -> Option<Value> {
    parse_lua_value(body)
}

#[allow(dead_code)] // only exercised by parse_lua_table_as_json
fn parse_lua_value(body: &str) -> Option<Value> {
    let body = body.trim();
    if body.starts_with('{') && body.ends_with('}') {
        let inner = &body[1..body.len() - 1];
        if inner.trim().is_empty() {
            return Some(Value::Object(Default::default()));
        }
        let entries = split_top_level(inner, ',');
        if !entries.is_empty()
            && entries.iter().all(|e| {
                let t = e.trim();
                !t.is_empty() && split_top_level(t, '=').len() == 2
            })
        {
            // Object form: every entry has exactly one top-level `=`.
            let mut obj = serde_json::Map::new();
            for entry in entries {
                let entry = entry.trim();
                let parts = split_top_level(entry, '=');
                if parts.len() != 2 {
                    return None;
                }
                let key_raw = parts[0].trim().trim_matches('"').to_string();
                let value = parse_lua_value(parts[1].trim())?;
                obj.insert(key_raw, value);
            }
            return Some(Value::Object(obj));
        }
        // Sequence form: array of scalars.
        let mut items = Vec::new();
        for el in entries {
            items.push(parse_lua_scalar(el.trim())?);
        }
        Some(Value::Array(items))
    } else {
        parse_lua_scalar(body)
    }
}

#[allow(dead_code)] // only exercised by parse_lua_table_as_json for now
fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '"' | '\'' => {
                in_str = !in_str;
                cur.push(c);
            }
            '{' | '[' if !in_str => {
                depth += 1;
                cur.push(c);
            }
            '}' | ']' if !in_str => {
                depth -= 1;
                cur.push(c);
            }
            c if c == sep && depth == 0 && !in_str => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[allow(dead_code)] // only exercised by parse_lua_table_as_json for now
fn parse_lua_scalar(s: &str) -> Option<Value> {
    let s = s.trim();
    if s == "true" {
        return Some(Value::Bool(true));
    }
    if s == "false" {
        return Some(Value::Bool(false));
    }
    if s == "nil" || s == "null" {
        return Some(Value::Null);
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(Value::String(inner.to_string()));
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return Some(Value::String(inner.to_string()));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(Value::Number(serde_json::Number::from(n)));
    }
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return Some(Value::Number(num));
        }
    }
    None
}

/// Tally every event by its `type` discriminator. Empty/missing types are
/// skipped from the histogram but still counted when their tag is non-empty.
pub(crate) fn summarise_event_types(events: &[Value]) -> EventStats {
    let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
    let mut total: u64 = 0;
    for ev in events {
        let ty = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if ty.is_empty() {
            continue;
        }
        *by_type.entry(ty.to_string()).or_insert(0) += 1;
        total += 1;
    }
    EventStats { total, by_type }
}

// ============================================================================
// Internal helpers
// ============================================================================

fn locate_run_done_status(events: &[Value]) -> Option<String> {
    events
        .iter()
        .find(|e| e.get("type").and_then(|v| v.as_str()) == Some("run_done"))
        .and_then(|e| e.get("status"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
}

fn build_agent_summaries(events: &[Value], checkpoint: &Value) -> Vec<AgentSummary> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Iterate in event-log order; agent_started defines ordering.
    let starts: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("agent_started"))
        .collect();

    for ev in starts {
        let agent_id = ev
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if agent_id.is_empty() || !seen.insert(agent_id.clone()) {
            continue;
        }
        let phase_id = ev
            .get("phase_id")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32)
            .unwrap_or(0);
        let name = ev.get("name").and_then(|v| v.as_str()).map(String::from);
        let description = ev
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);
        let role = ev.get("role").and_then(|v| v.as_str()).map(String::from);

        let done = events.iter().find(|e| {
            e.get("type").and_then(|v| v.as_str()) == Some("agent_done")
                && e.get("agent_id").and_then(|v| v.as_str()) == Some(agent_id.as_str())
        });

        let status = done
            .and_then(|e| e.get("status").and_then(|v| v.as_str()))
            .map(normalise_agent_status)
            .unwrap_or_else(|| "running".to_string());

        let elapsed_ms = done
            .and_then(|e| e.get("elapsed_ms").and_then(|v| v.as_u64()))
            .unwrap_or(0);

        // Tokens: take the checkpoint's per-agent figure (the source of
        // truth), falling back to nothing if the checkpoint didn't record
        // them (which shouldn't happen, but be defensive).
        let tokens = checkpoint
            .get("agent_results")
            .and_then(|v| v.get(&agent_id))
            .and_then(|r| r.get("tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // Raw output string. `agent_done.output` may already be a JSON value
        // (when the agent returned structured data) or a string. We treat
        // the rendered form as the "raw" form for classification.
        let raw = done
            .and_then(|e| e.get("output"))
            .map(value_to_raw_string)
            .unwrap_or_default();

        let (output_type, _parsed, output_preview, output_size, _marker) = classify_output(&raw);
        let output_ref = if output_size > AGENT_OUTPUT_INLINE_LIMIT as u64 {
            Some(format!("agent-outputs/{agent_id}.txt"))
        } else {
            None
        };

        out.push(AgentSummary {
            agent_id,
            phase_id,
            status,
            tokens,
            elapsed_ms,
            name,
            description,
            role,
            output_type,
            output_size,
            output_preview,
            output_ref,
        });
    }

    out
}

fn normalise_agent_status(s: &str) -> String {
    match s {
        "Ok" => "ok".to_string(),
        "Error" => "error".to_string(),
        "Cancelled" => "cancelled".to_string(),
        "TimedOut" => "timed_out".to_string(),
        other => other.to_lowercase(),
    }
}

fn value_to_raw_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => v.to_string(),
    }
}

fn build_report_ref(events: &[Value]) -> ReportRef {
    let report = events
        .iter()
        .find(|e| e.get("type").and_then(|v| v.as_str()) == Some("run_done"))
        .and_then(|e| e.get("report"));

    let value = match report {
        Some(v) if !v.is_null() => v.clone(),
        _ => return ReportRef::Empty,
    };

    let serialised = serde_json::to_string(&value).unwrap_or_default();
    if serialised.len() <= REPORT_INLINE_LIMIT {
        return ReportRef::Inline(value);
    }

    let preview: String = serialised.chars().take(800).collect();
    let value_type = match &value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
    }
    .to_string();

    ReportRef::File {
        r#ref: "report.json".to_string(),
        preview,
        value_type,
        size_bytes: serialised.len() as u64,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

// ============================================================================
// Fixtures — used only by tests below. Reads real artefacts under
// ANUREO_TEST_INSTANCES_DIR/anureo-instance_1783783769 (and friends) when the env var
// is set, otherwise falls back to <repo-root>/.anureo/instances/.../ .  When the
// file is absent in both places, the test reports a no-op skip (not a fail).
// ============================================================================

#[cfg(test)]
mod fixtures {
    use std::path::{Path, PathBuf};

    /// Resolve the instance fixtures dir using, in order:
    ///   1. `ANUREO_TEST_INSTANCES_DIR` env var (absolute path or relative-to-CWD)
    ///   2. `<CARGO_MANIFEST_DIR>/../../../.anureo/instances/` (the conventional
    ///      repo-root location)
    pub fn instances_dir() -> Option<PathBuf> {
        if let Some(env_val) = std::env::var_os("ANUREO_TEST_INSTANCES_DIR") {
            let p = PathBuf::from(env_val);
            if p.is_dir() {
                return Some(p);
            }
        }
        // CARGO_MANIFEST_DIR is set by cargo at compile-time.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        // tool-workflow lives at <root>/agent/tool/tool-workflow; three hops up
        // reaches the repo root.
        let candidate = manifest
            .join("..")
            .join("..")
            .join("..")
            .join(".anureo")
            .join("instances");
        if candidate.is_dir() {
            Some(candidate)
        } else {
            None
        }
    }

    pub fn load_jsonl(dir: &Path, file: &str) -> Vec<serde_json::Value> {
        let path = dir.join(file);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn load_json(dir: &Path, file: &str) -> Option<serde_json::Value> {
        let path = dir.join(file);
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn load_bytes(dir: &Path, file: &str) -> Option<Vec<u8>> {
        let path = dir.join(file);
        std::fs::read(&path).ok()
    }

    pub fn load_text(dir: &Path, file: &str) -> Option<String> {
        let path = dir.join(file);
        std::fs::read_to_string(&path).ok()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn inline_ref() -> WorkflowRef {
        WorkflowRef {
            kind: "inline",
            name: None,
            path: None,
        }
    }

    fn file_ref() -> WorkflowRef {
        WorkflowRef {
            kind: "file",
            name: Some("hello-agents".to_string()),
            path: Some(".anureo/workflows/hello-agents.lua".to_string()),
        }
    }

    fn single_agent_done(agent_id: &str, status: &str, elapsed_ms: u64) -> Value {
        json!({
            "type": "agent_done",
            "run_id": "run-1",
            "agent_id": agent_id,
            "status": status,
            "tokens": {"input": 100, "output": 10, "cache_read": 0, "cache_write": 0},
            "elapsed_ms": elapsed_ms,
            "name": null,
            "agent_seq": 0,
            "output": "Hello back",
            "findings": [],
            "prompt": "say hi",
            "retry_count": 0,
        })
    }

    fn single_agent_started(agent_id: &str) -> Value {
        json!({
            "type": "agent_started",
            "run_id": "run-1",
            "phase_id": 0,
            "agent_id": agent_id,
            "prompt_preview": "say hi",
            "model": null,
            "description": null,
            "role": null,
            "name": null,
            "agent_seq": 0,
        })
    }

    // -------- build_instance_meta: pure (synthetic) cases --------

    #[test]
    fn build_meta_single_agent_success() {
        let agent_id = "019f51cc-aaaa-7aaa-aaaa-aaaaaaaaaaaa";
        let checkpoint = json!({
            "run_id": "run-1",
            "task": "luft workflow",
            "status": "completed",
            "agent_results": {
                agent_id: {
                    "agent_id": agent_id,
                    "status": "ok",
                    "tokens": 1500,
                    "elapsed_ms": 2734,
                    "output": "Hello back",
                }
            },
            "total_tokens": 1500,
            "created_at": 1783783769,
            "updated_at": 1783783772,
            "started_agent_ids": [agent_id],
        });
        let events = vec![
            json!({"type":"run_started","run_id":"run-1","task":"luft workflow"}),
            single_agent_started(agent_id),
            single_agent_done(agent_id, "Ok", 2734),
            json!({"type":"run_done","run_id":"run-1","status":"completed","report":"Hello back"}),
        ];
        let checkpoint_bytes = serde_json::to_vec(&checkpoint).unwrap();

        let meta = build_instance_meta(
            &checkpoint,
            &events,
            None,
            &file_ref(),
            "anureo-instance_1783783769".to_string(),
            &checkpoint_bytes,
        );

        assert_eq!(meta.schema_version, 1);
        assert_eq!(meta.instance_id, "run-1");
        assert_eq!(meta.instance_dir, "anureo-instance_1783783769");
        assert_eq!(meta.workflow.kind, "file");
        assert_eq!(meta.workflow.name.as_deref(), Some("hello-agents"));
        assert_eq!(meta.status, "completed");
        assert_eq!(meta.created_at, 1783783769);
        assert_eq!(meta.completed_at, 1783783772);
        assert_eq!(meta.total_tokens, 1500);
        assert_eq!(meta.total_elapsed_ms, 2734);
        assert_eq!(meta.agent_count, 1);
        assert_eq!(meta.agents.len(), 1);
        assert_eq!(meta.agents[0].agent_id, agent_id);
        assert_eq!(meta.agents[0].status, "ok");
        assert_eq!(meta.agents[0].tokens, 1500);
        assert_eq!(meta.agents[0].output_type, "text");
        assert_eq!(meta.agents[0].output_size, b"Hello back".len() as u64);
        assert_eq!(meta.agents[0].output_preview, "Hello back");
        assert!(meta.agents[0].output_ref.is_none());
        assert!(!meta.checkpoint_hash.is_empty());
    }

    #[test]
    fn build_meta_cancellation_propagates_status_cancelled() {
        // Checkpoint shows "completed" (last persisted state) but the
        // runtime emits run_done.status = "cancelled". The clean-layer
        // status MUST be "cancelled".
        let checkpoint = json!({
            "run_id": "run-1",
            "status": "completed",
            "total_tokens": 0,
            "created_at": 100,
            "updated_at": 105,
        });
        let events = vec![json!({
            "type":"run_done",
            "run_id":"run-1",
            "status":"cancelled",
            "report": null
        })];
        let meta = build_instance_meta(
            &checkpoint,
            &events,
            None,
            &inline_ref(),
            "anureo-instance_x".into(),
            b"{}",
        );
        assert_eq!(meta.status, "cancelled");
    }

    // -------- classify_output --------

    #[test]
    fn classify_output_text_vs_json() {
        let (t1, _, p1, s1, r1) = classify_output("plain text");
        assert_eq!(t1, "text");
        assert_eq!(p1, "plain text");
        assert_eq!(s1, b"plain text".len() as u64);
        assert!(r1.is_none());

        let (t2, parsed2, p2, _s2, r2) = classify_output(r#"{"a":1,"b":"hi"}"#);
        assert_eq!(t2, "json");
        assert_eq!(parsed2, json!({"a": 1, "b": "hi"}));
        assert_eq!(p2, r#"{"a":1,"b":"hi"}"#);
        assert!(r2.is_none());

        // Garbage that's not valid JSON falls through to "text".
        let (t3, _, _, _, _) = classify_output("{ this is not json");
        assert_eq!(t3, "text");
    }

    #[test]
    fn classify_output_above_limit_produces_output_ref() {
        let big = "x".repeat(AGENT_OUTPUT_INLINE_LIMIT + 1);
        let (ty, _, prev, size, marker) = classify_output(&big);
        assert_eq!(ty, "text");
        assert_eq!(size, big.len() as u64);
        assert_eq!(prev.len(), 400); // truncated preview
        assert!(
            marker.is_some(),
            "output above limit must carry Some(_) marker"
        );
    }

    #[test]
    fn classify_output_preview_truncates_to_400_chars() {
        let big = "abcdefghij".repeat(100); // 1000 chars
        let (_, _, prev, _, _) = classify_output(&big);
        assert_eq!(prev.chars().count(), 400);
    }

    // -------- replay_phase_spans --------

    #[test]
    fn replay_phase_spans_pairs_started_ended() {
        let events = vec![
            json!({"type":"phase_span_started","span_id":1,"name":"root","parent_id":null,
                   "depth":0,"planned":2,"ts":"2026-07-11T10:00:00Z"}),
            json!({"type":"phase_span_started","span_id":2,"name":"child","parent_id":1,
                   "depth":1,"planned":0,"ts":"2026-07-11T10:00:01Z"}),
            json!({"type":"phase_span_ended","span_id":2,"name":"child","parent_id":1,
                   "depth":1,"ts":"2026-07-11T10:00:05Z"}),
            json!({"type":"phase_span_ended","span_id":1,"name":"root","parent_id":null,
                   "depth":0,"ts":"2026-07-11T10:00:06Z"}),
        ];
        let spans = replay_phase_spans(&events);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].span_id, 1);
        assert_eq!(spans[0].started_at.as_deref(), Some("2026-07-11T10:00:00Z"));
        assert_eq!(spans[0].ended_at.as_deref(), Some("2026-07-11T10:00:06Z"));
        assert_eq!(spans[1].span_id, 2);
        assert_eq!(spans[1].parent_id, Some(1));
        assert_eq!(spans[1].depth, 1);
        assert_eq!(spans[1].started_at.as_deref(), Some("2026-07-11T10:00:01Z"));
        assert_eq!(spans[1].ended_at.as_deref(), Some("2026-07-11T10:00:05Z"));
    }

    #[test]
    fn replay_phase_spans_missing_ended_leaves_ended_at_null() {
        let events = vec![
            json!({"type":"phase_span_started","span_id":3,"name":"open","parent_id":null,
                   "depth":0,"planned":0,"ts":"2026-07-11T10:00:00Z"}),
        ];
        let spans = replay_phase_spans(&events);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].span_id, 3);
        assert_eq!(spans[0].started_at.as_deref(), Some("2026-07-11T10:00:00Z"));
        assert!(spans[0].ended_at.is_none());
    }

    #[test]
    fn replay_phase_spans_treats_phase_span_done_as_end() {
        // `PhaseSpanDone` serialises to `"phase_span_done"`; the public
        // doc also mentions a hypothetical `"phase_span_ended"` key. Both
        // should close a span.
        let events = vec![
            json!({"type":"phase_span_started","span_id":1,"name":"x","parent_id":null,
                   "depth":0,"planned":0,"ts":"2026-01-01T00:00:00Z"}),
            json!({"type":"phase_span_done","span_id":1,"name":"x","parent_id":null,
                   "depth":0,"ts":"2026-01-01T00:00:01Z"}),
        ];
        let spans = replay_phase_spans(&events);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].ended_at.as_deref(), Some("2026-01-01T00:00:01Z"));
    }

    // -------- parse_workflow_meta_from_lua --------

    #[test]
    fn parse_workflow_meta_from_lua_extracts_table() {
        let src = r#"-- @meta {name = "refactor", version = 2, tags = {"a","b"}}
function main()
    return 0
end
"#;
        let v = parse_workflow_meta_from_lua(src).expect("must parse @meta");
        assert_eq!(v["name"], "refactor");
        assert_eq!(v["version"], 2);
        assert_eq!(v["tags"][0], "a");
        assert_eq!(v["tags"][1], "b");
    }

    #[test]
    fn parse_workflow_meta_from_lua_returns_none_on_garbage() {
        // No `@meta` marker at all.
        let src = "function main()\n    return 0\nend\n";
        assert!(parse_workflow_meta_from_lua(src).is_none());
    }

    #[test]
    fn parse_workflow_meta_from_lua_accepts_json() {
        let src = "-- @meta {\"k\":\"v\",\"n\":3}\nfunction main() end\n";
        let v = parse_workflow_meta_from_lua(src).expect("must parse JSON @meta");
        assert_eq!(v["k"], "v");
        assert_eq!(v["n"], 3);
    }

    #[test]
    fn parse_workflow_meta_from_lua_ignores_meta_after_line_30() {
        let mut src = String::new();
        for i in 0..40 {
            src.push_str(&format!("-- line {i}\n"));
        }
        src.push_str("-- @meta {\"reach\":false}\n");
        assert!(parse_workflow_meta_from_lua(&src).is_none());
    }

    // -------- summarise_event_types --------

    #[test]
    fn summarise_event_types_counts_by_type_key() {
        let events = vec![
            json!({"type": "agent_started"}),
            json!({"type": "agent_started"}),
            json!({"type": "agent_done"}),
            json!({"type": "run_done"}),
        ];
        let stats = summarise_event_types(&events);
        assert_eq!(stats.total, 4);
        assert_eq!(stats.by_type.get("agent_started"), Some(&2));
        assert_eq!(stats.by_type.get("agent_done"), Some(&1));
        assert_eq!(stats.by_type.get("run_done"), Some(&1));
    }

    // -------- write_instance_artifacts --------

    fn sample_meta() -> InstanceMeta {
        let aid = "019f51cc-aaaa-7aaa-aaaa-aaaaaaaaaaaa".to_string();
        InstanceMeta {
            schema_version: 1,
            instance_id: "run-1".into(),
            instance_dir: "anureo-instance_x".into(),
            workflow: file_ref(),
            status: "completed".into(),
            created_at: 100,
            completed_at: 105,
            total_tokens: 100,
            total_elapsed_ms: 50,
            agent_count: 1,
            agents: vec![AgentSummary {
                agent_id: aid.clone(),
                phase_id: 0,
                status: "ok".into(),
                tokens: 100,
                elapsed_ms: 50,
                name: None,
                description: None,
                role: None,
                output_type: "text",
                output_size: 1,
                output_preview: "x".into(),
                output_ref: None,
            }],
            phase_spans: vec![],
            event_stats: EventStats::default(),
            report: ReportRef::Empty,
            checkpoint_hash: "deadbeef".into(),
        }
    }

    #[test]
    fn write_instance_artifacts_writes_instance_json() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let meta = sample_meta();
        write_instance_artifacts(dir.path(), &meta, None, &[]).expect("ok");
        let p = dir.path().join("instance.json");
        let raw = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["instance_id"], "run-1");
        assert_eq!(v["schema_version"], 1);
    }

    #[test]
    fn write_instance_artifacts_writes_report_json_when_report_large() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let mut meta = sample_meta();
        meta.report = ReportRef::File {
            r#ref: "report.json".into(),
            preview: "preview".into(),
            value_type: "object".into(),
            size_bytes: 9000,
        };
        let report_v = json!({"answer": 42});
        write_instance_artifacts(dir.path(), &meta, Some(&report_v), &[]).unwrap();
        let p = dir.path().join("report.json");
        let raw = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["answer"], 42);
    }

    #[test]
    fn write_instance_artifacts_writes_agent_outputs_when_large() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let aid = "019f51cc-aaaa-7aaa-aaaa-aaaaaaaaaaaa".to_string();
        let mut meta = sample_meta();
        meta.agents[0].output_ref = Some(format!("agent-outputs/{aid}.txt"));
        meta.agents[0].output_size = (AGENT_OUTPUT_INLINE_LIMIT + 1) as u64;
        meta.agents[0].output_preview = "<truncated>".into();
        let big_text = "x".repeat(AGENT_OUTPUT_INLINE_LIMIT + 1);
        let raw = vec![(aid.clone(), big_text.clone())];
        write_instance_artifacts(dir.path(), &meta, None, &raw).unwrap();
        let p = dir.path().join("agent-outputs").join(format!("{aid}.txt"));
        assert!(p.exists(), "agent output file must exist");
        let stored = std::fs::read_to_string(&p).unwrap();
        assert_eq!(stored, big_text);
    }

    #[test]
    fn write_instance_artifacts_idempotent() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let aid = "019f51cc-aaaa-7aaa-aaaa-aaaaaaaaaaaa".to_string();
        let mut meta = sample_meta();
        meta.agents[0].output_ref = Some(format!("agent-outputs/{aid}.txt"));
        meta.agents[0].output_size = (AGENT_OUTPUT_INLINE_LIMIT + 1) as u64;
        let big_text: String = "first".into();
        let big_text_again: String = "second-overwrite-attempt".into();
        let raw1: Vec<(String, String)> = vec![(aid.clone(), big_text.clone())];

        write_instance_artifacts(dir.path(), &meta, None, &raw1).unwrap();
        // Second call: caller tries to "rewrite" the output. We must NOT
        // overwrite. The whole call must still succeed (no corruption).
        let raw2: Vec<(String, String)> = vec![(aid.clone(), big_text_again.clone())];
        write_instance_artifacts(dir.path(), &meta, None, &raw2).unwrap();
        let p = dir.path().join("agent-outputs").join(format!("{aid}.txt"));
        let stored = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            stored, big_text,
            "second call must NOT overwrite the agent output file"
        );
    }

    // -------- checksum round-trip --------

    #[test]
    fn sha256_of_bytes_is_hex() {
        let h = sha256_hex(b"abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // ============================================================
    // Fixture-backed tests — skip cleanly when artefacts are absent.
    // Activate by exporting ANUREO_TEST_INSTANCES_DIR=<path-to-.anureo/instances>.
    // ============================================================

    fn fixture_dir() -> Option<std::path::PathBuf> {
        fixtures::instances_dir()
    }

    /// Loads a real run directory and returns
    /// (checkpoint_value, checkpoint_bytes, events_vec, workflow_src).
    type LoadedRun = (Value, Vec<u8>, Vec<Value>, Option<String>);

    fn load_run(name: &str) -> Option<LoadedRun> {
        let instances = fixture_dir()?;
        let dir = instances.join(name);
        if !dir.is_dir() {
            return None;
        }
        let ckpt = fixtures::load_json(&dir, "checkpoint.json")?;
        let bytes = fixtures::load_bytes(&dir, "checkpoint.json")?;
        let events = fixtures::load_jsonl(&dir, "events.jsonl");
        let src = fixtures::load_text(&dir, "workflow.lua");
        Some((ckpt, bytes, events, src))
    }

    fn skip_if_missing(name: &str) -> bool {
        match fixture_dir() {
            None => {
                eprintln!("skip: no fixture dir (set ANUREO_TEST_INSTANCES_DIR)");
                true
            }
            Some(d) if !d.join(name).is_dir() => {
                eprintln!("skip: missing fixture {name}");
                true
            }
            Some(_) => false,
        }
    }

    #[test]
    fn build_meta_multi_agent_success() {
        if skip_if_missing("anureo-instance_1783786025") {
            return;
        }
        let (ckpt, bytes, events, src) = load_run("anureo-instance_1783786025").unwrap();
        let meta = build_instance_meta(
            &ckpt,
            &events,
            src.as_deref(),
            &file_ref(),
            "anureo-instance_1783786025".into(),
            &bytes,
        );
        assert_eq!(meta.status, "completed");
        assert_eq!(meta.agent_count, 3, "real fixture has 3 agents");
        assert_eq!(meta.agents.len(), 3);
        // Every agent in the fixture has output_type=text (the JSON is
        // wrapped in a markdown code block — not parseable as JSON).
        for a in &meta.agents {
            assert_eq!(a.output_type, "text");
        }
        // total_elapsed_ms = sum of per-agent elapsed_ms on agent_done events.
        let expected: u64 = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("agent_done"))
            .filter_map(|e| e.get("elapsed_ms").and_then(|v| v.as_u64()))
            .sum();
        assert_eq!(meta.total_elapsed_ms, expected);
        // The report in this fixture is structured (object), > 800 bytes:
        // ReportRef::File { ... }.
        assert!(
            matches!(meta.report, ReportRef::File { .. }),
            "expected File ref for large report"
        );
        // by_type must include run_done, agent_started, agent_done ...
        assert!(meta.event_stats.by_type.contains_key("agent_started"));
        assert!(meta.event_stats.by_type.contains_key("agent_done"));
        assert!(meta.event_stats.by_type.contains_key("agent_progress"));
        assert!(meta.event_stats.by_type.contains_key("run_done"));
    }

    #[test]
    fn build_meta_failed_run() {
        if skip_if_missing("anureo-instance_1783784203") {
            return;
        }
        let (ckpt, bytes, events, _src) = load_run("anureo-instance_1783784203").unwrap();
        let meta = build_instance_meta(
            &ckpt,
            &events,
            None,
            &file_ref(),
            "anureo-instance_1783784203".into(),
            &bytes,
        );
        assert_eq!(meta.status, "failed");
        assert!(matches!(meta.report, ReportRef::Empty));
        // The fixture's one agent is ok; the run is failed at the orchestration layer.
        assert_eq!(meta.agent_count, 1);
        assert_eq!(meta.agents[0].status, "ok");
        // phase_spans replayed from events (only the started events for both spans).
        assert_eq!(meta.phase_spans.len(), 2);
        for s in &meta.phase_spans {
            assert!(s.ended_at.is_none(), "no matching ended event");
        }
    }

    #[test]
    fn checkpoint_hash_matches_known_value() {
        if skip_if_missing("anureo-instance_1783783769") {
            return;
        }
        let instances = fixture_dir().unwrap();
        let ckpt_path = instances
            .join("anureo-instance_1783783769")
            .join("checkpoint.json");
        let bytes = fixtures::load_bytes(
            &instances.join("anureo-instance_1783783769"),
            "checkpoint.json",
        )
        .unwrap();
        assert!(ckpt_path.is_file());
        // SHA-256 of the on-disk checkpoint.json (874 bytes).
        let expected = "c9184f64305aa012ae4d83dda3cbaba50ce361c6d4ce606345676f175af3967f";
        assert_eq!(sha256_hex(&bytes), expected);

        // Also confirm the value flows through build_instance_meta.
        let ckpt: Value = serde_json::from_slice(&bytes).unwrap();
        let events: Vec<Value> =
            std::fs::read_to_string(ckpt_path.parent().unwrap().join("events.jsonl"))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
        let meta = build_instance_meta(
            &ckpt,
            &events,
            None,
            &file_ref(),
            "anureo-instance_1783783769".into(),
            &bytes,
        );
        assert_eq!(meta.checkpoint_hash, expected);
    }
}
