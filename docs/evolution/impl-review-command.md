# 开发方案: `loom review session` 命令

> 基于现有代码结构的具体实现方案，包含精确的文件路径、代码变更和依赖关系。

## 一、现状分析

### 已有代码

| 模块 | 文件 | 状态 |
|------|------|------|
| CLI 参数定义 | `cli/src/args.rs` | `Command::ReviewSkill(ReviewSkillArgs)` 已注册 |
| Review Agent 核心 | `cli/src/run/review.rs` | `ReviewAgent::review_session(&str) -> ReviewOutput` 已实现 |
| Review Skill 命令 | `cli/src/review_skill_cmd.rs` | `handle_review_skill_command` 已实现，读文件/stdin → 审查 |
| SessionManager | `cli/src/session.rs` | `cat_session()`, `list_sessions()`, `list_sessions_since()`, `search_sessions()` 已实现 |
| MemoryStore | `cli/src/run/memory.rs` | `MemoryStore::new()`, `append()`, `replace()` 已实现 |
| SkillRegistry | `cli/src/run/skill_registry.rs` | `SkillRegistry::new()`, `load()`, `save()` 已实现 |
| RealLlm | `cli/src/review_skill_cmd.rs` | OpenAI 兼容 API 调用已实现 |
| 命令分发 | `cli/src/main.rs:133-139` | `Cmd::ReviewSkill` 已在分发链中 |

### 关键发现

1. **现有 `review-skill` 命令已做 90% 的工作**：读内容 → 调用 `ReviewAgent` → 输出结果
2. **缺失的只是「从 session 加载内容」这一步**：目前只支持文件/stdin，不支持 session_id
3. **无审查记录持久化**：每次审查没有 history 记录，无法追踪哪些 session 已审查
4. **无批量审查**：只能单次输入，不能扫描多个 session

## 二、实现方案

### 方案选择：扩展现有 `review-skill` 命令 vs 新建 `review` 子命令

**选择：新建 `review` 子命令**，原因：
- `review-skill` 的 `ReviewSkillArgs` 只有 `--input` 和 `--model`，语义是"审查文本文件"
- 新命令需要 `session <id>`, `sessions --recent`, `history`, `pending` 等子命令，结构差异大
- 避免让已有命令过于复杂

### 架构设计

```
loom review (新命令)
    │
    ├── session <id>     ──→ SessionManager::cat_session() → 提取 messages 文本 → ReviewAgent
    ├── sessions          ──→ SessionManager::list_sessions_since() → 逐个 → ReviewAgent
    ├── history           ──→ 读取 review_history.jsonl
    ├── show <id>         ──→ 查询 review_history.jsonl
    └── pending           ──→ list_sessions() − 已审查 session_id 集合
```

## 三、具体变更

### Step 1: 新增 `ReviewArgs` 和 `ReviewCommand`（`cli/src/args.rs`）

在 `Command` enum 中新增 `Review` variant，替换原来的 `ReviewSkill`（保留兼容）：

```rust
// cli/src/args.rs — 新增

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ReviewArgs {
    #[command(subcommand)]
    pub(crate) command: ReviewCommand,

    /// Model to use for review (overrides config/env default)
    #[arg(long, value_name = "MODEL")]
    pub(crate) model: Option<String>,

    /// Verbose output: show prompt, full memory/skill content
    #[arg(long)]
    pub(crate) verbose: bool,

    /// Dry run: show what would be reviewed without calling LLM
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Only extract memory updates (skip skills)
    #[arg(long)]
    pub(crate) memory_only: bool,

    /// Only extract skill suggestions (skip memory)
    #[arg(long)]
    pub(crate) skills_only: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum ReviewCommand {
    /// Review a single session by session ID
    Session {
        /// Session ID to review
        session_id: String,
    },
    /// Batch review multiple sessions
    Sessions {
        /// Review sessions from the last N days (e.g. "7d", "30d")
        #[arg(long, value_name = "DURATION")]
        recent: Option<String>,
        /// Review all unreviewed sessions
        #[arg(long)]
        all_unreviewed: bool,
        /// Search sessions by keyword and review matches
        #[arg(long, value_name = "QUERY")]
        query: Option<String>,
        /// Maximum concurrent reviews (default: 1, serial)
        #[arg(long, default_value = "1")]
        max_concurrent: usize,
    },
    /// Show review history
    History {
        /// Filter by trigger type: manual, auto, batch
        #[arg(long)]
        trigger: Option<String>,
        /// Show last N records (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show review result for a specific session
    Show {
        /// Session ID
        session_id: String,
    },
    /// List sessions that have not been reviewed yet
    Pending {
        /// Maximum sessions to list (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}
```

