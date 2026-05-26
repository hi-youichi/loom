# Loom Git Worktree Isolation 设计方案

> **版本**: v3 — 基于竞品深度调研优化 + 代码库一致性审查
> **参考实现**: Claude Code (2025-06)、Worktrunk v0.48、amux、Cursor 3、OpenAI Codex
> **代码库基准**: `loom/src/tools/invoke_agent.rs`、`loom/src/cli_run/profile.rs`、`loom/src/agent/react/config.rs`

## 1. 背景与动机

当前 Loom 的 `invoke_agent` 在并行调用多个子 Agent 时，所有 Agent 共享同一个工作目录。当多个 Agent 同时编辑文件时会产生竞态条件（文件覆盖、index 锁冲突）。

参考 Claude Code (`--worktree`/`-w`)、Cursor 3 (Agents Window)、OpenAI Codex 的实现，核心思路是：**为每个并行子 Agent 创建独立的 git worktree，实现文件系统级隔离**。

### 1.1 竞品关键发现

通过深度调研 Claude Code、Worktrunk（Rust 实现）、amux（Python 多路复用器）等项目的 worktree 实现，提取出以下可借鉴的优化点：

| 来源 | 关键优化 | 对 Loom 的意义 |
|------|---------|-------------|
| Claude Code | `WorktreeCreate` hook 机制，支持非 git VCS | 可扩展为自定义隔离策略 |
| Claude Code | `.worktreeinclude` 文件复制 + symlink 两种模式 | 支持大文件零拷贝 |
| Worktrunk | Hooks 生命周期（pre-start/post-start/pre-merge/post-merge） | 完善的 worktree 生命周期管理 |
| Worktrunk | `wt step copy-ignored` — 跨 worktree 共享 `node_modules`/`target/` | 构建缓存共享，消除冷启动 |
| Worktrunk | `hash_port` — 每个 worktree 独立端口 | 避免开发服务器端口冲突 |
| Worktrunk | 异步后台清理（trash → prune） | 不阻塞主流程 |
| amux | SQLite 原子任务认领（CAS） | 防止多 Agent 重复处理同一任务 |
| amux | Self-healing watchdog — context compaction 检测 | Agent 崩溃自动恢复 |
| Minion 模式 | 预合并冲突检测（文件路径级） | 提前发现并行任务冲突，序列化执行 |
| 行业实践 | Sparse checkout 减少大仓库检出时间 | Monorepo 场景优化 |

### 1.2 用户故事与使用场景

#### 场景 A：并行功能开发

> 用户：「帮我同时实现登录、搜索、和购物车三个功能」
>
> Loom 调用 `invoke_agent` 并行启动 3 个 dev Agent，每个 Agent 在独立 worktree 中工作。
> 完成后用户看到三个分支的 diff，选择性地合并。

**用户价值**：任务并行执行，总耗时从 3× 降到 1×，且互不干扰。

#### 场景 B：安全实验

> 用户：`loom -w`（加 `--worktree` 标志启动）
>
> Loom 在独立 worktree 中运行整个会话。用户可以放心让 Agent 大幅修改代码，
> 主工作目录完全不受影响。结束后满意则合并，不满意直接丢弃。

**用户价值**：消除「Agent 改坏代码」的恐惧，鼓励用户将更大胆的任务交给 Agent。

#### 场景 C：团队级 Agent Profile

> 管理员在 `.loom/agents/dev/config.yaml` 中配置 `isolation: worktree`。
> 团队成员无需关心隔离细节，调用该 Agent 时自动获得文件级安全隔离。

**用户价值**：零配置安全隔离，降低使用门槛。

#### 场景 D：Monorepo 定向开发

> 用户在大型 monorepo 中指定 `sparse_paths: ["services/auth/"]`。
> Agent 只检出相关子目录，`git status` / 搜索 / 构建都更快。

**用户体验变化**：
- Agent 响应更快（不扫描全仓库）
- `node_modules` / `target/` 共享主仓库缓存，无需重新安装依赖

### 1.3 用户可见行为（What Changes for Users）

| 维度 | Before（当前） | After（启用 worktree） |
|------|--------------|----------------------|
| 并行子 Agent | 共享工作目录，后写入覆盖先写入 | 每个Agent独立目录，互不影响 |
| `invoke_agent` 参数 | 无 `isolation` 字段 | 新增 `isolation: "worktree"` |
| CLI 启动 | `loom` | `loom -w`（实验模式） |
| Agent Profile | 无隔离配置 | 新增 `isolation` + `worktree` 配置块 |
| 结果查看 | 只有一个工作目录的 diff | 每个worktree独立分支，可分别查看/合并 |
| 清理 | 无 | 无变更自动清理，有变更保留分支 |
| 依赖安装 | 每次可能重新安装 | 共享主仓库缓存（可选） |

### 1.4 非目标（What This Is NOT）

- **不是完整的沙箱**：worktree 共享 `.git` 对象库，Agent 理论上可修改 git hooks。安全模型依赖 Agent 可信。
- **不自动合并**：Agent 完成后保留分支和 diff，合并决策留给用户或父 Agent。
- **不替代 Docker/VM 隔离**：这是文件系统级隔离，不是进程/网络级隔离。
- **不解决非 git 项目**：非 git 仓库自动降级为共享目录模式。

## 2. 核心设计

### 2.1 隔离模型

```
主工作目录 (main checkout)
├── .loom/
│   └── worktrees/           ← worktree 存储目录
│       ├── task-auth/       ← worktree 1: branch worktree-task-auth
│       ├── task-search/     ← worktree 2: branch worktree-task-search
│       └── task-fix-bug/    ← worktree 3: branch worktree-task-fix-bug
```

