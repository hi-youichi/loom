# RFC: Out-of-Process Background Review

> 状态: Draft
> 日期: 2025-08-19
> 影响模块: `cli/src/run/background_review.rs`, `cli/src/args.rs`, `cli/src/main.rs`

## 背景与动机

当前后台审查使用 `tokio::spawn` 在同进程内异步执行。这带来几个问题：

- **主进程阻塞**：`main.rs:186` 的 `wait_for_pending_reviews()` 在进程退出前等待所有 review 完成，CLI 体验上等效于同步执行
- **资源竞争**：review agent 的 LLM 调用与主进程共享 Tokio runtime，可能影响后续命令的响应速度
- **故障隔离差**：review 的 panic 或内存泄漏会影响主进程稳定性
- **不支持持久队列**：进程异常退出后，未完成的 review 丢失

## 方案概览

将 review 执行从 `tokio::spawn`（同进程 async task）改为 `std::process::Command`（独立子进程），实现真正的后台执行。

```
当前:  main → tokio::spawn(review) → wait → exit
目标:  main → spawn_process("loom review daemon <session-id>") → 立即 exit
       子进程独立运行 → 结果写入文件
```

## 详细设计

### 1. 新增 CLI 子命令

在 `ReviewCommand` 枚举中新增 `Daemon` 变体：

```rust
// cli/src/args.rs
pub(crate) enum ReviewCommand {
    // ... 现有 Session, Sessions, History, Pending, Show ...
    
    /// Run review as a detached background process (internal use)
    Daemon {
        /// Session ID to review
        session_id: String,
        /// Path to config JSON file
        #[arg(long)]
        config: PathBuf,
    },
}
```

该子命令标记为 `hide(true)`（不在 help 中显示），仅供内部 `spawn_background_review` 调用。

### 2. 配置传递机制

`BackgroundReviewConfig` 序列化为 JSON 文件，通过 `--config` 参数传递给子进程：

```rust
// 写入临时配置文件
let config_path = config::home::loom_home()
    .join("data")
    .join("review")
    .join("pending")
    .join(format!("{}.config.json", session_id));
std::fs::write(&config_path, serde_json::to_string(&config)?)?;
```

子进程启动后读取配置、执行 review、完成后删除配置文件。

**序列化要求**：为 `BackgroundReviewConfig` 添加 `Serialize`/`Deserialize`：

```rust
#[derive(Serialize, Deserialize)]
pub struct BackgroundReviewConfig {
    // ... 现有字段不变
}
```

其中 `session_model: Option<ModelEntry>` 需确认 `ModelEntry` 已实现 `Serialize`/`Deserialize`。

### 3. 进程启动与 detach

```rust
// cli/src/run/background_review.rs
pub fn spawn_background_review(
    config: BackgroundReviewConfig,
    session_content: String,
    session_id: String,
) {
    // 1. 持久化 session_content
    let pending_dir = config::home::loom_home()
        .join("data")
        .join("review")
        .join("pending");
    std::fs::create_dir_all(&pending_dir).ok();
    
    let session_path = pending_dir.join(format!("{}.session.jsonl", session_id));
    std::fs::write(&session_path, &session_content).ok();
    
    // 2. 写入配置文件（不含 session_content）
    let config_path = pending_dir.join(format!("{}.config.json", session_id));
    std::fs::write(&config_path, serde_json::to_string(&config).unwrap_or_default()).ok();
    
    // 3. 启动子进程
    let exe = std::env::current_exe().unwrap_or_else(|_| "loom".into());
    let mut cmd = std::process::Command::new(exe);
    cmd.args([
        "review", "daemon",
        &session_id,
        "--config", &config_path.to_string_lossy(),
    ]);
    
    // 平台特定 detach
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x00000008); // DETACHED_PROCESS
    }
    
    #[cfg(unix)]
    {
        // std::process already creates independent process;
        // stdin/stdout/stderr redirect to log file
        let log_path = config::home::loom_home()
            .join("logs")
            .join(format!("review-{}.log", session_id));
        if let Ok(f) = std::fs::File::create(&log_path) {
            cmd.stdout(f.try_clone().unwrap_or_default());
            cmd.stderr(f);
        }
    }
    
    match cmd.spawn() {
        Ok(child) => {
            info!("Spawned review daemon PID={} for session={}", child.id(), session_id);
            // 不 wait，立即返回
        }
        Err(e) => {
            error!("Failed to spawn review daemon: {}", e);
        }
    }
}
```

### 4. Daemon 子命令处理器