同时更新 `Command` enum：

```rust
#[derive(Subcommand, Debug, Clone)]
pub(crate) enum Command {
    // ... 保留已有 ...
    
    /// Review sessions to extract skills and memory updates
    Review(ReviewArgs),
    
    /// [Deprecated] Use 'review session' instead
    ReviewSkill(ReviewSkillArgs),
}
```

### Step 2: 新增审查记录模块（`cli/src/review_history.rs`）

```rust
// cli/src/review_history.rs — 新文件

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub session_id: String,
    pub reviewed_at: DateTime<Utc>,
    pub trigger: String,  // "manual" | "auto" | "batch"
    pub model: String,
    pub memory_update_count: usize,
    pub skill_update_count: usize,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub duration_ms: u64,
}

pub struct ReviewHistory {
    path: PathBuf,
}

impl ReviewHistory {
    pub fn new(loom_home: &std::path::Path) -> Self {
        let dir = loom_home.join("data").join("review");
        let _ = fs::create_dir_all(&dir);
        Self {
            path: dir.join("history.jsonl"),
        }
    }

    pub fn append(&self, record: &ReviewRecord) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("Failed to open review history: {}", e))?;
        let line = serde_json::to_string(record)
            .map_err(|e| format!("Failed to serialize record: {}", e))?;
        writeln!(file, "{}", line)
            .map_err(|e| format!("Failed to write record: {}", e))?;
        Ok(())
    }

    pub fn list(&self, limit: usize) -> Result<Vec<ReviewRecord>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path)
            .map_err(|e| format!("Failed to open history: {}", e))?;
        let reader = std::io::BufReader::new(file);
        let records: Vec<ReviewRecord> = reader
            .lines()
            .filter_map(|line| line.ok())
            .filter_map(|line| serde_json::from_str(&line).ok())
            .collect();
        // 返回最后 N 条（倒序）
        let start = records.len().saturating_sub(limit);
        Ok(records[start..].to_vec())
    }

    pub fn reviewed_session_ids(&self) -> Result<std::collections::HashSet<String>, String> {
        let records = self.list(usize::MAX)?;
        Ok(records
            .into_iter()
            .filter(|r| !r.skipped)
            .map(|r| r.session_id)
            .collect())
    }

    pub fn find_by_session(&self, session_id: &str) -> Result<Option<ReviewRecord>, String> {
        let records = self.list(usize::MAX)?;
        Ok(records
            .into_iter()
            .rev()
            .find(|r| r.session_id == session_id))
    }
}
```

### Step 3: 新增会话内容提取器（`cli/src/review_cmd.rs`）

```rust
// cli/src/review_cmd.rs — 新文件

use crate::args::{ReviewArgs, ReviewCommand};
use crate::review_history::{ReviewHistory, ReviewRecord};
use crate::session::SessionManager;
use chrono::{Duration, Utc};
use cli::run::memory::MemoryStore;
use cli::run::review::{ReviewAgent, ReviewConfig, ReviewLlm, ReviewOutput};
use cli::run::skill_registry::SkillRegistry;
use config::home::loom_home;
use std::time::Instant;

// 复用 review_skill_cmd.rs 中的 RealLlm
// 需要将 RealLlm 和 resolve_config 提取为 pub(crate) 或直接复用
use crate::review_skill_cmd::{resolve_config_pub, RealLlm};

fn extract_session_text(session_id: &str) -> Result<String, String> {
    let mgr = SessionManager::with_default_path();
    let events = mgr.cat_session(session_id)?;
    // 将 CodexEvent 序列化为可读文本
    let mut parts = Vec::new();
    for event in &events {
        if let Some(text) = event_to_text(event) {
            parts.push(text);
        }
    }
    Ok(parts.join("\n\n"))
}

fn event_to_text(event: &stream_event::CodexEvent) -> Option<String> {
    // 提取 user/assistant/tool 的文本内容
    // 根据 CodexEvent 的实际字段结构实现
    match event {
        // UserMessage => Some(format!("User: {}", content))
        // AssistantMessage => Some(format!("Assistant: {}", content))
        // ToolResult => Some(format!("Tool({}): {}", name, content))
        _ => None,
    }
}
```