- 每个 worktree 是一个独立的工作目录，有独立分支、独立文件状态
- 共享 `.git` 对象数据库，几乎零额外磁盘开销
- 子 Agent 完成后可自动合并/清理

### 2.2 触发方式

三种触发路径，优先级从高到低：

| 触发方式 | 说明 |
|---------|------|
| `invoke_agent` 参数 `isolation: "worktree"` | LLM 在工具调用时显式指定 |
| Agent profile `config.yaml` 中 `isolation: worktree` | 管理员在 profile 中预设 |
| CLI `--worktree` / `-w` 标志 | 用户启动顶层会话时使用 |

**Claude Code 的 Hook 扩展机制**（新增）：Claude Code 通过 `WorktreeCreate` / `WorktreeRemove` hook 允许用户替换默认 git 行为（如支持非 git VCS）。Loom 可借鉴为 `WorktreeHook` trait：

```rust
pub trait WorktreeHook: Send + Sync {
    fn create(&self, ctx: &WorktreeContext) -> Result<PathBuf>;
    fn remove(&self, ctx: &WorktreeContext) -> Result<()>;
}
```

用户可通过 `.loom/worktree-hooks/` 目录提供自定义实现。

### 2.3 生命周期（借鉴 Worktrunk Hook 模型）

```
pre-start → 创建 → setup → 使用 → 评估 → pre-merge → 合并/清理 → post-merge
```

| 阶段 | 说明 | 对应 Hook |
|------|------|----------|
| **pre-start** | 检查磁盘空间、分支冲突 | `on_pre_start` |
| **创建** | `git worktree add .loom/worktrees/<name> -b worktree-<name> [--detach] <base-ref>` | 内置 |
| **setup** | 复制 `.worktreeinclude` 文件、共享构建缓存、安装依赖 | `on_post_start` |
| **使用** | 子 Agent 的 `working_folder` 指向 worktree 路径 | — |
| **评估** | 检查是否有未提交变更、预合并冲突检测 | `on_pre_merge` |
| **合并/清理** | 有变更→保留+报告；无变更→异步清理 | `on_post_merge` / `on_pre_remove` |

**异步清理**（借鉴 Worktrunk trash 模式）：worktree 删除不直接 `rm -rf`，而是先移到 `.loom/worktrees/.trash/` 再后台 prune，避免阻塞 Agent 返回结果。

## 3. 数据结构

### 3.1 `WorktreeConfig` - 配置

```rust
// 新文件: loom/src/worktree/mod.rs

#[derive(Clone, Debug)]
pub struct WorktreeConfig {
    pub base_ref: BaseRef,
    pub storage_dir: Option<PathBuf>,
    pub branch_prefix: String,
    pub auto_cleanup: bool,
    pub detached: bool,
    pub include_patterns: Vec<String>,
    /// 新增: 需要符号链接而非复制的文件模式（大文件优化）
    pub symlink_patterns: Vec<String>,
    /// 新增: 需要跨 worktree 共享的构建缓存目录（如 target/, node_modules/）
    pub shared_cache_dirs: Vec<String>,
    /// 新增: sparse checkout 路径（monorepo 场景）
    pub sparse_paths: Vec<String>,
    /// 新增: 预合并冲突检测策略
    pub conflict_detection: ConflictDetection,
    /// 新增: 清理策略（同步 vs 异步 trash）
    pub cleanup_strategy: CleanupStrategy,
}

#[derive(Clone, Debug, Default)]
pub enum BaseRef {
    #[default]
    Fresh,
    Head,
    Ref(String),
}

/// 预合并冲突检测
#[derive(Clone, Debug, Default)]
pub enum ConflictDetection {
    /// 不检测
    #[default]
    None,
    /// 文件路径级：比较各 worktree 修改的文件列表
    FilePath,
    /// Diff hunk 级：比较具体修改行范围
    HunkLevel,
}

/// 清理策略
#[derive(Clone, Debug, Default)]
pub enum CleanupStrategy {
    /// 同步删除
    #[default]
    Sync,
    /// 先移到 .trash/ 再后台 prune（不阻塞主流程）
    AsyncTrash,
}
```

### 3.2 `WorktreeHandle` - 运行时句柄

```rust
#[derive(Debug)]
pub struct WorktreeHandle {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub has_changes: bool,
    pub agent_name: String,
    /// 新增: 预计修改的文件路径（用于冲突检测）
    pub estimated_paths: Vec<String>,
    /// 新增: worktree 状态
    pub state: WorktreeState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorktreeState {
    Active,
    Completed,
    Failed,
    Trashed,
}

impl WorktreeHandle {
    pub async fn cleanup(self) -> Result<()> { ... }
    pub async fn check_changes(&self) -> Result<bool> { ... }
    pub async fn diff_main(&self) -> Result<String> { ... }
    /// 新增: 预合并冲突检测 — 返回与其他 worktree 重叠的文件列表
    pub fn detect_conflicts(&self, others: &[&WorktreeHandle]) -> Vec<ConflictInfo> { ... }
    /// 新增: 获取变更文件列表（不含 diff 内容，用于轻量级冲突检测）
    pub async fn changed_files(&self) -> Result<Vec<String>> { ... }
}

#[derive(Debug)]
pub struct ConflictInfo {
    pub other_agent: String,
    pub conflicting_paths: Vec<String>,
    pub severity: ConflictSeverity,
}

pub enum ConflictSeverity {
    /// 同一文件，需序列化执行
    FileOverlap,
    /// 同一文件同一区域，极大概率冲突
    HunkOverlap,
}
```

### 3.3 Agent Profile 扩展

