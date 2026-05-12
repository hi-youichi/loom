---
sidebar_position: 2
title: "Shell 后台执行与超时"
description: "Shell 命令超时后保持后台运行，持久化 stdout/stderr 输出文件"
---

# Shell 后台执行与超时

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 长时间构建 | ✅ 完美支持 | `cargo build`、`npm install` 等耗时命令 |
| 数据处理 | ✅ 完美支持 | ETL 脚本、日志分析等长时间运行任务 |
| 测试套件 | ✅ 完美支持 | 大型测试套件执行超过超时限制 |
| 服务部署 | ✅ 完美支持 | 部署脚本、Docker 构建等 |
| 快速命令 | ⚠️ 不需要 | 超时内完成的命令使用默认行为即可 |

## 核心问题

Loom 的 Bash 工具（`loom/src/tools/bash/executor.rs`）在命令执行超时后会 **直接 kill 子进程**，导致：

1. **输出丢失** — 已产生的 stdout/stderr 随进程销毁
2. **状态不可追踪** — Agent 无法知道进程最终执行结果
3. **无法恢复** — 必须重新执行整个命令

```
当前行为（超时 → kill → 丢失）:

  Agent ──► BashTool ──► sh -c "cargo build"
                              │
                          timeout (120s)
                              │
                           kill() ← 进程被终止，输出丢失
                              │
  Agent ◄── Error("command timed out")
```

## 方案设计

### 目标行为

超时后 **不 kill 进程**，将 stdout/stderr 写入临时文件，返回 PID 和文件路径给 Agent。

```
新行为（超时 → detach → 返回文件路径）:

  Agent ──► BashTool ──► sh -c "cargo build"
                              │
                          timeout (120s)
                              │
                     detach + 写入文件
                              │
  Agent ◄── Ok({
    pid: 12345,
    stdout_file: "/tmp/loom-shell-xxx.stdout",
    stderr_file: "/tmp/loom-shell-xxx.stderr",
    partial_output: "...",
  })
                              │
                     进程继续在后台运行
                              │
  Agent ──► cat /tmp/loom-shell-xxx.stdout  ← 随时读取最新输出
```

### 架构变更

#### 1. 输出结构扩展

当前 `ShellOutput` 只保存内存中的字符串：

```rust
struct ShellOutput {
    stdout: String,
    stderr: String,
}
```

扩展为支持文件持久化的结构：

```rust
struct ShellOutput {
    stdout: String,
    stderr: String,
    pid: Option<u32>,
    timed_out: bool,
    stdout_file: Option<PathBuf>,
    stderr_file: Option<PathBuf>,
}
```

#### 2. 执行流程变更

核心改造在 `run_spawned_shell_command` 函数中：

```rust
async fn run_spawned_shell_command(
    mut cmd: tokio::process::Command,
    timeout_ms: u64,
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    // 1. 创建临时输出文件
    let run_id = Uuid::new_v4();
    let stdout_path = temp_dir.join(format!("loom-shell-{}.stdout", run_id));
    let stderr_path = temp_dir.join(format!("loom-shell-{}.stderr", run_id));

    // 2. 重定向 stdout/stderr 到文件
    let stdout_file = File::create(&stdout_path)?;
    let stderr_file = File::create(&stderr_path)?;
    cmd.stdout(stdout_file);
    cmd.stderr(stderr_file);

    let mut child = cmd.spawn()?;

    // 3. 启动后台 tail task 读取文件内容到 buffer
    let buffer = Arc::new(Mutex::new(String::new()));
    let tail_handle = spawn_tail_task(&stdout_path, buffer.clone());

    // 4. select! 等待结果
    let result = tokio::select! {
        // 用户取消 → kill 进程 + 清理文件
        _ = kill_rx.changed() => {
            let _ = child.kill().await;
            return Err(ToolSourceError::Transport("command cancelled".into()));
        }
        // 超时 → detach 进程，返回 PID + 文件路径
        _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
            let pid = child.id();
            let partial = buffer.lock().await.clone();
            return Ok(ShellOutput {
                stdout: partial,
                stderr: String::new(),
                pid,
                timed_out: true,
                stdout_file: Some(stdout_path),
                stderr_file: Some(stderr_path),
            });
        }
        // 正常退出 → 读取完整输出
        status = child.wait() => {
            tail_handle.abort();
            let stdout = tokio::fs::read_to_string(&stdout_path).await?;
            let stderr = tokio::fs::read_to_string(&stderr_path).await?;
            // 清理临时文件
            let _ = tokio::fs::remove_file(&stdout_path).await;
            let _ = tokio::fs::remove_file(&stderr_path).await;
            Ok(ShellOutput {
                stdout, stderr,
                pid: None, timed_out: false,
                stdout_file: None, stderr_file: None,
            })
        }
    };
}
```

#### 3. 超时返回值格式

超时后 Agent 收到的 `ToolCallContent` 文本内容：

```json
{
  "text": "Command timed out after 120000ms (PID: 12345)\n\n--- Partial Output (3.2KB) ---\n   Compiling loom v0.1.0\n   Compiling cli v0.1.0\n...\n\n--- Output Files ---\nstdout: /tmp/loom-shell-a1b2c3.stdout\nstderr: /tmp/loom-shell-a1b2c3.stderr\n\nUse `cat /tmp/loom-shell-a1b2c3.stdout` to read the latest output.\nUse `kill 12345` to stop the process."
}
```

