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

超时后 **不 kill 进程**，将 stdout/stderr 重定向到项目 `.loom/shell/` 下的文件，返回 PID 和文件路径给 Agent。

```
新行为（超时 → detach → 返回文件路径）:

  Agent ──► BashTool ──► sh -c "cargo build"
                              │
                          timeout (120s)
                              │
                     detach + 保留输出文件
                              │
  Agent ◄── Ok(
    "Command timed out (PID: 12345)
     ...
     stdout: .loom/shell/2025-01-15/550e8400.stdout
     stderr: .loom/shell/2025-01-15/550e8400.stderr
     Use `cat .loom/shell/2025-01-15/550e8400.stdout` to read the latest output.
     Use `kill 12345` to stop the process."
  )
                              │
                     进程继续在后台运行
                              │
  Agent ──► cat .loom/shell/2025-01-15/550e8400.stdout  ← 随时读取最新输出
```

### 设计决策

#### 输出捕获方式：文件重定向（方案 A）

三种候选方案对比：

| | A: 重定向文件 | B: 先 pipe 后切文件 | C: pipe + tee |
|---|---|---|---|
| 原理 | spawn 前创建文件，`cmd.stdout(File)` | 先 pipe 到内存，超时再写文件 | 读 pipe 同时写文件和内存 |
| 正常完成路径 | 写文件 → 读文件 → 删文件 | 和现在一样 | 读 pipe + 写文件 → 读内存 → 删文件 |
| 超时路径 | 从文件读 → 保留文件 | ❌ pipe drop 后进程崩溃 | 从文件读 → 保留文件 |
| 后续读进度 | ✅ 文件持续被写入 | ❌ 不可行 | ✅ reader task 持续写文件 |
| IO 开销 | 多一次磁盘写+读 | N/A | 多一次磁盘写 |
| 实现复杂度 | **低** | 不可行 | 中 |

**选择方案 A**：进程直接写文件，最简实现，满足所有需求。方案 B 因 pipe 断开后进程收 SIGPIPE 崩溃而不可行。方案 C 功能更强但实现复杂，后续可按需升级。

#### 配置方式：默认 detach，禁止无超时

| 选项 | 说明 | 缺点 |
|------|------|------|
| 全局配置 `background_on_timeout` | LLM 无法按调用选择 | 灵活性不足 |
| 工具参数 `detach` | LLM 每次调用可决定 | 参数冗余 |
| **默认行为即 detach** | 无需配置，最简 | 回退需改代码 |

**选择默认 detach**：没有场景需要"超时后主动 kill"——需要结果的命令 detach 让它跑完更好，不需要的可以手动 `kill PID`。保持最简，有需求再加参数。

**禁止无超时**：`timeout_ms = 0` 或 `None` 统一使用默认值 `120000`（120 秒）。所有命令都有超时，确保超时 detach 机制始终生效。

#### 进程 detach：`std::mem::forget`

`tokio::process::Child` 在 Windows 上默认 `kill_on_drop = true`，在 Unix 上为 false。超时后需要让进程继续运行，不能触发 drop：

```rust
let pid = child.id();
let _ = child.stdin.take(); // 关闭 stdin，避免进程等待输入
std::mem::forget(child);    // 不调用 drop，不会 kill 进程
```

配合文件重定向方案 A，不再需要 `stdout_reader` / `stderr_reader` subtask（进程直接写文件），因此没有 subtask 泄漏问题。

#### 文件存储位置：项目 `.loom/shell/` 目录

| 选项 | 说明 | 缺点 |
|------|------|------|
| `/tmp/loom-shell/` | 系统临时目录，OS 自动清理 | 路径长，LLM 需越过工作目录；跨项目输出混在一起 |
| **项目 `.loom/shell/`** | 项目级目录，LLM 直接用相对路径 | 需要 `.gitignore` 忽略 |

**选择 `.loom/shell/`**：
- 路径短，LLM 在工作目录下直接 `cat .loom/shell/.../xxx.stdout`，无需输入长绝对路径
- 输出属于项目上下文，不同项目的后台输出互不干扰
- `.loom/` 已在 `.gitignore` 中，不会污染 git
- 按日期组织子目录，便于后续按日期批量清理

当 `working_folder` 为 None 时，fallback 到 `std::env::current_dir()`。

#### Trait 改动：不改 CommandExecutor