```yaml
# .loom/agents/dev/config.yaml 新增字段
isolation: worktree          # none (default) | worktree
worktree:
  base_ref: fresh            # fresh (default) | head | <ref>
  auto_cleanup: true
  cleanup_strategy: async_trash  # sync | async_trash
  conflict_detection: file_path  # none | file_path | hunk_level
  include:
    - ".env"
    - ".env.local"
  symlink:                    # 大文件用 symlink 代替复制
    - ".env.local"
    - "data/"
  shared_cache:               # 共享构建缓存目录（hardlink/symlink）
    - "node_modules/"
    - "target/"
  sparse_paths:               # monorepo 场景仅检出部分路径
    - "services/auth/"
    - "shared/"
```

### 3.4 `invoke_agent` 参数扩展

```json
{
  "agents": [
    {
      "agent": "dev",
      "task": "Implement auth feature",
      "isolation": "worktree",
      "working_folder": ".loom/worktrees/task-auth"
    }
  ]
}
```

`isolation: "worktree"` 时：
- 如果同时提供了 `working_folder`，使用该路径作为 worktree 目录
- 如果未提供 `working_folder`，自动生成路径 `.loom/worktrees/<agent>-<timestamp>/`

### 3.5 `ReactBuildConfig` 扩展

```rust
// 在 ReactBuildConfig 中新增
pub worktree_config: Option<WorktreeConfig>,
```

## 4. 模块设计

### 4.1 新增 `loom/src/worktree/` 模块

```
loom/src/worktree/
├── mod.rs           # 公共接口 + WorktreeConfig, BaseRef, WorktreeState
├── manager.rs       # WorktreeManager: 创建/清理/列出 worktree
├── git_ops.rs       # 底层 git worktree 命令封装
├── include.rs       # .worktreeinclude 文件处理 + symlink/copy 策略
├── cache.rs         # 构建缓存共享（target/, node_modules/ 跨 worktree）
├── conflict.rs      # 预合并冲突检测（FilePath / HunkLevel）
├── cleanup.rs       # 清理策略（Sync / AsyncTrash + prune）
└── hooks.rs         # WorktreeHook trait + 内置 hook 注册
```

### 4.2 `WorktreeManager` 核心接口

```rust
pub struct WorktreeManager {
    repo_root: PathBuf,
    config: WorktreeConfig,
    /// 活跃的 worktree 注册表（用于冲突检测）
    active_handles: Arc<Mutex<Vec<WorktreeHandle>>>,
}

impl WorktreeManager {
    pub fn from_working_dir(config: WorktreeConfig) -> Result<Self>;

    /// 创建隔离 worktree（完整流程：git add → setup hooks → 缓存共享 → sparse checkout）
    pub async fn create_for_agent(
        &self,
        agent_name: &str,
        task_hint: Option<&str>,
        estimated_paths: Option<&[String]>,  // 新增: 任务预计修改的文件
    ) -> Result<WorktreeHandle>;

    pub fn list_active(&self) -> Result<Vec<WorktreeHandle>>;

    pub async fn cleanup(&self, handle: WorktreeHandle) -> Result<()>;

    pub async fn cleanup_stale(&self) -> Result<usize>;

    /// 新增: 预合并冲突检测 — 在并行任务启动前检测
    pub fn detect_parallel_conflicts(
        &self,
        handles: &[WorktreeHandle],
    ) -> Vec<ConflictInfo>;

    /// 新增: 异步 prune trash 目录
    pub async fn prune_trash(&self) -> Result<usize>;
}

/// 全局单例 — 在 invoke_agent 并行调用时共享活跃 worktree 信息
static GLOBAL_MANAGER: Lazy<Mutex<Option<WorktreeManager>>> = 
    Lazy::new(|| Mutex::new(None));
```

### 4.3 `git_ops.rs` 底层操作

```rust
/// 封装 git worktree 命令，不依赖 git2 crate，使用进程调用
pub fn worktree_add(
    repo_root: &Path,
    target_path: &Path,
    branch_name: Option<&str>,
    base_ref: &BaseRef,
) -> Result<()>;

pub fn worktree_remove(path: &Path, force: bool) -> Result<()>;
pub fn worktree_list(repo_root: &Path) -> Result<Vec<PathBuf>>;
pub fn branch_delete(repo_root: &Path, branch: &str) -> Result<()>;
pub fn has_uncommitted_changes(worktree_path: &Path) -> Result<bool>;
pub fn diff_worktree(worktree_path: &Path, base: &str) -> Result<String>;
pub fn current_branch(path: &Path) -> Result<String>;
pub fn resolve_default_ref(repo_root: &Path) -> Result<String>;
```

使用 `std::process::Command` 调用系统 `git`（与现有 loom 的 bash 执行模式一致），不引入 `git2` C 绑定。

## 5. 集成点

### 5.1 `InvokeAgentTool` 集成

修改 `invoke_agent.rs` 的 `call_single_exec` 和 `invoke_single_agent`：

```rust
// 在 resolve_profile 之后，build_react_runner 之前插入：

let isolation = args.get("isolation")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string())
    .or_else(|| profile.isolation.clone());

let worktree_handle = if isolation.as_deref() == Some("worktree") {
    let manager = WorktreeManager::from_working_dir(
        self.base_config.worktree_config.clone().unwrap_or_default()
    )?;
    let handle = manager.create_for_agent(agent_name, Some(task)).await?;
    tracing::info!(
        agent = %agent_name,
        worktree_path = %handle.path.display(),
        branch = ?handle.branch,
        "Created worktree for sub-agent"
    );
    Some(handle)
} else {
    None
};

// 如果有 worktree，覆盖 working_folder
let effective_working_folder = worktree_handle
    .as_ref()
    .map(|h| h.path.clone())
    .or(working_folder_override);

// ... 原有的 build_config_from_profile / resolve_tier_and_build_config 逻辑 ...

// Agent 执行完成后，处理 worktree 清理
if let Some(handle) = worktree_handle {
    let has_changes = handle.check_changes().await?;
    if !has_changes && manager.config.auto_cleanup {
        manager.cleanup(handle).await?;
    } else {
        // 报告 worktree 路径和分支给父 Agent
        tracing::info!(
            agent = %agent_name,
            path = %handle.path.display(),
            branch = ?handle.branch,
            "Worktree has changes, preserving for review"
        );
    }
}
```

