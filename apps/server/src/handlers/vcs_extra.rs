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

/// `GET /vcs/status` — real repository status summary, parsed from
/// `git status --porcelain=v2 -b`. Branch name is corroborated (and
/// recovered if missing) via `git rev-parse --abbrev-ref HEAD`.
pub async fn get_vcs_status() -> Json<Value> {
    let dir = working_dir();
    match run_git(&dir, &["status", "--porcelain=v2", "-b"]).await {
        GitResult::Ok(stdout) => {
            let mut value = parse_porcelain_status(&stdout);
            // Porcelain v2 always emits `# branch.head`, but recover the
            // branch name via an explicit `git rev-parse --abbrev-ref HEAD`
            // if the parser couldn't find one (returns "(unknown)").
            if value["branch"].as_str() == Some("(unknown)") {
                if let GitResult::Ok(name) =
                    run_git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await
                {
                    let name = name.trim();
                    if !name.is_empty() {
                        value["branch"] = json!(name);
                    }
                }
            }
            Json(value)
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

/// `GET /vcs/diff` — textual diff, combining unstaged (`git diff`) and
/// staged (`git diff --cached`) changes.
pub async fn get_vcs_diff() -> Json<Value> {
    let dir = working_dir();
    let unstaged = run_git(&dir, &["diff"]).await;
    let staged = run_git(&dir, &["diff", "--cached"]).await;

    match (unstaged, staged) {
        (GitResult::Ok(u), GitResult::Ok(s)) => {
            let combined = format!("{u}{s}");
            Json(json!({
                "diff": combined.trim(),
                "unstaged": u.trim(),
                "staged": s.trim(),
            }))
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

/// `GET /vcs/diff/raw` — raw (`--raw`) diff output for unstaged changes,
/// plus the staged raw diff for completeness.
pub async fn get_vcs_diff_raw() -> Json<Value> {
    let dir = working_dir();
    match run_git(&dir, &["diff", "--raw"]).await {
        GitResult::Ok(stdout) => {
            let staged = match run_git(&dir, &["diff", "--cached", "--raw"]).await {
                GitResult::Ok(s) => s,
                _ => String::new(),
            };
            Json(json!({
                "diff": stdout.trim(),
                "staged": staged.trim(),
            }))
        }
        GitResult::NotARepo => {
            error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}"))
        }
        GitResult::OtherError(msg) => {
            error_envelope("GIT_ERROR", &format!("git diff --raw failed: {msg}"))
        }
        GitResult::Unavailable(msg) => {
            error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}"))
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

/// Parse `git status --porcelain=v2 -b` output into the status envelope.
///
/// Header lines (`# branch.*`) carry branch, upstream, and ahead/behind.
/// Entry lines classify changes by their two-character `XY` field:
/// `X` is the index (staged) status, `Y` the worktree (unstaged) status.
/// A value of `.` means "unchanged" for that side.
fn parse_porcelain_status(stdout: &str) -> Value {
    let mut branch = String::new();
    let mut ahead: i64 = 0;
    let mut behind: i64 = 0;
    let mut staged: Vec<String> = Vec::new();
    let mut modified: Vec<String> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();

    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            branch = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // format: `+<ahead> -<behind>`
            for token in rest.split_whitespace() {
                if let Some(n) = token.strip_prefix('+') {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = token.strip_prefix('-') {
                    behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(path) = line.strip_prefix("? ") {
            untracked.push(clean_path(path));
        } else if line.starts_with('1')
            || line.starts_with('2')
            || line.starts_with('u')
        {
            // Changed entry: `1`/`2`/`u` ordinary/renamed/unmerged.
            if let Some((x, y)) = entry_xy(line) {
                if let Some(path) = entry_path(line) {
                    if x != '.' && x != ' ' {
                        staged.push(path.clone());
                    }
                    if y != '.' && y != ' ' {
                        modified.push(path);
                    }
                }
            }
        }
    }

    let dirty = !(staged.is_empty() && modified.is_empty() && untracked.is_empty());
    let branch_value = if branch.is_empty() {
        "(unknown)".to_string()
    } else {
        branch
    };

    json!({
        "dirty": dirty,
        "branch": branch_value,
        "ahead": ahead,
        "behind": behind,
        "modified": modified,
        "staged": staged,
        "untracked": untracked,
    })
}

/// Extract the two-character `XY` status field from a porcelain v2 entry
/// line (`1`/`2`/`u`). `X` = index status, `Y` = worktree status.
fn entry_xy(line: &str) -> Option<(char, char)> {
    let mut parts = line.splitn(3, ' ');
    parts.next()?; // entry kind
    let xy = parts.next()?;
    let chars: Vec<char> = xy.chars().collect();
    if chars.len() >= 2 {
        Some((chars[0], chars[1]))
    } else {
        None
    }
}

/// Extract the path field from a porcelain v2 entry line. The field
/// index depends on the entry kind (`1`/`2`/`u`); using `splitn` keeps
/// the remainder intact so paths containing spaces are preserved.
fn entry_path(line: &str) -> Option<String> {
    let mut parts = line.splitn(2, ' ');
    let kind = parts.next()?;
    let path = match kind {
        "1" => line.splitn(9, ' ').nth(8)?,
        "2" => line.splitn(10, ' ').nth(9)?,
        "u" => line.splitn(13, ' ').nth(12)?,
        _ => return None,
    };
    // For renamed/copied entries (type 2), porcelain v2 appends the
    // original path after a TAB (`<path>\t<origPath>`). Keep the
    // current path only.
    let path = path.split('\t').next().unwrap_or(path);
    Some(clean_path(path))
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
    use super::parse_porcelain_status;

    #[test]
    fn parses_branch_ahead_behind_and_changes() {
        let output = "\
# branch.oid 2117e71a763dbc73706bf04b18ad66587f5d3d3b
# branch.head feature/cli-server-backend
# branch.upstream origin/feature/cli-server-backend
# branch.ab +2 -1
1 .M N... 100644 100644 100644 aaaa bbbb apps/server/Cargo.toml
1 M. N... 100644 100644 100644 cccc dddd apps/server/src/lib.rs
2 RM N... 100644 100644 100644 eeee ffff R100 apps/server/src/new.rs\tapps/server/src/old.rs
? apps/server/untracked.txt
";
        let value = parse_porcelain_status(output);
        assert_eq!(value["branch"], "feature/cli-server-backend");
        assert_eq!(value["ahead"], 2);
        assert_eq!(value["behind"], 1);
        assert_eq!(value["dirty"], true);
        // Worktree-modified (Y = 'M') entries.
        let modified: Vec<&str> = value["modified"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(modified.contains(&"apps/server/Cargo.toml"));
        // Staged (X = 'M'/'R') entries.
        let staged: Vec<&str> = value["staged"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(staged.contains(&"apps/server/src/lib.rs"));
        // Renamed entry: current path lands in the index (staged) side.
        assert!(staged.contains(&"apps/server/src/new.rs"));
        // Untracked.
        let untracked: Vec<&str> = value["untracked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(untracked.contains(&"apps/server/untracked.txt"));
    }

    #[test]
    fn clean_repo_is_not_dirty() {
        let output = "\
# branch.oid 2117e71a763dbc73706bf04b18ad66587f5d3d3b
# branch.head main
";
        let value = parse_porcelain_status(output);
        assert_eq!(value["branch"], "main");
        assert_eq!(value["dirty"], false);
        assert_eq!(value["ahead"], 0);
        assert_eq!(value["behind"], 0);
        assert!(value["modified"].as_array().unwrap().is_empty());
        assert!(value["staged"].as_array().unwrap().is_empty());
        assert!(value["untracked"].as_array().unwrap().is_empty());
    }

    #[test]
    fn detached_head_branch_is_preserved() {
        let output = "\
# branch.oid 2117e71a763dbc73706bf04b18ad66587f5d3d3b
# branch.head (detached)
";
        let value = parse_porcelain_status(output);
        assert_eq!(value["branch"], "(detached)");
    }
}