### Step 4: 实现命令处理器（`cli/src/review_cmd.rs` 续）

```rust
pub(crate) async fn handle_review_command(
    args: &ReviewArgs,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match &args.command {
        ReviewCommand::Session { session_id } => {
            review_single(session_id, args)?;
        }
        ReviewCommand::Sessions { recent, all_unreviewed, query, max_concurrent } => {
            review_batch(recent, all_unreviewed, query, *max_concurrent, args)?;
        }
        ReviewCommand::History { trigger, limit } => {
            show_history(trigger, *limit, json)?;
        }
        ReviewCommand::Show { session_id } => {
            show_review(session_id, json)?;
        }
        ReviewCommand::Pending { limit } => {
            show_pending(*limit, json)?;
        }
    }
    Ok(())
}

fn review_single(session_id: &str, args: &ReviewArgs) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let loom_home = loom_home();
    let history = ReviewHistory::new(&loom_home);

    // 1. 提取会话文本
    let text = extract_session_text(session_id)
        .map_err(|e| format!("Failed to load session '{}': {}", session_id, e))?;

    if args.dry_run {
        println!("[DRY RUN] Would review session: {}", session_id);
        println!("  Text length: {} chars", text.len());
        if args.verbose {
            println!("\n--- Session Content (first 2000 chars) ---\n");
            let preview = if text.len() > 2000 { &text[..2000] } else { &text };
            println!("{}", preview);
        }
        return Ok(());
    }

    // 2. 检查最小长度
    if text.len() < 200 {
        let record = ReviewRecord {
            session_id: session_id.to_string(),
            reviewed_at: Utc::now(),
            trigger: "manual".to_string(),
            model: String::new(),
            memory_update_count: 0,
            skill_update_count: 0,
            skipped: true,
            skip_reason: Some("insufficient_content".to_string()),
            duration_ms: start.elapsed().as_millis() as u64,
        };
        history.append(&record)?;
        println!("Skipped: session content too short ({} chars, minimum 200)", text.len());
        return Ok(());
    }

    // 3. 创建 LLM + Agent
    let (api_key, base_url, default_model) = resolve_config_pub()?;
    let model = args.model.as_deref().unwrap_or(&default_model).to_string();
    let llm = RealLlm::new(api_key, base_url, model.clone());

    let memory = MemoryStore::new(&loom_home);
    let skills_dir = loom_home.join("skills");
    let skills = SkillRegistry::new(&skills_dir);

    let config = ReviewConfig {
        auto_create_threshold: 1,
        max_session_chars: 24000,
    };
    let agent = ReviewAgent::with_config(&llm, &memory, &skills, config);

    // 4. 执行审查
    eprintln!("Reviewing session: {}", session_id);
    match agent.review_session(&text) {
        Ok(output) => {
            let record = ReviewRecord {
                session_id: session_id.to_string(),
                reviewed_at: Utc::now(),
                trigger: "manual".to_string(),
                model: model.clone(),
                memory_update_count: output.memory_updates.len(),
                skill_update_count: output.skill_suggestions.len(),
                skipped: false,
                skip_reason: None,
                duration_ms: start.elapsed().as_millis() as u64,
            };
            history.append(&record)?;

            print_review_output(&output, args.verbose);
            eprintln!("Duration: {}ms", start.elapsed().as_millis());
        }
        Err(e) => {
            let record = ReviewRecord {
                session_id: session_id.to_string(),
                reviewed_at: Utc::now(),
                trigger: "manual".to_string(),
                model: model.clone(),
                memory_update_count: 0,
                skill_update_count: 0,
                skipped: true,
                skip_reason: Some(format!("llm_error: {}", e)),
                duration_ms: start.elapsed().as_millis() as u64,
            };
            history.append(&record)?;
            return Err(e.into());
        }
    }
    Ok(())
}

fn review_batch(
    recent: &Option<String>,
    all_unreviewed: &bool,
    query: &Option<String>,
    max_concurrent: usize,
    args: &ReviewArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let loom_home = loom_home();
    let history = ReviewHistory::new(&loom_home);
    let mgr = SessionManager::with_default_path();

    // 1. 获取 session 列表
    let sessions = if let Some(q) = query {
        mgr.search_sessions(q, 100)?
    } else if let Some(dur_str) = recent {
        let days = parse_duration_days(dur_str)?;
        let since = Utc::now() - Duration::days(days as i64);
        mgr.list_sessions_since(since)?
    } else if *all_unreviewed {
        let reviewed = history.reviewed_session_ids()?;
        mgr.list_sessions()?
            .into_iter()
            .filter(|s| !reviewed.contains(&s.session_id))
            .collect()
    } else {
        return Err("Specify --recent <Nd>, --all-unreviewed, or --query <text>".into());
    };

    if sessions.is_empty() {
        println!("No sessions to review.");
        return Ok(());
    }

    eprintln!("Found {} sessions to review", sessions.len());

    let mut reviewed_count = 0;
    let mut skipped_count = 0;
    let mut total_memory = 0;
    let mut total_skills = 0;

    for (i, session) in sessions.iter().enumerate() {
        eprint!("  [{}/{}] {} — ", i + 1, sessions.len(), &session.session_id[..8]);
        
        // 复用 review_single 的逻辑
        let single_args = ReviewArgs {
            command: ReviewCommand::Session {
                session_id: session.session_id.clone(),
            },
            model: args.model.clone(),
            verbose: false,
            dry_run: args.dry_run,
            memory_only: args.memory_only,
            skills_only: args.skills_only,
        };

        match review_single(&session.session_id, &single_args) {
            Ok(()) => reviewed_count += 1,
            Err(e) => {
                eprintln!("ERROR: {}", e);
                skipped_count += 1;
            }
        }
    }

    eprintln!(
        "\nSummary: {} reviewed, {} skipped",
        reviewed_count, skipped_count
    );
    Ok(())
}

fn show_history(trigger: &Option<String>, limit: usize, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let loom_home = loom_home();
    let history = ReviewHistory::new(&loom_home);
    let records = history.list(limit)?;

    let filtered: Vec<_> = if let Some(t) = trigger {
        records.into_iter().filter(|r| r.trigger == *t).collect()
    } else {
        records
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
    } else {
        for r in &filtered {
            let status = if r.skipped { "SKIP" } else { "OK" };
            println!(
                "[{}] {} | {} | {} | mem:{} skills:{} | {}ms",
                status,
                &r.session_id[..8],
                r.reviewed_at.format("%Y-%m-%d %H:%M"),
                r.trigger,
                r.memory_update_count,
                r.skill_update_count,
                r.duration_ms,
            );
        }
    }
    Ok(())
}

fn show_review(session_id: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let loom_home = loom_home();
    let history = ReviewHistory::new(&loom_home);
    match history.find_by_session(session_id)? {
        Some(record) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&record)?);
            } else {
                println!("Session: {}", record.session_id);
                println!("Reviewed: {}", record.reviewed_at.format("%Y-%m-%d %H:%M:%S"));
                println!("Trigger: {}", record.trigger);
                println!("Model: {}", record.model);
                println!("Memory updates: {}", record.memory_update_count);
                println!("Skill updates: {}", record.skill_update_count);
                println!("Skipped: {}", record.skipped);
                if let Some(reason) = &record.skip_reason {
                    println!("Skip reason: {}", reason);
                }
                println!("Duration: {}ms", record.duration_ms);
            }
        }
        None => {
            eprintln!("No review record found for session: {}", session_id);
            std::process::exit(1);
        }
    }
    Ok(())
}

fn show_pending(limit: usize, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let loom_home = loom_home();
    let history = ReviewHistory::new(&loom_home);
    let mgr = SessionManager::with_default_path();

    let reviewed = history.reviewed_session_ids()?;
    let all = mgr.list_sessions()?;
    let pending: Vec<_> = all
        .into_iter()
        .filter(|s| !reviewed.contains(&s.session_id))
        .take(limit)
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&pending)?);
    } else {
        if pending.is_empty() {
            println!("All sessions have been reviewed.");
        } else {
            println!("{} pending sessions:", pending.len());
            for s in &pending {
                let title = s.title.as_deref().unwrap_or("(untitled)");
                let time = s.last_updated
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                println!("  {} | {} | steps:{}", &s.session_id[..8], time, s.latest_step);
            }
        }
    }
    Ok(())
}

fn parse_duration_days(s: &str) -> Result<usize, String> {
    let s = s.trim().to_lowercase();
    if let Some(num) = s.strip_suffix('d') {
        num.parse::<usize>()
            .map_err(|e| format!("Invalid duration '{}': {}", s, e))
    } else {
        s.parse::<usize>()
            .map_err(|e| format!("Invalid duration '{}': expected 'Nd' format (e.g. '7d')", s))
    }
}

fn print_review_output(output: &ReviewOutput, verbose: bool) {
    if output.memory_updates.is_empty() && output.skill_suggestions.is_empty() {
        println!("  No updates extracted.");
        return;
    }

    for update in &output.memory_updates {
        println!("  + {} ({}): {} chars", update.action, update.file, update.content.len());
        if verbose && update.content.len() <= 300 {
            println!("    {}", update.content);
        }
    }

    for skill in &output.skill_suggestions {
        println!("  ~ {}: {}", skill.name, skill.description);
        if verbose {
            println!("    Triggers: {:?}", skill.triggers);
        }
    }
}
```