### 5.2 CLI `--worktree` 集成

在 `cli` crate 中添加 CLI 参数：

```rust
// cli/src/args.rs 新增
#[arg(long = "worktree", short = 'w")]
pub worktree: bool,
```

在 `run_cli_turn` 中，如果 `--worktree` 为 true：
1. 调用 `WorktreeManager::create_for_agent("main", None)` 创建主会话的 worktree
2. 将 `RunOptions::working_folder` 设为 worktree 路径
3. 会话结束后，有变更则保留，无变更则清理

### 5.3 Agent Profile 加载扩展

修改 `load_profile_from_options` 相关代码，读取 `config.yaml` 中的 `isolation` 和 `worktree` 字段：

```rust
// AgentProfile 结构体新增
pub isolation: Option<String>,
pub worktree: Option<WorktreeConfig>,
```

### 5.4 `.worktreeinclude` 支持

```
# .worktreeinclude 文件示例
.env
.env.local
config/local.*
```

`include.rs` 负责解析此文件，在创建 worktree 后复制匹配的文件到 worktree 目录。

## 6. 并行调用流程（含冲突检测 + 缓存共享）

```
用户: "并行实现三个功能"
  │
  ▼
父 Agent (think node)
  │ 调用 invoke_agent
  │
  ├─► invoke_agent({
  │     agents: [
  │       {agent: "dev", task: "实现认证", isolation: "worktree", estimated_paths: ["src/auth/"]},
  │       {agent: "dev", task: "实现搜索", isolation: "worktree", estimated_paths: ["src/search/"]},
  │       {agent: "dev", task: "修复Bug",  isolation: "worktree", estimated_paths: ["src/api/bug.ts"]}
  │     ]
  │   })
  │
  ▼ Phase 0: 预合并冲突检测
  │  ├─ 对比 estimated_paths
  │  ├─ 发现 "实现搜索" 和 "修复Bug" 都可能改 src/api/
  │  └─ 决策: 将 "修复Bug" 从并行波次移到第二波
  │
  ▼ Phase 1: 创建 worktrees（可并行）
  │
  ├── create_worktree("dev-auth")  ← sparse checkout: services/auth/
  │   ├── copy .worktreeinclude files
  │   ├── symlink shared_cache (node_modules → 主仓库)
  │   └── run post-start hooks (npm install --frozen-lockfile, etc.)
  │
  └── create_worktree("dev-search") ← sparse checkout: services/search/
      ├── copy .worktreeinclude files
      ├── symlink shared_cache
      └── run post-start hooks
  │
  ▼ Phase 2: 并行执行 Agents
  │
  ├── tokio::spawn → agent 在 worktree-dev-auth/ 中工作
  └── tokio::spawn → agent 在 worktree-dev-search/ 中工作
  │
  ▼ Phase 3: 评估 + 异步清理
  │
  ├── dev-auth 完成 → check_changes → 有变更 → 保留分支
  ├── dev-search 完成 → check_changes → 无变更 → trash + prune
  │
  ▼ Phase 4: 第二波（串行化冲突任务）
  │
  └── create_worktree("dev-fix-bug") → agent 执行 → 完成
  │
  ▼
聚合所有结果返回给父 Agent
父 Agent 决定如何 merge 各 worktree 分支
```

### 6.1 预合并冲突检测算法

借鉴 Minion 模式的冲突预测 + codeongrass.com 的 Clash Predictor 思路：

```rust
/// 文件路径级冲突检测
fn detect_path_conflicts(tasks: &[(String, Vec<String>)]) -> Vec<(String, String, Vec<String>)> {
    // 1. 对每个任务的 estimated_paths 构建前缀树 (trie)
    // 2. 对任意两个任务做交集
    // 3. 返回重叠的 (task_a, task_b, overlapping_paths)
    // 4. 如果 conflict_detection: hunk_level，进一步比较 diff hunks
}

/// 决策：冲突的任务序列化到不同波次
fn schedule_waves(tasks: Vec<TaskSpec>, conflicts: &[ConflictInfo]) -> Vec<Vec<TaskSpec>> {
    // 1. 构建冲突图（顶点=任务，边=冲突）
    // 2. 图着色算法分配波次（无冲突的任务同波次）
    // 3. 返回波的序列
}
```

## 7. 性能优化（借鉴竞品实践）

### 7.1 构建缓存共享

**问题**：每个 worktree 需要独立 `node_modules/` 或 `target/`，冷启动成本高。

**Worktrunk 方案**：`wt step copy-ignored` — 从主仓库 hardlink/symlink 构建缓存到新 worktree。

Loom 实现：

```rust
// cache.rs
pub fn share_cache_dirs(
    source: &Path,   // 主仓库路径
    target: &Path,   // worktree 路径
    cache_dirs: &[String],  // ["node_modules/", "target/"]
    strategy: CacheShareStrategy,
) -> Result<()> {
    for dir in cache_dirs {
        let src = source.join(dir);
        let dst = target.join(dir);
        if src.exists() && !dst.exists() {
            match strategy {
                // 优先 symlink（零拷贝），Windows 上可能需要 junction
                CacheShareStrategy::Symlink => std::os::windows::fs::symlink_dir(&src, &dst)
                    .or_else(|_| symlink_junction(&src, &dst))?,
                // hardlink 每个文件（跨设备安全）
                CacheShareStrategy::Hardlink => hardlink_dir_recursive(&src, &dst)?,
            }
        }
    }
}
```