### 文件管理

#### 文件命名和位置

```
$TMPDIR/loom-shell/
├── loom-shell-550e8400-e29b-41d4-a716-446655440000.stdout
├── loom-shell-550e8400-e29b-41d4-a716-446655440000.stderr
├── loom-shell-6ba7b810-9dad-11d1-80b4-00c04fd430c8.stdout
└── loom-shell-6ba7b810-9dad-11d1-80b4-00c04fd430c8.stderr
```

#### 清理策略

| 策略 | 触发条件 | 行为 |
|------|---------|------|
| 即时清理 | 进程正常退出 | 读取输出后立即删除文件 |
| 延迟清理 | 进程后台运行结束 | 文件保留 10 分钟后自动清理 |
| 会话清理 | 会话结束 | 清理该会话创建的所有文件 |
| 主动清理 | Agent 执行 cleanup | 立即删除指定文件 |

#### 后台进程管理

| 操作 | Agent 命令 | 说明 |
|------|-----------|------|
| 查看输出 | `cat /tmp/loom-shell-xxx.stdout` | 读取最新输出 |
| 查看进度 | `tail -f /tmp/loom-shell-xxx.stdout` | 实时跟踪输出 |
| 检查状态 | `kill -0 {pid}` | 检查进程是否仍在运行 |
| 终止进程 | `kill {pid}` | 终止后台进程 |

### 配置项

在 `config.toml` 中添加配置：

```toml
[tools.bash]
# 超时后是否保持后台运行（默认 false = 兼容旧行为）
background_on_timeout = true

# 后台输出文件目录（默认 $TMPDIR/loom-shell/）
output_dir = "/tmp/loom-shell"

# 后台进程文件保留时间（默认 "10m"）
output_retention = "10m"

# 最大后台进程数（默认 10）
max_background_processes = 10
```

### 涉及修改的文件

| 文件 | 变更说明 |
|------|---------|
| `loom/src/tools/bash/executor.rs` | 核心改造：超时不 kill，输出写文件，返回 PID |
| `loom/src/tools/bash/mod.rs` | `BashTool::call` 适配新的 `ShellOutput` 结构 |
| `loom/src/tools/bash/output_files.rs` | **新增**：文件管理（创建/清理/tail 读取） |
| `config/src/lib.rs` | 新增 `tools.bash` 配置段 |

## 代码示例

### Agent 使用场景

Agent 调用 Bash 工具执行长时间命令：

```json
{
  "name": "bash",
  "arguments": {
    "command": "cargo build --release",
    "timeout_ms": 120000
  }
}
```

超时后收到响应：

```
Command timed out after 120000ms (PID: 48321)

--- Partial Output (3.2KB) ---
   Compiling loom v0.1.0 (/Users/dev/loom)
   Compiling cli v0.1.0 (/Users/dev/cli)
   Compiling serve v0.1.0 (/Users/dev/serve)
   ...

--- Output Files ---
stdout: /tmp/loom-shell-550e8400.stdout
stderr: /tmp/loom-shell-550e8400.stderr

Use `cat /tmp/loom-shell-550e8400.stdout` to read the latest output.
Use `kill 48321` to stop the process.
```

Agent 继续跟踪进度：

```json
{
  "name": "bash",
  "arguments": {
    "command": "tail -5 /tmp/loom-shell-550e8400.stdout && kill -0 48321 && echo 'RUNNING' || echo 'DONE'"
  }
}
```

```json
{
  "name": "bash",
  "arguments": {
    "command": "cat /tmp/loom-shell-550e8400.stdout"
  }
}
```

### 最小可行方案

如果需要最小改动，可以先只修改 `executor.rs` 中的超时分支：

```rust
// 替换超时 kill 为 detach
_ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
    let pid = child.id();

    // stdout/stderr 已经在文件中，进程继续运行
    // 不调用 child.kill()，让进程自然完成
    // 注意：必须 drop child 避免 drop 时自动 kill

    let partial = tokio::fs::read_to_string(&stdout_path).await
        .unwrap_or_default();

    Ok(ShellOutput {
        stdout: partial,
        stderr: String::new(),
        pid,
        timed_out: true,
        stdout_file: Some(stdout_path),
        stderr_file: Some(stderr_path),
    })
}
```

## 注意事项

- **文件描述符**：进程 stdout/stderr 重定向到文件后，需要确保文件正确 flushed
- **进程泄漏**：后台进程需要 Agent 主动 kill，否则会一直运行
- **并发限制**：通过 `max_background_processes` 限制同时后台运行的进程数
- **安全性**：输出文件可能包含敏感信息，应设置合适的文件权限（0600）
- **兼容性**：默认关闭 `background_on_timeout`，保持向后兼容

---

## 相关概念

- [工具系统](./tool-system.md) — Bash 工具的注册和调用机制
- [MCP 协议](./mcp.md) — 第三方服务集成标准
- [ReAct 运行模式](../core/react.md) — 工具与智能体的集成方式

---

**下一页**: [MCP 协议集成](./mcp.md) | [工具系统](./tool-system.md)
