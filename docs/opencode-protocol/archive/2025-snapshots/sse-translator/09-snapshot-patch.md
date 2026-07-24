# Snapshot / Patch 机制

> 返回 [README.md](README.md)

## 9.1 目的

> 开发任务：X6（step-finish 生成 patch part）

OpenCode 和 Loom 在 step-finish 时生成 `patch` part，用于：

| 用途 | 说明 |
|---|---|
| UI diff 展示 | TUI/IDE 渲染本回合修改的文件清单 |
| 撤销/回滚 | 用户可选择撤销某个 step 的全部改动 |
| 审计 | 记录哪个 LLM 调用回合改了哪些文件 |

## 9.2 OpenCode 实现（`packages/opencode/src/snapshot/index.ts`，约 500 行）

### 9.2.1 核心架构：Shadow Git DB

OpenCode **不直接对比**当前文件状态与起始状态——它为每个 project+worktree 维护一个**独立的影子 git 仓库**：

```
~/.local/share/opencode/snapshot/
  └── <project_id>/
      └── <worktree-hash>/           ← 影子 git DB（独立 .git）
          ├── objects/                ← 文件内容对象
          ├── index                   ← 文件索引（树快照）
          └── info/exclude            ← gitignore 替代品
```

**为什么用 git 而不是自定义快照**：
- 文件变更检测、内容 hash、树 hash 都是 git 现成的
- 自带 ignore 机制（`check-ignore`）
- `git write-tree` 返回的 hash 天然适合作为快照标识
- `git checkout <hash> -- <file>` 原生支持 revert
- 复用 git 的对象压缩/去重

### 9.2.2 `track()` 流程（`step-start` 调用）

```typescript
track: () => Effect.Effect<string | undefined>
```

```typescript
const track = Effect.fnUntraced(function* () {
  // 1. 检查是否启用（git 项目 + config.snapshot !== false）
  if (!(yield* enabled())) return
  
  // 2. 首次初始化影子仓库
  if (!existed) {
    yield* git(["init"])
    yield* git(["config", "core.autocrlf", "false"])
    yield* git(["config", "feature.manyFiles", "true"])  // 大仓库优化
    yield* git(["config", "index.version", "4"])
    yield* git(["config", "index.threads", "true"])
    yield* seed()  // ← 关键优化：复用源仓库的 index + alternates
  }
  
  // 3. 扫描变更并暂存
  yield* add()
  
  // 4. 返回 tree hash 作为快照标识
  return git(["write-tree"]).text.trim()
})
```

**`add()` 细节**：
```typescript
// 同时跑两个命令（concurrency: 2）
git diff-files --name-only -z -- .         // 已跟踪的变更
git ls-files --full-name --others \
    --exclude-standard -z -- .              // 新增的未跟踪

// 应用 .gitignore 过滤
git check-ignore --no-index --stdin -z

// 跳过 > 2MB 的文件（limit = 2 * 1024 * 1024）
fs.stat() → 大文件标记

// 暂存
git add --all --sparse --pathspec-from-file=- --pathspec-file-nul
```

**`seed()` 优化**（注释解释）：
> "在大型仓库（如 chromium checkout）中，`git add --all` 重建哈希需要几分钟。
> 通过共享源仓库的对象数据库（alternates），可以完全消除这一步。"

### 9.2.3 `patch(hash)` 流程（`step-finish` 调用）

```typescript
patch: (hash: string) => Effect.Effect<Patch>
```

```typescript
const patch = Effect.fnUntraced(function* (hash) {
  yield* add()  // 重新扫描当前状态（包括 step 期间的变更）
  
  const result = yield* git([
    ...quote,
    ...args(["diff", "--cached", "--no-ext-diff", "--name-only", hash, "--", "."])
  ])
  
  const files = result.text.trim().split("\n").filter(Boolean)
  const ignored = yield* ignore(files)  // 过滤 ignore 文件
  
  return {
    hash,                    // 当前 tree hash（下次 patch 时的基线）
    files: files
      .filter((f) => !ignored.has(f))
      .map((f) => path.join(worktree, f).replaceAll("\\", "/"))
  }
})
```

**输出**：
```typescript
export const Patch = Schema.Struct({
  hash: Schema.String,                       // git tree hash
  files: Schema.mutable(Schema.Array(Schema.String)),  // 绝对路径列表
})
```

processor.ts 中对应的 part：
```typescript
yield* session.updatePart({
  id: PartID.ascending(), messageID, sessionID,
  type: "patch", hash: patch.hash, files: patch.files,
})
```