`CommandExecutor` trait 的 `execute` 方法签名不变，只改 `LocalCommandExecutor` 内部的 `run_spawned_shell_command` 函数（private）。其他执行器（`TerminalCommandExecutor`、`AcpBridgeCommandExecutor`、`LocalPowerShellExecutor`）暂不改动。

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
    pid: Option<u32>,            // Some 当超时 detach 时
    timed_out: bool,              // true 当超时
    stdout_file: Option<PathBuf>, // 输出文件路径，超时时 Some
    stderr_file: Option<PathBuf>, // 错误文件路径，超时时 Some
}
```

正常完成与超时 detach 的输出对比：

```rust
// 超时 detach
ShellOutput {
    stdout: "partial output so far",
    stderr: "",
    pid: Some(12345),
    timed_out: true,
    stdout_file: Some(".loom/shell/2025-01-15/550e8400.stdout"),
    stderr_file: Some(".loom/shell/2025-01-15/550e8400.stderr"),
}
```

#### 2. 执行流程变更

核心改造在 `run_spawned_shell_command` 函数中（`loom/src/tools/bash/executor.rs`）。所有命令都走文件重定向路径，总是有超时：

```rust
async fn run_spawned_shell_command(
    mut cmd: tokio::process::Command,
    workdir: Option<&Path>,   // 工作目录，也用于确定 .loom/shell/ 位置
    timeout_ms: u64,            // 禁止无超时，默认 120000
    ctx: Option<&ToolCallContext>,
) -> Result<ShellOutput, ToolSourceError> {
    // 1. 确定输出文件目录（.loom/shell/{date}/）
    let base_dir = workdir...unwrap_or_else(|| current_dir());
    let shell_dir = shell_output_dir(&base_dir);
    tokio::fs::create_dir_all(&shell_dir).await?;

    // 2. 创建输出文件（权限 0600）
    let run_id = generate_run_id();  // 时间戳+PID 的 8 位 hex
    let stdout_path = shell_dir.join(format!("{}.stdout", run_id));
    let stderr_path = shell_dir.join(format!("{}.stderr", run_id));
    let stdout_file = create_output_file(&stdout_path)?;
    let stderr_file = create_output_file(&stderr_path)?;

    // 3. 重定向 stdout/stderr 到文件，piped stdin（用于超时 detach 时关闭）
    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));
    cmd.stdin(Stdio::piped());

    let mut child = cmd.spawn()?;
    let pid = child.id();

    // 4. select! 等待结果（无 timeout_ms==0 分支）
    tokio::select! {
        // 用户取消 → kill 进程 + 清理文件
        _ = kill_rx.changed() => { kill + cleanup }
        // 超时 → detach 进程，返回 PID + 文件路径
        _ = sleep(timeout) => {
            let pid_val = child.id().unwrap_or(pid.unwrap_or(0));
            child.stdin.take();          // 关闭 stdin
            std::mem::forget(child);    // 不 kill，让进程继续运行
            let partial = read_to_string(stdout_path).await;
            return Ok(ShellOutput { pid: Some(pid_val), timed_out: true, ... });
        }
        // 正常退出 → 读取完整输出 + 清理文件
        status = child.wait() => {
            let stdout = read_to_string(stdout_path).await;
            remove_file(stdout_path).await;
            remove_file(stderr_path).await;
            Ok(ShellOutput { pid: None, timed_out: false, ... })
        }
    }
}
```

#### 3. 返回值格式化

`LocalCommandExecutor::execute` 将 `ShellOutput` 转为 `ToolCallContent::text()`，超时时格式化为人类可读文本：

```rust
impl CommandExecutor for LocalCommandExecutor {
    async fn execute(...) -> Result<ToolCallContent, ToolSourceError> {
        let output = run_shell_command(command, workdir_str.as_deref(), timeout, ctx).await?;

        let text = if output.timed_out {
            format_timed_out_output(&output)
        } else if output.stderr.is_empty() {
            output.stdout.clone()
        } else if output.stdout.is_empty() {
            format!("stderr:\n{}", output.stderr)
        } else {
            format!("stdout:\n{}\nstderr:\n{}", output.stdout, output.stderr)
        };

        Ok(ToolCallContent::text(text))
    }
}