### Step 5: 提取公共 RealLlm（修改 `cli/src/review_skill_cmd.rs`）

将 `RealLlm` 和 `resolve_config` 改为 `pub(crate)`：

```rust
// 修改 RealLlm 和 resolve_config 的可见性
pub(crate) struct RealLlm { ... }

impl RealLlm {
    pub(crate) fn new(api_key: String, base_url: String, model: String) -> Self { ... }
}

pub(crate) fn resolve_config_pub() -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // 原 resolve_config 的逻辑
}
```

### Step 6: 注册模块和命令分发

**`cli/src/main.rs`** 新增：

```rust
mod review_cmd;    // 新增

use subcommands::{
    // ... 已有 ...
};

// 在分发链中新增：
if let Some(Cmd::Review(ra)) = &args.cmd {
    if let Err(err) = review_cmd::handle_review_command(ra, args.json).await {
        eprintln!("{}", err);
        std::process::exit(1);
    }
    return Ok(());
}
```

### Step 7: Session 文本提取（关键细节）

`SessionManager::cat_session()` 返回 `Vec<CodexEvent>`，需要转为纯文本。查看 `codex_event_builder.rs` 确认 CodexEvent 结构：

```rust
// 方案 A：使用 CodexEvent 的 text 字段（如果有）
// 方案 B：直接从 ReActState 的 messages 提取

fn extract_session_text(session_id: &str) -> Result<String, String> {
    let mgr = SessionManager::with_default_path();
    
    // 直接从 DB 加载 ReActState，不走 CodexEvent 转换
    let conn = rusqlite::Connection::open(&mgr.db_path())
        .map_err(|e| format!("Failed to open database: {}", e))?;
    
    let mut stmt = conn.prepare(
        "SELECT payload FROM checkpoints WHERE thread_id = ?1 ORDER BY metadata_created_at ASC"
    ).map_err(|e| format!("Failed to prepare: {}", e))?;
    
    let payloads: Vec<Vec<u8>> = stmt.query_map([session_id], |row| row.get(0))
        .map_err(|e| format!("Query failed: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    if payloads.is_empty() {
        return Err(format!("Session not found: {}", session_id));
    }

    let mut parts = Vec::new();
    for data in &payloads {
        if let Ok(state) = serde_json::from_slice::<loom::state::ReActState>(data) {
            for msg in &state.messages {
                match msg {
                    loom::message::Message::User(u) => {
                        parts.push(format!("User: {}", u.as_text()));
                    }
                    loom::message::Message::Assistant(a) => {
                        parts.push(format!("Assistant: {}", a.content));
                    }
                    loom::message::Message::Tool { content, .. } => {
                        if let Some(text) = content.as_text() {
                            parts.push(format!("Tool: {}", text));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(parts.join("\n"))
}
```