### 9.2.4 其他能力（Loom 暂不实现）

**`revert(patches[])`** —— 真正的回滚：
```typescript
// 对每个 file: git checkout <hash> -- <file>
// 失败回退到逐文件 checkout
// 快照中不存在的文件 → 删除（snapshot 时未跟踪 = 不应存在）
```

**`restore(snapshot)`** —— 完整恢复到某次快照：
```typescript
git read-tree <snapshot>
git checkout-index -a -f
```

**`diff(hash)`** —— 完整 unified diff（用于 TUI 显示）：
```typescript
git diff --cached --no-ext-diff <hash> -- .
// 用 `diff` npm 包的 structuredPatch + formatPatch 生成 patch 字符串
```

**`diffFull(from, to)`** —— 任意两个快照之间的完整 diff（含 additions/deletions）：
```typescript
git diff --name-status <from> <to>
git diff --numstat <from> <to>   // 数字统计
git cat-file --batch             // 批量读取 blob 内容
// 用 diff 库生成 unified diff 文本
```

**后台 GC**：
```typescript
yield* cleanup().pipe(
  Effect.catchCause(...),
  Effect.repeat(Schedule.spaced(Duration.hours(1))),
  Effect.delay(Duration.minutes(1)),
  Effect.forkScoped,
)
// 内部：git gc --prune=7.days
```

## 9.3 Loom 实现（精简版，约 100 行）

### 9.3.1 核心原则

复用 Loom 已有 `git_ops`（`experimental/worktree/src/git_ops.rs`），不重新发明 git 调用。

### 9.3.2 文件结构

```
apps/server/src/snapshot/
  mod.rs       ← SnapshotService trait + DI
  git_snap.rs  ← 基于 git 的实现（首选）
```

数据存储：`Global.Path.data/snapshot/<project_id>/<worktree-hash>/`（与 OpenCode 同布局）

### 9.3.3 `SnapshotService` 接口

```rust
// apps/server/src/snapshot/mod.rs

#[async_trait]
pub trait SnapshotService: Send + Sync {
    /// 在 step-start 调用：拍摄当前文件状态快照，返回 tree hash
    async fn track(&self, session_id: &str, worktree: &Path) -> Result<Option<String>>;
    
    /// 在 step-finish 调用：对比 hash 与当前状态，返回变更文件列表
    async fn patch(&self, session_id: &str, worktree: &Path, hash: &str)
        -> Result<PatchInfo>;
    
    /// 未来用：恢复到某个快照
    async fn restore(&self, session_id: &str, worktree: &Path, hash: &str)
        -> Result<()>;
}

pub struct PatchInfo {
    pub hash: String,                // 当前 tree hash
    pub files: Vec<PathBuf>,         // 变更的绝对路径列表
}
```

### 9.3.4 基于 git 的实现