fn format_timed_out_output(output: &ShellOutput) -> String {
    let pid = output.pid.unwrap_or(0);
    let size = format_size(output.stdout.len());
    let mut text = format!("Command timed out (PID: {})\n", pid);
    text.push_str(&format!("\n--- Partial Output ({}) ---\n", size));
    text.push_str(&output.stdout);
    if let Some(stdout_file) = &output.stdout_file {
        text.push_str(&format!("\n--- Output Files ---\nstdout: {}\n", stdout_file.display()));
    }
    if let Some(stderr_file) = &output.stderr_file {
        text.push_str(&format!("stderr: {}\n", stderr_file.display()));
    }
    if let Some(stdout_file) = &output.stdout_file {
        text.push_str(&format!(
            "\nUse `cat {}` to read the latest output.\nUse `kill {}` to stop the process.",
            stdout_file.display(), pid
        ));
    }
    text
}
```

#### 4. 超时返回值示例

超时后 Agent 收到的 `ToolCallContent` 文本内容：

```
Command timed out (PID: 12345)

--- Partial Output (3.2KB) ---
   Compiling loom v0.1.0
   Compiling cli v0.1.0
...

--- Output Files ---
stdout: .loom/shell/2025-01-15/550e8400.stdout
stderr: .loom/shell/2025-01-15/550e8400.stderr

Use `cat .loom/shell/2025-01-15/550e8400.stdout` to read the latest output.
Use `kill 12345` to stop the process.
```

选择人类可读文本而非 JSON 的理由：LLM 天然理解自然语言格式，无需额外解析指令；Agent 可直接按提示操作。使用相对路径：LLM 执行 bash 时默认在工作目录下，相对路径直接可用，比绝对路径更短更自然。

### 文件管理

#### 文件命名和位置

```
.loom/
├── agents/              # 已有：Agent 配置
├── skills/              # 已有：Skill 定义
└── shell/               # 新增：后台 shell 输出
    ├── 2025-01-15/
    │   ├── 550e8400.stdout
    │   ├── 550e8400.stderr
    │   └── 6ba7b810.stdout
    └── 2025-01-16/
        └── a3f2c910.stdout
```

- 目录：项目根目录下 `.loom/shell/{date}/`，`date` 格式 `YYYY-MM-DD`
- 文件名：`{short_id}.{stream}`，8 字符十六进制短 ID
- `working_folder` 为 None 时，fallback 到 `std::env::current_dir()`
- 权限：创建时设置 `0600`，防止其他用户读取敏感输出

#### 短 ID 碰撞分析

8 个十六进制字符 = 32 bit = ~43 亿个值。同一天内的碰撞概率：

| 同一目录下文件数 | 碰撞概率 |
|---|---|
| 100 | ~0.0001% |
| 1,000 | ~0.01% |
| 10,000 | ~1% |

单个项目一天内不太可能超过几百个超时文件，8 字符足够安全。如需更高保障可扩展为 12 字符。

#### 清理策略

| 策略 | 触发条件 | 行为 |
|------|---------|------|
| 即时清理 | 进程正常退出 | 读取输出后立即删除文件 |
| 按日期清理 | 用户手动或自动化 | `rm -rf .loom/shell/2025-01-14/` 删除旧日期目录 |
| gitignore | `.loom/.gitignore` 追加 `shell/` | shell 输出不会被 git 跟踪 |

正常完成的命令：读文件 → 删文件 → 返回结果，和当前 pipe 模式的资源释放语义一致。

超时 detach 的命令：文件保留在日期目录中。LLM 通过 `cat` 读取最终结果后，进程通常已结束，文件不再变化。旧日期目录可手动或脚本清理。

#### 后台进程管理

| 操作 | Agent 命令 | 说明 |
|------|-----------|------|
| 查看输出 | `cat .loom/shell/2025-01-15/550e8400.stdout` | 读取最新输出 |
| 查看进度 | `tail -f .loom/shell/2025-01-15/550e8400.stdout` | 实时跟踪输出 |
| 检查状态 | `kill -0 {pid}` | 检查进程是否仍在运行（退出码 0=运行中） |
| 终止进程 | `kill {pid}` | 终止后台进程 |
| 批量清理 | `rm -rf .loom/shell/2025-01-14/` | 删除指定日期的所有输出 |

### 涉及修改的文件

| 文件 | 变更说明 |
|------|---------|
| `loom/src/tools/bash/executor.rs` | 核心改造：`ShellOutput` 扩展 + 文件重定向 + 超时 detach + 格式化 |
| `loom/tools/bash.yaml` | 工具描述更新：说明超时后进程进入后台，提供 PID 和文件路径 |
| `loom/src/tools/bash/mod.rs` | `BashTool::call` 中 `spec()` 的 description 更新 |

不需要新增文件，不需要改 `CommandExecutor` trait，不需要改配置系统。

关键实现细节：
- **`timeout_ms = 0` 或 `None`**：统一使用 `DEFAULT_TIMEOUT_MS`（120000ms），禁止无超时
- **`generate_run_id()`**：使用 `(nanos_timestamp as u32).wrapping_add(process_id)` 生成 8 字符 hex ID，避免引入额外依赖
- **`create_output_file()`**：使用 `OpenOptions::create_new(true).mode(0o600)` 创建文件，确保排他创建和权限安全
- **`make_relative()`**：将绝对路径转为相对路径（基于 `base_dir`），使 LLM 收到的路径更短更自然

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
Command timed out (PID: 48321)

--- Partial Output (3.2KB) ---
   Compiling loom v0.1.0 (/Users/dev/loom)
   Compiling cli v0.1.0 (/Users/dev/cli)
   Compiling serve v0.1.0 (/Users/dev/serve)
   ...

--- Output Files ---
stdout: .loom/shell/2025-01-15/550e8400.stdout
stderr: .loom/shell/2025-01-15/550e8400.stderr

Use `cat .loom/shell/2025-01-15/550e8400.stdout` to read the latest output.
Use `kill 48321` to stop the process.
```