```rust
// cli/src/review_cmd.rs (新增分支)
ReviewCommand::Daemon { session_id, config } => {
    let config: BackgroundReviewConfig = serde_json::from_str(
        &std::fs::read_to_string(&config)?
    )?;
    
    let pending_dir = config::home::loom_home()
        .join("data")
        .join("review")
        .join("pending");
    let session_path = pending_dir.join(format!("{}.session.jsonl", session_id));
    let session_content = std::fs::read_to_string(&session_path)?;
    
    // 执行 review workflow（复用现有逻辑）
    let result = run_background_review_workflow(&config, &session_content, &session_id).await;
    
    // 清理临时文件
    let _ = std::fs::remove_file(&session_path);
    let _ = std::fs::remove_file(&config);
    
    match result {
        Ok((summary, action_count, _, _, _)) => {
            info!("Review daemon completed: {} ({} actions)", summary, action_count);
            
            // Curator + Evolution（复用现有逻辑）
            if action_count > 0 {
                let _ = run_curator_if_needed(/* ... */);
            }
        }
        Err(e) => {
            error!("Review daemon failed: {}", e);
            std::process::exit(1);
        }
    }
}
```

### 5. 状态查询

利用现有 `ReviewHistory` 机制，`loom review pending` 改为扫描 `pending/` 目录：

```rust
// 检查是否有 daemon 子进程在运行
// Windows: 检查进程列表或 lock 文件
// Unix: 检查 PID 文件
```

更简单的方案：daemon 启动时写 PID 文件，完成时删除。

```
pending/
├── {session_id}.config.json    # 输入配置
├── {session_id}.session.jsonl  # 输入 session 内容
└── {session_id}.pid            # daemon PID（完成后删除）
```

### 6. 移除 PendingReviewRegistry

改为子进程后，不再需要同进程的 handle 跟踪：

- 删除 `PENDING_REVIEWS` 全局静态变量
- 删除 `PendingReviewRegistry` 结构体
- 删除 `main.rs:186` 的 `wait_for_pending_reviews()` 调用
- 进程可以立即退出

## 目录结构变更

```
~/.loom/data/review/
├── history.jsonl          # 现有，不变
└── pending/               # 新增
    ├── {session_id}.config.json
    ├── {session_id}.session.jsonl
    └── {session_id}.pid
```

## 迁移步骤

1. **Phase 1 — 基础设施**
   - `BackgroundReviewConfig` 添加 `Serialize`/`Deserialize`
   - 创建 `pending/` 目录结构
   - 新增 `ReviewCommand::Daemon` 子命令

2. **Phase 2 — 实现 daemon 模式**
   - 实现 daemon handler（复用 `run_background_review_workflow`）
   - 改造 `spawn_background_review` 为 `Command::spawn`
   - 添加 PID 文件管理

3. **Phase 3 — 清理**
   - 移除 `PendingReviewRegistry`
   - 移除 `main.rs` 中的 `wait_for_pending_reviews()`
   - 更新 `loom review pending` 扫描逻辑

4. **Phase 4 — 增强可选**
   - 添加 retry 机制（daemon 失败后由下次 CLI 调用触发重试）
   - 添加 `loom review daemon --status` 查询运行中 daemon 状态
   - 并发限制（限制同时运行的 daemon 数量）

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 配置文件包含 API key | 文件权限 600；完成后立即删除 |
| daemon 僵尸进程 | PID 文件 + 超时检测；下次 CLI 启动时清理 |
| 磁盘空间（session 文件残留） | 设置 TTL，`loom review pending` 启动时清理 >24h 的文件 |
| Windows detach 行为差异 | `DETACHED_PROCESS` flag 已验证可靠；添加集成测试 |

## 与现有方案的兼容性

- `loom review session <id>` 手动审查不受影响
- `loom review sessions --recent` 批量审查不受影响
- `ReviewHistory` 记录机制不变（daemon 完成后追加记录）
- `Curator` 和 `Evolution` 触发逻辑移入 daemon 子进程内部

## 替代方案

### A. 复用 `loom review session` 子命令（不新增 Daemon）

直接调用 `loom review session <id> --json`，通过环境变量传递配置。

**问题**：无法传递 `session_model`（`ModelEntry` 包含 tier/family 信息，不适合环境变量）。

### B. Loom Serve 模式做 worker

将 review 提交给长期运行的 `loom serve` 进程处理。

**问题**：依赖 serve 模式已启动；增加架构复杂度；当前 serve 主要面向 Telegram bot 场景。

### C. 保留 tokio::spawn + 去掉 wait

最小改动：仅删除 `main.rs:186` 的 wait 调用。

**问题**：进程退出时 tokio runtime 销毁，review 任务被强制终止，结果不可靠。