### 7.2 Sparse Checkout（Monorepo 优化）

**问题**：大仓库全量检出慢，Agent 不需要看到所有文件。

**Git 原生支持**：`git sparse-checkout init --cone && git sparse-checkout set <paths>`

Loom 实现：

```rust
// git_ops.rs
pub fn enable_sparse_checkout(worktree_path: &Path, paths: &[String]) -> Result<()> {
    // git -C <worktree> sparse-checkout init --cone
    run_git(worktree_path, &["sparse-checkout", "init", "--cone"])?;
    // git -C <worktree> sparse-checkout set <path1> <path2> ...
    let mut args = vec!["sparse-checkout", "set"];
    args.extend(paths.iter().map(|s| s.as_str()));
    run_git(worktree_path, &args)
}
```

当 `config.yaml` 中配置 `sparse_paths` 时，创建 worktree 后自动启用。优点：
- `git status`/`git diff` 只扫描相关目录（10-100x 加速）
- Agent grep 搜索范围更精准
- 减少 context window 污染

### 7.3 `.gitignore` 处理

自动将 `.loom/worktrees/` 添加到项目 `.gitignore`（如果尚未存在），防止 worktree 内容被跟踪。

### 7.4 异步清理（Trash + Prune）

```rust
// cleanup.rs
pub async fn cleanup_worktree(handle: WorktreeHandle, strategy: CleanupStrategy) -> Result<()> {
    match strategy {
        CleanupStrategy::Sync => {
            // git worktree remove + git branch -d（同步，可能慢）
            git_ops::worktree_remove(&handle.path, true)?;
            if let Some(ref branch) = handle.branch {
                // branch delete 需要 repo_root
                git_ops::branch_delete(&handle.repo_root, branch)?;
            }
        }
        CleanupStrategy::AsyncTrash => {
            let trash_dir = handle.repo_root.join(".loom/worktrees/.trash");
            // 原子移动到 trash 目录
            let dest = trash_dir.join(handle.path.file_name().unwrap());
            tokio::fs::rename(&handle.path, &dest).await?;
            // 后台 prune（不阻塞主流程）
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let _ = git_ops::worktree_remove(&dest, true);
                if let Some(ref branch) = handle.branch {
                    let _ = git_ops::branch_delete(&handle.repo_root, branch);
                }
                let _ = tokio::fs::remove_dir_all(&dest).await;
            });
        }
    }
}
```

### 7.5 Worktree 复用（Session 恢复）

Claude Code 的一个关键优化：如果 worktree 目录已存在且分支匹配，直接复用而非重新创建。

```rust
pub fn get_or_create_worktree(
    &self,
    slug: &str,
    base_ref: &BaseRef,
) -> Result<WorktreeHandle> {
    let target = self.storage_path.join(slug);
    if target.exists() {
        // 快速恢复：验证 worktree 状态
        if let Ok(handle) = self.validate_existing(&target) {
            return Ok(handle);
        }
        // 状态不一致，清理后重建
        let _ = git_ops::worktree_remove(&target, true);
    }
    self.create_new(slug, base_ref)
}
```

## 8. 错误处理

| 场景 | 处理方式 |
|------|---------|
| 非 git 仓库 | 忽略 `isolation: worktree`，正常执行（降级为共享目录） |
| `git worktree add` 失败 | 返回错误，不创建 worktree |
| 分支名冲突 | 附加 `-<short-uuid>` 后缀 |
| worktree 清理失败 | 记录警告，不影响 Agent 结果返回 |
| 磁盘空间不足 | 在创建前检查，提前失败 |
| 并发 git 操作导致 index 锁 | 指数退避重试（100ms/200ms/400ms），最多 3 次 |
| macOS `SIGBUS` 信号 | 已知问题：并发 worktree 操作可能触发，加互斥锁序列化 git 操作 |
| symlink 失败（Windows） | 降级为 hardlink 或 copy |
| sparse checkout 不支持 | 检测 git 版本（需 >= 2.25），不支持则跳过 |
| 子 Agent 崩溃 | on_pre_remove hook 记录部分进度，worktree 保留供人工检查 |

### 8.1 Index 锁竞争处理

