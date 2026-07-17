//! Git operations handlers for `/git/*` routes.
//!
//! Provides check, stage, unstage, commit, log, and branch listing.
//! Reuses the same `run_git` + `GitResult` + `error_envelope` pattern
//! as `vcs_extra.rs`, operating on the server's current working directory.

use axum::{
    http::StatusCode,

    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

// ───────────────────────── helpers (mirrors vcs_extra.rs) ─────────────────────────

fn working_dir() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

enum GitResult {
    Ok(String),
    NotARepo,
    OtherError(String),
    Unavailable(String),
}

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

fn error_envelope(code: &str, message: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": { "code": code, "message": message }
        })),
    )
}

// ───────────────────────── GET /git/check ─────────────────────────

pub async fn check() -> (StatusCode, Json<Value>) {
    let dir = working_dir();
    match run_git(&dir, &["rev-parse", "--is-inside-work-tree"]).await {
        GitResult::Ok(stdout) => {
            let is_repo = stdout.trim() == "true";
            (StatusCode::OK, Json(json!({ "isRepo": is_repo })))
        }
        GitResult::NotARepo => (StatusCode::OK, Json(json!({ "isRepo": false }))),
        GitResult::OtherError(msg) => error_envelope("GIT_ERROR", &format!("git check failed: {msg}")),
        GitResult::Unavailable(msg) => error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")),
    }
}

// ───────────────────────── POST /git/stage ─────────────────────────

#[derive(Deserialize)]
pub struct PathsBody {
    pub paths: Vec<String>,
}

pub async fn stage(Json(body): Json<PathsBody>) -> (StatusCode, Json<Value>) {
    let dir = working_dir();
    let mut args: Vec<&str> = vec!["add", "--"];
    let owned: Vec<String> = body.paths.iter().map(|s| s.clone()).collect();
    for p in &owned {
        args.push(p.as_str());
    }
    match run_git(&dir, &args).await {
        GitResult::Ok(_) => (StatusCode::OK, Json(json!({ "staged": owned }))),
        GitResult::NotARepo => error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}")),
        GitResult::OtherError(msg) => error_envelope("GIT_ERROR", &format!("git add failed: {msg}")),
        GitResult::Unavailable(msg) => error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")),
    }
}

// ───────────────────────── POST /git/unstage ─────────────────────────

pub async fn unstage(Json(body): Json<PathsBody>) -> (StatusCode, Json<Value>) {
    let dir = working_dir();
    let mut args: Vec<&str> = vec!["reset", "HEAD", "--"];
    let owned: Vec<String> = body.paths.iter().map(|s| s.clone()).collect();
    for p in &owned {
        args.push(p.as_str());
    }
    match run_git(&dir, &args).await {
        GitResult::Ok(_) => (StatusCode::OK, Json(json!({ "unstaged": owned }))),
        GitResult::NotARepo => error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}")),
        GitResult::OtherError(msg) => error_envelope("GIT_ERROR", &format!("git reset failed: {msg}")),
        GitResult::Unavailable(msg) => error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")),
    }
}

// ───────────────────────── POST /git/commit ─────────────────────────

#[derive(Deserialize)]
pub struct CommitBody {
    pub message: String,
}

pub async fn commit(Json(body): Json<CommitBody>) -> (StatusCode, Json<Value>) {
    let dir = working_dir();
    match run_git(&dir, &["commit", "-m", &body.message]).await {
        GitResult::Ok(stdout) => {
            let hash = match run_git(&dir, &["rev-parse", "HEAD"]).await {
                GitResult::Ok(h) => h.trim().to_string(),
                _ => String::new(),
            };
            (StatusCode::OK, Json(json!({
                "hash": hash,
                "message": body.message,
                "output": stdout.trim(),
            })))
        }
        GitResult::NotARepo => error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}")),
        GitResult::OtherError(msg) => error_envelope("GIT_ERROR", &format!("git commit failed: {msg}")),
        GitResult::Unavailable(msg) => error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")),
    }
}

// ───────────────────────── GET /git/log ─────────────────────────

#[derive(Deserialize, Default)]
pub struct LogQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

pub async fn log(
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> (StatusCode, Json<Value>) {
    let dir = working_dir();
    let limit = q.limit.unwrap_or(20).min(500);
    let fmt = "%H%x1f%an%x1f%ad%x1f%s";
    let limit_str = limit.to_string();
    match run_git(
        &dir,
        &["log", &format!("--format={fmt}"), "--date=iso", "-n", &limit_str],
    )
    .await
    {
        GitResult::Ok(stdout) => {
            let commits: Vec<Value> = stdout
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let parts: Vec<&str> = line.split('\u{1f}').collect();
                    let hash = parts.first().unwrap_or(&"").to_string();
                    let author = parts.get(1).unwrap_or(&"").to_string();
                    let date = parts.get(2).unwrap_or(&"").to_string();
                    let message = parts.get(3).unwrap_or(&"").to_string();
                    json!({
                        "hash": hash,
                        "author": author,
                        "date": date,
                        "message": message,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "commits": commits })))
        }
        GitResult::NotARepo => error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}")),
        GitResult::OtherError(msg) => error_envelope("GIT_ERROR", &format!("git log failed: {msg}")),
        GitResult::Unavailable(msg) => error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")),
    }
}

// ───────────────────────── GET /git/branches ─────────────────────────

pub async fn branches() -> (StatusCode, Json<Value>) {
    let dir = working_dir();
    let current = match run_git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).await {
        GitResult::Ok(c) => c.trim().to_string(),
        _ => "(unknown)".to_string(),
    };
    let fmt = "%(refname:short)%x1f%(objecttype)%x1f%(upstream:short)%x1f%(HEAD)";
    match run_git(&dir, &["for-each-ref", &format!("--format={fmt}"), "refs/heads/", "refs/remotes/"]).await {
        GitResult::Ok(stdout) => {
            let mut branches: Vec<Value> = Vec::new();
            for line in stdout.lines() {
                if line.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = line.split('\u{1f}').collect();
                let name = parts.first().unwrap_or(&"").to_string();
                let is_head = parts.get(3).unwrap_or(&"").trim() == "*";
                let is_remote = name.contains('/');
                branches.push(json!({
                    "name": name,
                    "isCurrent": is_head,
                    "isRemote": is_remote,
                }));
            }
            (StatusCode::OK, Json(json!({ "branches": branches, "current": current })))
        }
        GitResult::NotARepo => error_envelope("NOT_A_REPO", &format!("not a git repository: {dir}")),
        GitResult::OtherError(msg) => error_envelope("GIT_ERROR", &format!("git branch failed: {msg}")),
        GitResult::Unavailable(msg) => error_envelope("GIT_UNAVAILABLE", &format!("failed to execute git: {msg}")),
    }
}