> 注意：`SessionManager::db_path` 是私有字段，需要新增 `pub fn db_path(&self) -> &Path` 或直接在 `SessionManager` 上新增 `extract_text(session_id)` 方法。

## 四、文件变更清单

| 文件 | 操作 | 变更内容 |
|------|------|----------|
| `cli/src/args.rs` | 修改 | 新增 `ReviewArgs`, `ReviewCommand`；`Command` enum 新增 `Review` variant |
| `cli/src/review_cmd.rs` | **新增** | 命令处理器：review_single, review_batch, show_history, show_pending, extract_session_text |
| `cli/src/review_history.rs` | **新增** | `ReviewRecord`, `ReviewHistory` — JSONL 持久化 |
| `cli/src/review_skill_cmd.rs` | 修改 | `RealLlm` 和 `resolve_config` 改为 `pub(crate)` |
| `cli/src/main.rs` | 修改 | 新增 `mod review_cmd`，分发链新增 `Cmd::Review` |
| `cli/src/session.rs` | 修改 | 新增 `pub fn extract_session_text(&self, session_id: &str)` 方法 |
| `docs/evolution/commands.md` | 修改 | 补充 `review` 命令文档 |

## 五、实施顺序

```
Step 1 — review_history.rs（独立，无依赖）
    ↓
Step 2 — session.rs: extract_session_text()（依赖现有 SessionManager）
    ↓
Step 3 — review_skill_cmd.rs: 提取 pub RealLlm（小改动）
    ↓
Step 4 — args.rs: ReviewArgs + ReviewCommand（纯数据定义）
    ↓
Step 5 — review_cmd.rs: 核心命令处理（依赖 Step 1-4）
    ↓
Step 6 — main.rs: 注册模块 + 分发（集成）
    ↓
Step 7 — 编译 + 测试
    ↓
Step 8 — commands.md 文档更新
```