```rust
// apps/server/src/snapshot/git_snap.rs

use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::process::Command as AsyncCommand;
use crate::git_ops;

pub struct GitSnapshotService {
    data_root: PathBuf,
    worktree_hashes: parking_lot::RwLock<HashMap<PathBuf, String>>,
    /// per-gitdir mutex：序列化所有 git 操作，防止并发损坏 index
    /// （等价于 OpenCode 的 `lock(gitdir)` Semaphore）
    locks: Arc<RwLock<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>>,
}

impl GitSnapshotService {
    pub fn new(data_root: PathBuf) -> Self {
        Self {
            data_root,
            worktree_hashes: Default::default(),
            locks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn gitdir(&self, session_id: &str, worktree_hash: &str) -> PathBuf {
        self.data_root
            .join("snapshot")
            .join(session_id)
            .join(worktree_hash)
    }

    /// 获取/创建 per-gitdir 互斥锁。
    /// 所有对同一 gitdir 的 `git add` / `write-tree` / `diff` 操作都需在同一锁内串行执行。
    async fn with_lock<F, T>(&self, gitdir: &Path, f: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let lock = {
            let mut locks = self.locks.write();
            locks
                .entry(gitdir.to_path_buf())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        f.await
    }

    fn hash_worktree(worktree: &Path) -> String {
        // 用 blake3 哈希绝对路径，作为 gitdir 的命名空间
        let canonical = worktree.canonicalize().unwrap_or_else(|_| worktree.to_path_buf());
        let hash = blake3::hash(canonical.to_string_lossy().as_bytes());
        hex::encode(&hash.as_bytes()[..8])
    }
}

#[async_trait]
impl SnapshotService for GitSnapshotService {
    async fn track(&self, session_id: &str, worktree: &Path) -> Result<Option<String>> {
        self.with_lock(worktree, async {
            self.track_locked(session_id, worktree).await
        }).await
    }

    async fn patch(&self, session_id: &str, worktree: &Path, hash: &str)
        -> Result<PatchInfo>
    {
        self.with_lock(worktree, async {
            self.patch_locked(session_id, worktree, hash).await
        }).await
    }

    async fn restore(&self, session_id: &str, worktree: &Path, hash: &str)
        -> Result<()>
    {
        self.with_lock(worktree, async {
            self.restore_locked(session_id, worktree, hash).await
        }).await
    }
}

impl GitSnapshotService {
    /// track() 的实际实现——必须在 with_lock 内调用
    async fn track_locked(&self, session_id: &str, worktree: &Path) -> Result<Option<String>> {
        // 跳过非 git 项目
        if !worktree.join(".git").exists() { return Ok(None); }

        let worktree_hash = Self::hash_worktree(worktree);
        let gitdir = self.gitdir(session_id, &worktree_hash);

        // 首次初始化
        if !gitdir.join(".git").exists() {
            tokio::fs::create_dir_all(&gitdir).await?;
            run_git(&gitdir, &["init"]).await?;
            // 基础配置
            run_git(&gitdir, &["config", "core.autocrlf", "false"]).await?;
            run_git(&gitdir, &["config", "core.longpaths", "true"]).await?;
            run_git(&gitdir, &["config", "core.symlinks", "true"]).await?;
            run_git(&gitdir, &["config", "core.fsmonitor", "false"]).await?;  // 与 shadow git 不兼容
            // 性能配置（OpenCode 的 large-files 优化）
            run_git(&gitdir, &["config", "feature.manyFiles", "true"]).await?;
            run_git(&gitdir, &["config", "index.version", "4"]).await?;
            run_git(&gitdir, &["config", "index.threads", "true"]).await?;
            run_git(&gitdir, &["config", "core.untrackedCache", "true"]).await?;
        }

        // add：扫描变更 + 暂存
        self.add(worktree, &gitdir).await?;

        // write-tree → 返回 tree hash
        let hash = run_git_stdout(&gitdir, &["write-tree"]).await?;
        Ok(Some(hash.trim().to_string()))
    }

    async fn patch_locked(&self, session_id: &str, worktree: &Path, hash: &str)
        -> Result<PatchInfo>
    {
        let worktree_hash = Self::hash_worktree(worktree);
        let gitdir = self.gitdir(session_id, &worktree_hash);

        // 重新扫描当前状态
        self.add(worktree, &gitdir).await?;

        // diff --cached --name-only <hash>
        let output = run_git_stdout(
            &gitdir,
            &["diff", "--cached", "--no-ext-diff", "--name-only", hash, "--", "."],
        ).await?;

        let files: Vec<PathBuf> = output
            .lines()
            .filter(|s| !s.is_empty())
            .map(|rel| worktree.join(rel).canonicalize().unwrap_or_else(|_| worktree.join(rel)))
            .collect();

        // 获取新的 tree hash
        let new_hash = run_git_stdout(&gitdir, &["write-tree"]).await?;

        Ok(PatchInfo { hash: new_hash.trim().to_string(), files })
    }

    async fn restore_locked(&self, session_id: &str, worktree: &Path, hash: &str)
        -> Result<()>
    {
        let worktree_hash = Self::hash_worktree(worktree);
        let gitdir = self.gitdir(session_id, &worktree_hash);

        run_git(&gitdir, &["read-tree", hash]).await?;
        run_git(&gitdir, &["checkout-index", "-a", "-f"]).await?;
        Ok(())
    }
}

impl GitSnapshotService {
    /// add：扫描 + 暂存所有变更文件
    async fn add(&self, worktree: &Path, gitdir: &Path) -> Result<()> {
        let worktree_str = worktree.to_string_lossy().to_string();
        let gitdir_str = gitdir.to_string_lossy().to_string();

        // git diff-files --name-only -z -- .   （已跟踪的变更）
        // git ls-files --others --exclude-standard -z -- .  （新增的）
        // 并发跑两个命令
        let (tracked, untracked) = tokio::try_join!(
            run_git_stdout_with_env(
                gitdir, worktree,
                &["--git-dir", &gitdir_str, "--work-tree", &worktree_str,
                  "diff-files", "--name-only", "-z", "--", "."],
            ),
            run_git_stdout_with_env(
                gitdir, worktree,
                &["--git-dir", &gitdir_str, "--work-tree", &worktree_str,
                  "ls-files", "--full-name", "--others", "--exclude-standard", "-z", "--", "."],
            ),
        )?;

        // 合并去重 + 解析 NUL 分隔
        let mut all: Vec<String> = tracked.split('\0')
            .filter(|s| !s.is_empty())
            .chain(untracked.split('\0').filter(|s| !s.is_empty()))
            .map(String::from)
            .collect();
        all.sort();
        all.dedup();
        if all.is_empty() { return Ok(()); }

        // 应用 .gitignore 过滤（git check-ignore）
        let ignored = self.filter_ignored(worktree, &all).await?;

        // 关键：移除快照 index 中已 ignore 的文件
        // 否则用户后续修改 .gitignore 不会生效，patch() 会误报这些文件
        // （等价于 OpenCode 的 `drop(ignoredFiles)`，snapshot/index.ts:148-157）
        if !ignored.is_empty() {
            let ignored_vec: Vec<String> = ignored.iter().cloned().collect();
            self.drop_from_index(&gitdir, &ignored_vec).await?;
        }

        let allow: Vec<&String> = all.iter().filter(|p| !ignored.contains(*p)).collect();

        if allow.is_empty() { return Ok(()); }

        // 暂存（用 pathspec-from-file 避免命令行过长）
        let pathspec = allow.iter().map(|p| format!(":(top,literal){}", p)).collect::<Vec<_>>().join("\0");
        run_git_with_stdin(
            gitdir, worktree,
            &["--git-dir", &gitdir_str, "--work-tree", &worktree_str,
              "add", "--all", "--sparse",
              "--pathspec-from-file=-", "--pathspec-file-nul"],
            pathspec.as_bytes(),
        ).await?;

        Ok(())
    }

    /// git check-ignore 过滤
    async fn filter_ignored(&self, worktree: &Path, files: &[String]) -> Result<HashSet<String>> {
        let input = files.join("\0") + "\0";
        let output = run_git_with_stdin_in_worktree(
            worktree,
            &["check-ignore", "--no-index", "--stdin", "-z"],
            input.as_bytes(),
        ).await?;
        Ok(output.split('\0').filter(|s| !s.is_empty()).map(String::from).collect())
    }

    /// 从快照 index 移除指定文件（保留 worktree 中的实际文件）。
    /// 等价于 `git rm --cached`，用于清除被 ignore 的已跟踪文件。
    async fn drop_from_index(&self, gitdir: &Path, files: &[String]) -> Result<()> {
        if files.is_empty() { return Ok(()); }
        let pathspec = files.iter()
            .map(|f| format!(":(top,literal){}", f))
            .collect::<Vec<_>>()
            .join("\0");
        run_git_with_stdin(gitdir, &[
            "rm", "--cached", "-f", "--ignore-unmatch",
            "--pathspec-from-file=-", "--pathspec-file-nul",
        ], pathspec.as_bytes()).await
    }
}

async fn run_git(gitdir: &Path, args: &[&str]) -> Result<()> {
    let status = AsyncCommand::new("git")
        .args(args)
        .current_dir(gitdir)
        .status().await?;
    if !status.success() { return Err(anyhow!("git {:?} failed: {}", args, status)); }
    Ok(())
}

async fn run_git_stdout(gitdir: &Path, args: &[&str]) -> Result<String> {
    let output = AsyncCommand::new("git")
        .args(args)
        current_dir(gitdir)
        .output().await?;
    if !output.status.success() {
        return Err(anyhow!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

### 9.3.5 与 OpenCode 实现的差异

| 方面 | OpenCode | Loom |
|---|---|---|
| alternates 优化（共享源仓库对象） | ✅ `seed()` | ❌ 不实现（大仓库才需要） |
| ignore 文件过滤 | ✅ `check-ignore` | ✅ 同 |
| drop ignored（移除快照 index） | ✅ `drop()` | ✅ 同 |
| 大文件限制（2MB） | ✅ | ❌ 不实现（先全量跟踪） |
| 后台 GC（每小时 prune 7d） | ✅ | ❌ 不实现 |
| revert | ✅ `revert(patches[])` 完整实现 | ⏳ 仅 `restore(hash)` 简单版本 |
| diffFull（任意两快照 diff） | ✅ 含 additions/deletions/patch 文本 | ❌ 不实现 |
| 跨平台 | git 必须可用 | 同（Loom 依赖 git） |

### 9.3.6 集成到 translator

`SharedState` 不需要新增字段——`SnapshotService` 是**有状态的服务**，通过 DI 注入而非 per-session 状态。

```rust
// translator.rs