```rust
pub async fn run_git_with_retry(args: &[&str], workdir: &Path) -> Result<Output> {
    let mut delay = Duration::from_millis(100);
    for attempt in 0..3 {
        match run_git(args, workdir) {
            Ok(output) => return Ok(output),
            Err(e) if is_index_lock_error(&e) && attempt < 2 => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

## 9. 实施计划

### Phase 1: 核心模块（~3 天）
- `loom/src/worktree/mod.rs` - WorktreeConfig, BaseRef, WorktreeHandle, WorktreeState
- `loom/src/worktree/git_ops.rs` - git worktree 命令封装 + index 锁重试
- `loom/src/worktree/manager.rs` - WorktreeManager + GLOBAL_MANAGER 单例

### Phase 2: invoke_agent 集成（~2 天）
- `invoke_agent` 参数解析扩展（`isolation` + `estimated_paths` 字段）
- Agent Profile `config.yaml` 扩展
- worktree 创建/清理集成到 `call_single_exec` 和 `invoke_single_agent`
- 全局冲突检测注册表

### Phase 3: CLI 集成 + Hooks（~2 天）
- CLI `--worktree` / `-w` 参数
- `.worktreeinclude` 支持 + symlink 模式
- `.gitignore` 自动更新
- worktree 清理命令 `loom worktree cleanup`
- WorktreeHook trait + 内置 hooks

### Phase 4: 性能优化（~2 天）
- `cache.rs` — 构建缓存共享（node_modules/target hardlink/symlink）
- `conflict.rs` — 预合并冲突检测 + 波次调度
- `cleanup.rs` — AsyncTrash + prune
- Sparse checkout 支持
- Worktree 复用（session 恢复）

### Phase 5: 测试 + 文档（~1 天）
- 单元测试（git_ops, manager, conflict detection）
- 集成测试（并行 invoke_agent with worktree）
- 压力测试（10 个并行 worktree）
- 用户文档

## 10. 依赖

- **无新 crate 依赖**：使用 `std::process::Command` 调用系统 `git`
- 前提条件：用户系统已安装 `git` >= 2.25（sparse checkout 需要）
- 如果需要更精细的 git 操作，未来可选引入 `gix`（纯 Rust，Worktrunk 即用此方案）

### 10.1 可选集成

| 工具 | 集成方式 | 收益 |
|------|---------|------|
| **Worktrunk** (`wt`) | Loom 可调用 `wt` CLI 作为 git 后端 | 成熟的 worktree 生命周期管理 + hooks |
| **amux** | 通过 REST API 与 Loom 对接 | Kanban 任务管理 + Agent-to-Agent 协调 |
| **gix** | 纯 Rust git 实现 | 避免 shell 调用开销，更可靠的错误处理 |

### 10.2 Git 版本兼容性

| Git 版本 | 支持的功能 |
|---------|-----------|
| < 2.25 | 基础 worktree（无 sparse checkout） |
| >= 2.25 | sparse-checkout 命令 |
| >= 2.34 | sparse index（性能优化） |
| >= 2.37 | cone mode 默认启用 |

## 11. 与竞品对比

| 特性 | Claude Code | Cursor 3 | Codex | Worktrunk | Loom（本方案） |
|------|------------|-----------|-------|-----------|-------------|
| 触发方式 | `-w` flag | Agents Window | Thread 设置 | CLI `wt switch -c` | `isolation` 参数 + CLI `-w` + profile |
| 存储位置 | `.claude/worktrees/` | IDE 管理 | `$CODEX_HOME/worktrees` | 兄弟目录 `../repo.<branch>` | `.loom/worktrees/` |
| 基础分支 | `origin/HEAD` / `head` | 当前分支 | HEAD (detached) | 当前分支 | `fresh` / `head` / 自定义 ref |
| 自动清理 | 无变更时自动清理 | 手动 | Apply/Overwrite | `wt remove` | 可配置，默认自动清理 |
| 子 Agent 隔离 | `isolation: worktree` 前置声明 | 每个 Agent 自动隔离 | Thread 级隔离 | 手动 `wt switch` | `isolation: "worktree"` 参数 |
| 未跟踪文件复制 | `.worktreeinclude` | IDE 同步 | 复制完整目录 | hooks | `.worktreeinclude` + symlink |
| 构建缓存共享 | 无 | 无 | 无 | `wt step copy-ignored` | `shared_cache` 配置 |
| 冲突检测 | 无 | 无 | 无 | `wt list` 显示冲突 | 预合并冲突检测 + 波次调度 |
| Sparse checkout | 无 | 无 | 无 | 无 | `sparse_paths` 配置 |
| 异步清理 | 无 | 无 | 无 | trash + prune | `async_trash` 策略 |
| Worktree 复用 | 有（快速恢复） | 无 | 无 | 有（`wt switch` 到已有） | 有（`get_or_create`） |
| Hook 系统 | WorktreeCreate/Remove | 无 | 无 | 10+ lifecycle hooks | `WorktreeHook` trait |
| 最大并行数 | ~10 | ~10 | ~5 | 无限制 | ~7（受 API 限制） |

## 12. 安全考虑

- **符号链接安全**：Windows 上需要管理员权限或开发者模式才能创建 symlink，降级为 junction 或 hardlink
- **Worktree 泄漏**：Agent 崩溃可能导致 worktree 未清理。方案：启动时扫描 `.loom/worktrees/.trash/` 并 prune
- **权限隔离**：worktree 共享 `.git` 目录，Agent 理论上可修改 git hooks。安全模型依赖 Agent 沙箱
- **分支命名**：使用 `worktree-` 前缀，避免与用户分支冲突。自动检测并添加 UUID 后缀

## 13. 未来扩展

1. **Agent-to-Agent 协调**（借鉴 amux）：通过共享 SQLite 任务板实现原子任务认领，防止多 Agent 重复工作
2. **Self-healing watchdog**（借鉴 amux）：检测 Agent context compaction / 崩溃，自动重启并重放最后消息
3. **Stacked branches**（借鉴 worktrunk-sync）：支持分支依赖链，按拓扑顺序 rebase
4. **Cloud worktree**（借鉴 Cursor）：worktree 在远程 VM 中运行，本地仅同步 diff
5. **Merge queue 集成**：worktree 分支完成后自动入队 CI/CD merge queue

---

## 附录 A: 代码库一致性审查（v3 新增）

> 本附录记录方案与当前代码库（截至 2026-05）的对照分析，标记实现时需注意的接口变更。

### A.1 `WorktreeHandle` 字段补全

方案 3.2 节的 `WorktreeHandle` 缺少 `repo_root` 字段，但 7.4 节 cleanup 代码中引用了 `handle.repo_root`。需修正为：

```rust
pub struct WorktreeHandle {
    /// 主仓库根目录（用于 branch delete 等需要 repo 级操作的场景）
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub has_changes: bool,
    pub agent_name: String,
    pub estimated_paths: Vec<String>,
    pub state: WorktreeState,
}
```

### A.2 `BaseRef::Fresh` 语义明确

`BaseRef::Fresh` 是默认值但语义模糊。明确为：

- **`Fresh`**：基于 `HEAD` 创建一个新的空工作树，不继承暂存区内容。子 Agent 看到的是干净的 HEAD 快照。这是最安全的默认值，保证子 Agent 从干净状态开始。
- **`Head`**：同 `Fresh`，但保留分支名（不 detach）。用于需要 branch 可追溯的场景。
- **`Ref(String)`**：基于指定 ref（如 `origin/main`、`v1.0.0`）创建。

### A.3 `invoke_agent.yaml` 工具 schema 扩展

当前 `loom/tools/invoke_agent.yaml` 的 `items.properties` 仅包含 `agent`、`task`、`working_folder`、`model_tier`。需新增：

```yaml
# invoke_agent.yaml items.properties 新增
isolation:
  type: string
  description: "Optional: create an isolated git worktree for this sub-agent. Values: 'worktree'. If omitted, uses shared working directory."
  enum: ["worktree"]