## 六、测试计划

### 单元测试

| 测试 | 位置 | 内容 |
|------|------|------|
| `parse_duration_days` | review_cmd.rs | "7d" → 7, "30d" → 30, "abc" → error |
| `ReviewHistory::append + list` | review_history.rs | 写入记录 → 读回 → 验证字段 |
| `ReviewHistory::reviewed_session_ids` | review_history.rs | 写入混合记录 → 返回未跳过的 session_id 集合 |
| `extract_session_text` | session.rs | mock DB → 提取 messages → 验证文本格式 |

### 集成测试（手动）

```bash
# 1. 查看待审查会话
loom review pending

# 2. 试运行单个会话
loom review session <id> --dry-run --verbose

# 3. 审查单个会话
loom review session <id>

# 4. 批量审查最近 7 天
loom review sessions --recent 7d

# 5. 查看历史
loom review history
loom review show <id>

# 6. JSON 输出
loom review pending --json
loom review history --json
```

## 七、风险与缓解

| 风险 | 缓解 |
|------|------|
| `cat_session()` 返回 `CodexEvent` 无法直接提取文本 | 直接从 SQLite payload 反序列化 `ReActState`，绕过 CodexEvent |
| `SessionManager.db_path` 是私有字段 | 在 SessionManager 上新增 `extract_text()` 公共方法 |
| 大 session 导致 prompt 过长 | 已有 `max_session_chars: 24000` 截断 |
| 批量审查成本不可控 | 默认串行，每轮显示进度，后续可加 `--max-cost` |

## 八、工作量预估

| 任务 | 预估 |
|------|------|
| review_history.rs | 2h |
| session.rs 扩展 | 2h |
| review_skill_cmd.rs 提取公共部分 | 0.5h |
| args.rs 参数定义 | 1h |
| review_cmd.rs 核心逻辑 | 4h |
| main.rs 集成 | 0.5h |
| 测试 | 2h |
| 文档 | 1h |
| **合计** | **~13h (1.5-2 天)** |