StreamEvent::TurnStart => {
    let worktree = ...;
    let session_id = ...;

    // 调用 SnapshotService.track()
    if let Some(hash) = state.snapshot_service.track(&session_id, &worktree).await.ok().flatten() {
        // 可选：存入 SharedState.pending_snapshot_hash[session_id]
        state.pending_snapshot_hash.write().insert(session_id.clone(), hash);
    }
    
    // ... 创建 step-start part
}

StreamEvent::TurnFinish { .. } => {
    // ... 现有 step-finish part 创建

    // 调用 SnapshotService.patch()
    let hash = state.pending_snapshot_hash.read().get(&session_id).cloned();
    if let Some(hash) = hash {
        match state.snapshot_service.patch(&session_id, &worktree, &hash).await {
            Ok(patch) if !patch.files.is_empty() => {
                push_part(state, msg_id, session_id, "patch", json!({
                    "type": "patch",
                    "hash": patch.hash,
                    "files": patch.files.iter()
                        .map(|f| f.to_string_lossy().replace("\\", "/"))
                        .collect::<Vec<_>>(),
                }));
            }
            _ => {}
        }
        state.pending_snapshot_hash.write().remove(&session_id);
    }
}
```

### 9.3.7 失败模式

| 失败 | 处理 |
|---|---|
| 非 git 项目 | `track()` 返回 `Ok(None)`，patch part 不生成 |
| git 二进制不可用 | `track()` 返回 `Err` → 记录日志，translator 跳过 |
| 大文件导致 `git add` 超时 | 不实现 2MB 限制；如出现，扩展 `add()` 加 stat + filter |
| shadow git DB 损坏 | 首次失败时 `git init` 会重建；老 hash 失效但不会崩溃 |
| 用户取消 step | `TurnStart` 后未到 `TurnFinish` → hash 留在 `pending_snapshot_hash`，下次启动 GC 清理 |

## 9.4 不基于 git 的备选方案（不推荐）

理论上可以用纯文件系统实现：

```rust
// 不推荐：复杂度高、收益小
struct Snapshot {
    files: BTreeMap<PathBuf, [u8; 32]>,  // path → blake3 hash
}

