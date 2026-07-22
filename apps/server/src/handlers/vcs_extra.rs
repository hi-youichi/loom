//! VCS helpers (task P0.2 / LS-015).
//!
//! Runs real `git` commands against the server's working directory. These
//! routes (`/vcs/*`) are global — they are not session-scoped — so the
//! directory they operate on is the server process's current directory
//! (the project root the server was launched in, typically via
//! `--directory`). This matches the directory that `state::emit()`
//! publishes in the SSE envelope.
//!
//! When the working directory is not a git repository, the handlers
//! return a clear error envelope (carrying an `error` field) rather than
//! a fake clean status, so callers can distinguish "no repo" from a clean
//! working tree.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::state::{emit, SharedState};

/// `GET /vcs` — list VCS providers. Git is the only supported provider.
pub async fn get_vcs() -> Json<Value> {
    Json(json!({
        "branch": "main",
        "providers": ["git"],
    }))
}

/// `GET /vcs/status` — per-file status with additions/deletions.
/// Returns `Vcs.FileStatus[] = [{file, additions, deletions, status}]`.
pub async fn get_vcs_status() -> Json<Value> {
    let dir = working_dir();
    match run_git(&dir, &["status", "--porcelain"]).await {
        GitResult::Ok(status_output) => {
            let numstat = match run_git(&dir, &["diff", "--numstat", "HEAD"]).await {
                GitResult::Ok(n) => n,
                _ => String::new(),
            };
            Json(json!(parse_file_status(&status_output, &numstat)))
        }
        GitResult::NotARepo => error_envelope(
            "NOT_A_REPO",
            &format!("not a git repository: {dir}"),
        ),
        GitResult::OtherError(msg) => error_envelope(
            "GIT_ERROR",
            &format!("git status failed: {msg}"),
        ),
        GitResult::Unavailable(msg) => error_envelope(
            "GIT_UNAVAILABLE",
            &format!("failed to execute git: {msg}"),
        ),
    }
}

/// `GET /vcs/diff` — structured per-file diff.
/// Returns `Vcs.FileDiff[] = [{file, patch?, additions, deletions}]`.
pub async fn get_vcs_diff() -> Json<Value> {
    let dir = working_dir();
    let numstat_result = run_git(&dir, &["diff", "--numstat"]).await;
    let diff_result = run_git(&dir, &["diff"]).await;

    match (numstat_result, diff_result) {
        (GitResult::Ok(numstat), GitResult::Ok(diff)) => {
            Json(json!(parse_file_diffs(&numstat, &diff)))
        }
        (GitResult::NotARepo, _) | (_, GitResult::NotARepo) => {
            error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}"))
        }
        (GitResult::OtherError(msg), _) | (_, GitResult::OtherError(msg)) => {
            error_envelope("GIT_ERROR", &format!("git diff failed: {msg}"))
        }
        (GitResult::Unavailable(msg), _) | (_, GitResult::Unavailable(msg)) => {
            error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}"))
        }
    }
}

/// `GET /vcs/diff/raw` — raw `git diff` output as `text/x-diff`.
pub async fn get_vcs_diff_raw() -> Response {
    let dir = working_dir();
    match run_git(&dir, &["diff"]).await {
        GitResult::Ok(stdout) => {
            let mut resp = stdout.into_response();
            resp.headers_mut().insert(
                "content-type",
                "text/x-diff; charset=utf-8".parse().unwrap(),
            );
            resp
        }
        GitResult::NotARepo => {
            error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}")).into_response()
        }
        GitResult::OtherError(msg) => {
            error_envelope("GIT_ERROR", &format!("git diff failed: {msg}")).into_response()
        }
        GitResult::Unavailable(msg) => {
            error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")).into_response()
        }
    }
}

/// `POST /api/location/snapshot` — v2 spec lets the TUI tell the
/// kernel "I'm about to ask a question about this state". We log it
/// for parity and return the current project location.
pub async fn post_api_location_snapshot(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    tracing::info!(body = %body, "location snapshot");
    emit(&state, "location.snapshot", body.clone());
    Json(body)
}

#[allow(dead_code)]
pub async fn not_implemented(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "message": "not implemented" })),
    )
}

// ───────────────────────── git helpers ─────────────────────────

/// Resolve the directory these global VCS endpoints operate on: the
/// server's current working directory. Falls back to `"."` if the cwd
/// cannot be read.
fn working_dir() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

/// Categorised result of a single `git` invocation.
enum GitResult {
    /// Command succeeded; holds stdout.
    Ok(String),
    /// The target directory is not inside a git repository.
    NotARepo,
    /// git ran but returned a non-zero exit (and it was not "not a repo").
    OtherError(String),
    /// The `git` binary could not be spawned.
    Unavailable(String),
}

/// Run `git <args>` in `dir`, returning stdout on success or a
/// categorised error. Uses `tokio::process::Command` (same pattern as
/// `run_shell` in `session.rs`).
async fn run_git(dir: &str, args: &[&str]) -> GitResult {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await;
    match output {
        Ok(out) => {
            if out.status.success() {
                GitResult::Ok(String::from_utf8_lossy(&out.stdout).to_string())
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if stderr.contains("not a git repository") {
                    GitResult::NotARepo
                } else {
                    GitResult::OtherError(stderr.trim().to_string())
                }
            }
        }
        Err(error) => GitResult::Unavailable(error.to_string()),
    }
}