Agent 继续跟踪进度：

```json
{
  "name": "bash",
  "arguments": {
    "command": "tail -5 .loom/shell/2025-01-15/550e8400.stdout && kill -0 48321 && echo 'RUNNING' || echo 'DONE'"
  }
}
```

Agent 读取完整输出：

```json
{
  "name": "bash",
  "arguments": {
    "command": "cat .loom/shell/2025-01-15/550e8400.stdout"
  }
}
```

Agent 终止后台进程：

```json
{
  "name": "bash",
  "arguments": {
    "command": "kill 48321"
  }
}
```

## YAML 描述更新

当前 `loom/tools/bash.yaml`：

```yaml
description: |
  Executes a shell command in a subprocess with optional workdir and timeout.
  - Commands run in the working folder by default...
  - Optional timeout in milliseconds (default 120000).
```

更新为：

```yaml
description: |
  Executes a shell command in a subprocess with optional workdir and timeout.
  - Commands run in the working folder by default. Use workdir to run in a different directory.
    Prefer workdir over "cd ... && command".
  - Use for git, npm, cargo, docker, etc. Do NOT use for file read/write/search — use read, grep, glob, edit instead.
  - Quote paths with spaces (e.g. rm "path with spaces/file.txt").
  - Optional timeout in milliseconds (default 120000).
  - **When a command exceeds the timeout, it continues running in the background.**
    The tool returns the PID and output file paths. You can:
    - Use `cat <stdout_file>` to read the latest output
    - Use `kill <PID>` to stop the process
    - Use `kill -0 <PID>` to check if the process is still running
```

## 注意事项

- **文件刷新**：进程写文件是 OS buffered write，LLM `cat` 读取时内容取决于 OS flush 时机（通常秒级）。如果需要更实时的跟踪，可后续升级为 pipe + tee 方案
- **进程泄漏**：后台进程需要 Agent 主动 `kill`，否则会一直运行。提示词中已包含操作指引
- **安全性**：输出文件可能包含敏感信息，创建时设置 `0600` 权限
- **Windows 兼容**：`tokio::process::Child` 在 Windows 上默认 `kill_on_drop = true`，使用 `std::mem::forget` 规避；`kill -0` 在 Windows 上不可用，需要用 `tasklist /FI "PID eq {pid}"` 替代
- **向后兼容**：超时返回值从 `Error` 变为 `Ok(文本)`，调用方需要适配。但 `CommandExecutor::execute` 的返回类型不变（`Result<ToolCallContent, ToolSourceError>`），所以外部接口兼容
- **禁止无超时**：`timeout_ms = 0` 现在使用默认值 120000ms，不再允许无限等待
- **stdin 统一 piped**：所有平台都设置 `Stdio::piped()` 用于 stdin，超时时 `child.stdin.take()` 关闭管道防止进程挂起
- **只读文件系统**：`.loom/shell/` 创建失败时直接返回错误，未实现 fallback（极少场景，后续可加）

---

## 相关概念

- [工具系统](./tool-system.md) — Bash 工具的注册和调用机制
- [MCP 协议](./mcp.md) — 第三方服务集成标准
- [ReAct 运行模式](../core/react.md) — 工具与智能体的集成方式

---

**下一页**: [MCP 协议集成](./mcp.md) | [工具系统](./tool-system.md)