estimated_paths:
  type: array
  description: "Optional: list of file/dir paths the task is expected to modify. Used for pre-merge conflict detection when multiple agents run in parallel."
  items:
    type: string
```

### A.4 `invoke_agent.rs` 代码集成点精确映射

对照 `invoke_agent.rs` 当前代码，集成点精确位置：

1. **`call_single_exec`** (L235-439)：worktree 创建应在 L268（`resolve_profile` 之后）和 L283（`build_config_from_profile` 之前）之间插入。`effective_working_folder` 传入 `build_config_from_profile` 替代 `working_folder_override`。
2. **`call_multiple`** (L442+)：需在 `tokio::join!` 循环前添加 Phase 0 冲突检测逻辑，为每个 agent 创建独立 worktree handle。
3. **`call_multiple_async`**：async 模式下 worktree 生命周期管理更复杂——需注册全局 cleanup handler 确保后台 Agent 的 worktree 不会泄漏。

### A.5 `AgentProfile` 结构体扩展位置

`loom/src/cli_run/profile.rs` 的 `AgentProfile` 需新增：

```rust
// 在 pub extends: Option<String> 之后添加
#[serde(default)]
pub isolation: Option<String>,
#[serde(default)]
pub worktree: Option<WorktreeProfileConfig>,
```

注意：`WorktreeProfileConfig` 应为 `WorktreeConfig` 的简化版（仅包含 profile 可配置的字段），而非直接使用 `WorktreeConfig`（后者包含运行时计算的字段如 `storage_dir`）。

### A.6 `ReactBuildConfig` 传递路径

`ReactBuildConfig.worktree_config` 需要在以下路径中正确传播：
- CLI 入口 → `HelveConfig` → `ReactBuildConfig`（主会话 `--worktree` 模式）
- `build_config_from_profile` → 子 Agent 的 `ReactBuildConfig`（从 profile 继承 worktree 配置）
- `resolve_tier_and_build_config` → 确认 worktree_config 不被 tier 解析覆盖

## 附录 B: 并行模式 Worktree 生命周期（v3 新增）

### B.1 `call_multiple` 集成伪代码

```rust
// invoke_agent.rs — call_multiple 中新增 Phase 0/1/3
async fn call_multiple(&self, args: Value, ctx: Option<&ToolCallContext>) -> Result<...> {
    // ... 现有验证逻辑 ...

    // Phase 0: 解析 isolation + estimated_paths，检测冲突
    let worktree_specs = parse_worktree_specs(&agents);
    let conflicts = WorktreeManager::detect_parallel_conflicts_from_specs(&worktree_specs);

    // Phase 0.5: 按冲突结果分波
    let waves = schedule_waves(agents, &conflicts);

    let mut all_results = Vec::new();
    for wave in waves {
        // Phase 1: 批量创建 worktrees
        let handles: Vec<Option<WorktreeHandle>> = wave.iter()
            .map(|spec| create_worktree_if_needed(spec))
            .collect();

        // Phase 2: 并行执行（现有 tokio::join 逻辑，working_folder 指向 worktree）
        let wave_results = run_wave_parallel(wave, handles, ctx).await;
        all_results.extend(wave_results);

        // Phase 3: 评估 + 清理每个 worktree
        for handle in handles.into_iter().flatten() {
            evaluate_and_cleanup(handle).await;
        }
    }
    // 聚合结果
}
```

### B.2 父 Agent 结果传播

子 Agent 完成后，worktree 信息需附加到返回结果中，让父 Agent 知道变更在哪：

```rust
// 在 Ok(ToolCallContent::text(reply)) 之前附加 worktree 元数据
if let Some(ref handle) = worktree_handle {
    let summary = if handle.has_changes {
        let diff = handle.diff_main().await.unwrap_or_default();
        let truncated = diff.chars().take(4000).collect::<String>();
        format!(
            "\n\n---\n[worktree] path: {}\n[worktree] branch: {}\n[worktree] diff (truncated):\n{}",
            handle.path.display(),
            handle.branch.as_deref().unwrap_or("(detached)"),
            truncated,
        )
    } else {
        "\n\n---\n[worktree] No file changes detected.".to_string()
    };
    reply = format!("{}{}", reply, summary);
}
```

## 附录 C: Windows 特殊处理（v3 新增）

### C.1 长路径问题

Windows 默认路径限制 260 字符。worktree 路径如 `.loom/worktrees/task-very-long-feature-name-with-details/` 可能超出限制。

```rust
// 在 git worktree add 之前，确保使用 \\?\ 前缀
fn ensure_long_path_compatible(path: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !abs.to_str().map(|s| s.starts_with(r"\\?\")).unwrap_or(false) {
            PathBuf::from(format!(r"\\?\{}", abs.display()))
        } else {
            abs
        }
    } else {
        path.to_path_buf()
    }
}
```

同时限制 worktree slug 长度：

```rust
fn sanitize_slug(name: &str) -> String {
    let slug: String = name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(32)  // 限制长度
        .collect();
    slug
}
```

### C.2 Junction vs Symlink

Windows 上 symlink 需要开发者模式或管理员权限。降级策略：

```
symlink_dir → 失败 → junction (fs::symlink_dir on Windows via CreateSymbolicLink) → 失败 → hardlink_dir_recursive → 失败 → copy_dir_recursive
```

```rust
#[cfg(target_os = "windows")]
fn symlink_or_junction(src: &Path, dst: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
        .or_else(|_| junction::create(src, dst))
        .map_err(|e| anyhow::anyhow!("symlink/junction failed: {}", e))
}
```

### C.3 AsyncTrash 跨盘符问题

`tokio::fs::rename` 在 Windows 上不能跨盘符操作。如果 `.loom/worktrees/` 在 C 盘而临时目录在 D 盘，需回退到 copy+delete：

```rust
pub async fn move_to_trash(src: &Path, trash_dir: &Path) -> Result<()> {
    let dest = trash_dir.join(src.file_name().unwrap());
    if tokio::fs::rename(src, &dest).await.is_err() {
        // 跨盘符回退：copy + delete
        tokio::fs::create_dir_all(&dest).await?;
        copy_dir_recursive(src, &dest).await?;
        tokio::fs::remove_dir_all(src).await?;
    }
    Ok(())
}
```

## 附录 D: Worktree 嵌套检测（v3 新增）

如果用户已在 worktree 中运行 Loom（例如 `--worktree` 嵌套），需要检测并防止无限嵌套：

```rust
pub fn detect_worktree_nesting(working_dir: &Path) -> Result<bool> {
    // 检查 .git 是否为文件（而非目录）— 这是 worktree 的标志
    let git_path = working_dir.join(".git");
    if git_path.exists() && git_path.is_file() {
        // 当前已在 worktree 中，读取 .git 内容获取 gitdir
        let gitdir = std::fs::read_to_string(&git_path)?;
        // gitdir: gitdir: /path/to/main/.loom/worktrees/xxx/.git
        if gitdir.contains(".loom/worktrees") {
            return Ok(true); // 嵌套 worktree
        }
    }
    Ok(false)
}
```

嵌套检测策略：
- 如果检测到已在 Loom worktree 中，`isolation: "worktree"` 被静默忽略（降级为共享目录），并记录 warning 日志
- 如果检测到在非 Loom worktree（用户手动创建的）中，正常创建子 worktree（git 支持嵌套 worktree）

## 附录 E: Worktree CLI 命令面（v3 新增）

除了 `--worktree` 标志外，需要独立的 worktree 管理命令：

```
loom worktree list              # 列出所有活跃 worktree（路径、分支、状态、创建时间）
loom worktree cleanup [--all]   # 清理无变更的 worktree；--all 强制清理所有
loom worktree prune             # 清理 .trash/ 目录中的残留
loom worktree diff <name>       # 显示指定 worktree 与 base 的 diff
loom worktree merge <name>      # 将 worktree 分支合并回主分支
```

这些命令便于用户在 Agent 执行后手动检查和合并结果。

## 附录 F: 遥测与可观测性（v3 新增）

### F.1 结构化日志

每个 worktree 操作应输出结构化日志，便于问题排查：

```rust
tracing::info!(
    target: "loom::worktree",
    agent = %handle.agent_name,
    worktree_path = %handle.path.display(),
    branch = ?handle.branch,
    base_ref = ?config.base_ref,
    phase = "create",  // create | setup | cleanup | merge
    duration_ms = elapsed.as_millis(),
    "Worktree lifecycle event"
);
```

### F.2 Metrics（可选）

- `loom_worktree_active_count` — 当前活跃 worktree 数量
- `loom_worktree_create_duration_seconds` — 创建耗时
- `loom_worktree_cleanup_duration_seconds` — 清理耗时
- `loom_worktree_conflict_detected_total` — 冲突检测触发次数

## 附录 G: 测试策略（v3 新增）

### G.1 单元测试（不需要 git）

| 测试用例 | 覆盖点 |
|---------|-------|
| `sanitize_slug` 特殊字符/长度 | slug 生成安全性 |
| `detect_path_conflicts` 无冲突/有冲突 | 冲突检测正确性 |
| `schedule_waves` 2 任务无冲突 / 3 任务图着色 | 波次调度 |
| `parse_worktreeinclude` 空文件/注释/glob | include 文件解析 |

### G.2 集成测试（需要真实 git）

| 测试用例 | 覆盖点 |
|---------|-------|
| `worktree_create_and_cleanup` | 完整生命周期 |
| `worktree_fresh_has_no_changes` | BaseRef::Fresh 干净状态 |
| `worktree_multiple_parallel` | 3 个并行 worktree 无冲突 |
| `worktree_conflict_detection_overlapping_paths` | 预合并冲突检测 |
| `worktree_windows_junction_fallback` | Windows symlink 降级 |
| `worktree_nesting_detection` | 嵌套 worktree 检测 |
| `worktree_leaked_cleanup_on_startup` | 启动时清理泄漏 worktree |

### G.3 测试工具

使用 `tempfile::TempDir` 创建临时 git 仓库，避免污染真实项目：

```rust
fn setup_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    run_git(dir.path(), &["init"]).unwrap();
    run_git(dir.path(), &["config", "user.email", "test@loom.dev"]).unwrap();
    run_git(dir.path(), &["config", "user.name", "Test"]).unwrap();
    // 创建初始提交
    std::fs::write(dir.path().join("README.md"), "# test").unwrap();
    run_git(dir.path(), &["add", "."]).unwrap();
    run_git(dir.path(), &["commit", "-m", "init"]).unwrap();
    dir
}
```