impl Snapshot {
    fn track(worktree: &Path) -> Self {
        walk(worktree)
            .filter(|p| !is_ignored(p))
            .map(|p| (p, blake3(&fs::read(p))))
            .collect()
    }
    
    fn diff(&self, worktree: &Path) -> Vec<Patch> {
        // 对比当前 walk 与 self.files
        // ...
    }
}
```

**为什么不推荐**：
- `walkdir` 需要自己写 ignore 匹配（要解析 .gitignore 语法）
- 大量小文件时性能远差于 git
- revert 需要自己实现文件写入/删除
- 无 alternates 之类的优化

**只在以下情况考虑**：Loom 需要支持非 git 项目（暂未提上日程）。

## 9.5 改动清单

| # | 文件 | 改动 | 工作量 |
|---|---|---|---|
| 1 | `apps/server/src/snapshot/mod.rs` | 新建：`SnapshotService` trait + DI 容器 | 小 |
| 2 | `apps/server/src/snapshot/git_snap.rs` | 新建：`GitSnapshotService` 实现 | 中 |
| 3 | `apps/server/src/state.rs` | `SharedState` 新增 `snapshot_service` 字段 | 小 |
| 4 | `apps/server/src/main.rs` | 注入 `GitSnapshotService::new(data_root)` | 小 |
| 5 | `apps/server/src/translator.rs` | `TurnStart`/`TurnFinish` arm 调用 snapshot service | 中 |
| 6 | `apps/server/src/lib.rs` / `mod.rs` | 注册 `snapshot` 模块 | 小 |

## 9.6 与其他文档的关系

- **OpenCode 实现细节**：参考 `packages/opencode/src/snapshot/index.ts`（约 500 行）
- **translator 中的 step-finish part**：[04-protocol-and-id.md §2.9](04-protocol-and-id.md)
- **StreamEvent::TurnStart/TurnFinish 事件**：[08-stream-event-refactor.md](08-stream-event-refactor.md)
- **共享基础设施**（`git_ops`）：`experimental/worktree/src/git_ops.rs`