/// Build a JSON error envelope. The handlers return `Json<Value>` (HTTP
/// 200), so callers detect errors via the top-level `error` field rather
/// than the status code.
fn error_envelope(code: &str, message: &str) -> Json<Value> {
    Json(json!({
        "error": {
            "code": code,
            "message": message,
        }
    }))
}

/// Parse `git status --porcelain` + `git diff --numstat HEAD` into
/// `Vcs.FileStatus[] = [{file, additions, deletions, status}]`.
fn parse_file_status(status_output: &str, numstat: &str) -> Vec<Value> {
    let mut stats: std::collections::HashMap<String, (i64, i64)> =
        std::collections::HashMap::new();
    for line in numstat.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() == 3 {
            let additions = parts[0].parse::<i64>().unwrap_or(0);
            let deletions = parts[1].parse::<i64>().unwrap_or(0);
            stats.insert(parts[2].to_string(), (additions, deletions));
        }
    }

    let mut results = Vec::new();
    for line in status_output.lines() {
        if line.len() < 3 {
            continue;
        }
        let xy = &line[..2];
        let raw_path = &line[3..];
        let path_str = if let Some(rest) = raw_path.split(" -> ").nth(1) {
            rest
        } else {
            raw_path
        };
        let path = clean_path(path_str);
        let status = if xy == "??" {
            "untracked"
        } else if xy.contains('A') {
            "added"
        } else if xy.contains('D') {
            "deleted"
        } else {
            "modified"
        };
        let (additions, deletions) = stats.get(&path).copied().unwrap_or((0, 0));
        results.push(json!({
            "file": path,
            "additions": additions,
            "deletions": deletions,
            "status": status,
        }));
    }
    results
}

/// Parse `git diff --numstat` + `git diff` into
/// `Vcs.FileDiff[] = [{file, patch?, additions, deletions}]`.
fn parse_file_diffs(numstat: &str, diff: &str) -> Vec<Value> {
    let patches = split_diff_by_file(diff);

    let mut results = Vec::new();
    for line in numstat.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let additions = parts[0].parse::<i64>().unwrap_or(0);
        let deletions = parts[1].parse::<i64>().unwrap_or(0);
        let path = parts[2].to_string();

        let mut entry = json!({
            "file": path.clone(),
            "additions": additions,
            "deletions": deletions,
        });
        if let Some(patch) = patches.get(&path) {
            entry["patch"] = json!(patch);
        }
        results.push(entry);
    }
    results
}

/// Split unified diff output into a map of file path → patch text.
fn split_diff_by_file(diff: &str) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    let mut current_file: Option<String> = None;
    let mut current_lines = Vec::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(ref f) = current_file {
                result.insert(f.clone(), current_lines.join("\n"));
            }
            current_lines.clear();
            current_lines.push(line.to_string());
            if let Some(b_path) = line.split(" b/").nth(1) {
                current_file = Some(b_path.trim().to_string());
            }
        } else {
            current_lines.push(line.to_string());
        }
    }
    if let Some(ref f) = current_file {
        result.insert(f.clone(), current_lines.join("\n"));
    }
    result
}

/// Strip surrounding C-style quotes that porcelain v2 adds to paths
/// containing unusual characters. Minimal unquoting for the common case.
fn clean_path(path: &str) -> String {
    let path = path.trim();
    if path.len() >= 2 && path.starts_with('"') && path.ends_with('"') {
        path[1..path.len() - 1].to_string()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_file_status, parse_file_diffs};

    #[test]
    fn parse_file_status_maps_porcelain_codes() {
        let status = " M src/main.rs\nA  src/new.rs\nD  src/old.rs\n?? src/untracked.txt\n";
        let numstat = "3\t1\tsrc/main.rs\n5\t0\tsrc/new.rs\n";
        let result = parse_file_status(status, numstat);
        assert_eq!(result.len(), 4);

        let main = &result[0];
        assert_eq!(main["file"], "src/main.rs");
        assert_eq!(main["additions"], 3);
        assert_eq!(main["deletions"], 1);
        assert_eq!(main["status"], "modified");

        let new = &result[1];
        assert_eq!(new["file"], "src/new.rs");
        assert_eq!(new["additions"], 5);
        assert_eq!(new["deletions"], 0);
        assert_eq!(new["status"], "added");

        let old = &result[2];
        assert_eq!(old["file"], "src/old.rs");
        assert_eq!(old["status"], "deleted");

        let untracked = &result[3];
        assert_eq!(untracked["file"], "src/untracked.txt");
        assert_eq!(untracked["additions"], 0);
        assert_eq!(untracked["deletions"], 0);
        assert_eq!(untracked["status"], "untracked");
    }

    #[test]
    fn parse_file_diffs_extracts_patches() {
        let numstat = "2\t1\tsrc/main.rs\n";
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index aaa..bbb 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
 }
";
        let result = parse_file_diffs(numstat, diff);
        assert_eq!(result.len(), 1);

        let entry = &result[0];
        assert_eq!(entry["file"], "src/main.rs");
        assert_eq!(entry["additions"], 2);
        assert_eq!(entry["deletions"], 1);
        assert!(entry["patch"].is_string());
        assert!(entry["patch"].as_str().unwrap().contains("diff --git"));
    }

    #[test]
    fn empty_status_returns_empty_array() {
        let result = parse_file_status("", "");
        assert!(result.is_empty());
    }
}
