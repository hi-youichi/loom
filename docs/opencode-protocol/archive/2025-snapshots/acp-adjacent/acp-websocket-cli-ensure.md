# `loom acp --websocket` 开发方案

> 状态：草案 v1（2025-08）
> 目标：在 `loom acp` 上提供 “确保 WebSocket 入口可用” 的开关，IDE/CLI 通过 `ws://host:port/acp`
> 接入 ACP 时无需手工 `cargo run -p loom-server`。

---

## 1. 动机与现状

- `loom-server` 已在 HTTP/SSE 端口上提供 `GET /acp` WebSocket（`apps/server/src/routes.rs:29`，
  handler 在 `apps/server/src/handlers/acp.rs:23`），是面向 IDE / 远程 CLI 的主要 ACP 接入方式。
- `loom acp` 子命令目前只支持 stdio 模式（`apps/acp/src/server.rs:105`）。
- 用户痛点：IDE 启动时常常 `ws://127.0.0.1:18081/acp` 拒连，需要切到终端手动拉起 `loom-server`。
- 现存 `--server`（`apps/cli/src/args.rs:132`）只做客户端直连，**不会**自动拉起服务端。
- 仓库没有生产级 “探测端口 + 自动 spawn” 工具；现有 detached 进程模板见
  `experimental/curator/src/review.rs:497`。

本文档定义一个独立开关 `loom acp --websocket`，让 CLI 在进入 ACP 协议前
保证 `loom-server` 已就绪；不再接管协议本身。

---

## 2. 目标 / 非目标

### 目标

- 提供 `loom acp --websocket` 子命令：
  - 探测目标端口是否被健康的 `loom-server` 占用；
  - 未占用则后台启动 `loom-server` 并轮询就绪；
  - 启动完成后退出 `0`，不进入 stdio 循环。
- 默认端口 `127.0.0.1:3030`；可用全局 `--server URL` 覆盖（与现有 HTTP/SSE 客户端共享）。
- 仅作用于 loopback、非零端口；远程 / `wss` 报错退出，绝不 spawn。
- 输出走 stderr，stdout 保持干净（满足 ACP JSON-RPC 契约，见 `apps/cli/src/main.rs:71`）。

### 非目标

- 不接管 ACP 协议：保持 `apps/acp/src/server.rs:105` 的 stdio server 不变。
- 不替代 IDE/CLI 的 WS 连接：仍由各客户端自行 `connect_async` 到 `/acp`。
- 不实现 “运行中断线自动重启”：下一次 `loom acp --websocket` 重新探测即可。
- 不引入跨进程引用计数 / systemd / Windows Service 集成。
- 不修改 `apps/server` 任何 handler / 路由。

---

## 3. CLI 设计

### 3.1 参数

在 `AcpArgs`（`apps/cli/src/args.rs:740`）上新增：

```rust
/// Ensure a loom-server with /acp WebSocket is running, then exit.
#[arg(long)]
pub(crate) websocket: bool,
```

地址解析顺序（与现有 `--server` 一致）：

1. `--server <URL>`（`apps/cli/src/args.rs:132`，`global = true`）；
2. `LOOM_SERVER_URL` 环境变量；
3. 兜底 `http://127.0.0.1:3030`。

WS 端点固定由 HTTP base 派生：`ws://{host}:{port}/acp`。

不引入新的 `--port` / `--ws-url` / `--bind`：保持布尔开关语义，避免与 `--server` 冲突。


#### 3.1.1 互斥与组合行为

`--websocket` 与 `AcpCmd` 子命令（目前仅 `reload`）以及 `--show-log-dir` 互斥。
具体由 clap 在 `AcpArgs` 上以 `ArgGroup` 形式声明，确保同时给定时直接报错并打印
`the following arguments cannot be used together`，避免在 `apps/cli/src/main.rs:73`
分支判断时落入二义状态：

```rust
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct AcpArgs {
    /// Print the log (and PID) file directory and exit.
    #[arg(long, group = "acp_action")]
    pub(crate) show_log_dir: bool,

    /// Ensure a loom-server with /acp WebSocket is running, then exit.
    #[arg(long, group = "acp_action")]
    pub(crate) websocket: bool,

    #[command(subcommand)]
    pub(crate) cmd: Option<AcpCmd>,
}
```

> 实际字段保留现有名与类型；上述代码块仅说明 `ArgGroup` 思路。
> 未来若新增 `AcpCmd::Stop` / `AcpCmd::Status` 等 ws-only 子命令，
> 需要把 `cmd` 也加入同一 `ArgGroup`，或在 §3.2 分派点显式 panic / 早返回。

| 组合                                          | 行为                                                  |
|----------------------------------------------|-------------------------------------------------------|
| `loom acp --websocket`                       | 主路径（§4 流程）                                     |
| `loom acp --websocket reload`                | clap 拒绝（`cannot be used with`）                    |
| `loom acp --websocket --show-log-dir`        | clap 拒绝                                             |
| `loom acp reload`                            | 既有行为，往 stdio PID 发 SIGHUP                      |
| `loom acp`（无参数）                          | 既有行为，进入 stdio 循环                             |

### 3.2 分派点

`apps/cli/src/main.rs:73` ACP 早期分支内，在现有 `show_log_dir` / `Reload` 分支之后插入：

```rust
if acp_args.websocket {
    let server = args.server.clone()
        .or_else(|| std::env::var("LOOM_SERVER_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:3030".into());
    return server_process::ensure_loom_server(&server)
        .map(|_| ())
        .map_err(|e| {
            eprintln!("loom acp --websocket: {e}");
            std::process::exit(1);
        });
}
```

### 3.3 退出码

| Code | 含义                                                |
|------|-----------------------------------------------------|
| 0    | 就绪（已存在 healthy 或本次启动成功）                |
| 2    | 端口被非 Loom 进程占用                              |
| 3    | 鉴权失败（探测返回 401 / 403）                      |
| 4    | 启动失败 / 就绪轮询超时                              |
| 5    | 找不到 `loom-server` binary                         |

---

## 4. 运行时行为

### 4.1 总体流程

```
解析 URL → 派生 host / port / http_base / ws_base
  ↓
获取每端口进程锁 $LOOM_HOME/acp/loom-server-{host}-{port}.lock
  ↓
probe_loose() ── 拒连 ──▶ 尝试 spawn   (PROBE_TIMEOUT_MS = 1500, 见 §4.2)
  │ 接受
  ↓
probe_ready() ── healthy + external-kernel ──▶ ready
  │ 401/403                          ──▶ exit 3
  │ 非 Loom 身份                     ──▶ exit 2
  │ 超时 / 解析失败                  ──▶ 仍尝试 spawn（不强杀）
  ↓
写 .meta.json.tmp (phase: spawning)
  ↓
spawn loom-server serve --host ... --port ... --directory <cwd>
  ↓
poll /global/health 200ms 间隔, 上限 10s
  ↓ ready                                ↓ timeout
rename .tmp → .meta.json (phase: ready)   exit 4 + 输出日志路径
exit 0
```

### 4.2 探测契约

- 探测超时：`PROBE_TIMEOUT_MS = 1500`（常量），对 TCP 连接 / HTTP 请求统一生效。
  loopback 本地端口在毫秒级响应；超过阈值一律判 `Refused` 或 `Timeout`，
  立即进入 spawn 路径，避免在已死端口上等系统 TCP SYN 重传（数十秒）。
- `/global/health`（`apps/server/src/handlers/health.rs:12`）返回 `{"healthy": true}`。
- `/instance`（`apps/server/src/handlers/instance.rs:6`）返回 `kind == "external-kernel"`,
  `id` 与版本作为强身份；缺一项视为非 Loom。
- 认证：`require_valid_token` middleware（`apps/server/src/routes.rs:909`）要求 Bearer 或 Basic；
  CLI 探测必须透传环境变量 `LOOM_AUTH_TOKEN` / `OPENCODE_SERVER_PASSWORD`，**不接受明文凭据**。
- WS 探测：避免 `connect_async` 到 `/acp`，因为 attach 会抢占现有 lease
  （`apps/server/src/acp_hub.rs:61`）；真正使用 WS 连接的客户端各自负责握手。

### 4.3 进程锁

- 路径: `$LOOM_HOME/acp/loom-server-{host}-{port}.lock`
- 创建：`OpenOptions::new().create_new(true).write(true).open(path)`，失败即视为持锁中。
- 持锁方负责二次 `probe_ready()`（关闭 TOCTOU），然后 `spawn` 或直接复用。
- 锁内附带内容 `{pid, started_at_unix}`，仅作诊断，不作为存活判断。

### 4.4 Spawn

- 解析 `loom-server` binary：
  1. `LOOM_SERVER_BIN` 环境变量；
  2. `current_exe()` 同目录的 `loom-server(.exe)`；
  3. `PATH` 中的 `loom-server(.exe)`。
  找不到 → `exit 5`。
- 固定参数：`serve --host {host} --port {port} --directory <cwd>`。
- 继承环境（认证、模型、TLS 等），**不**注入 stdin。
- stdout / stderr 追加到 `$LOOM_HOME/acp/loom-server-{port}.log`，避免污染父进程 stdout。
- Windows：`std::os::windows::process::CommandExt::creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)`，
  沿用 `experimental/curator/src/review.rs:497` 模板。
- Unix：先 `setpgid`（可选），后续父进程不再向子进程组发信号。
- meta.json **原子写**（避免 spawn 成功但写 metadata 失败导致 detached 子进程无身份）：
  1. spawn 之前先写 `loom-server-{host}-{port}.meta.json.tmp`，内容 `{phase:"spawning",
     binary, host, port, started_at}`；
  2. spawn 完成、readiness 通过后，原子 rename 为 `loom-server-{host}-{port}.meta.json`，
     补 `{pid, version, phase:"ready"}`；
  3. 任何中间步骤失败时 `.tmp` 文件保留作为"上次未完成"痕迹；下一次 `--websocket` 在 §4.1
     探测时若发现遗留 `.tmp` 不阻塞流程，但打印 `warning: previous spawn left a stale .tmp,
     pid=<recorded_pid>; not blocking`，便于人工介入。

### 4.5 错误信息

- 成功:
  - 复用: `loom acp --websocket: reusing existing loom-server pid=<pid>`；
  - 新启动: `loom acp --websocket: ready ws://{host}:{port}/acp (started by pid=<pid>)`。
- 失败: 单行人类可读 + `hint: tail $LOOM_HOME/acp/loom-server-{port}.log`。
- 所有日志写 stderr，绝不写 stdout。

---

## 5. 模块结构

新增 `apps/cli/src/server_process/` 目录式模块（参考 `agent/agent-core/src/tools/invoke_agent/`）：

| 文件        | 职责                                                                 |
|-------------|----------------------------------------------------------------------|
| `mod.rs`    | 公共 API：`pub async fn ensure_loom_server(&str) -> Result<()>`；错误枚举 `EnsureError` |
| `probe.rs`  | `probe_loose()` / `probe_ready()`，基于 `reqwest` 已有的 blocking + json 特性 |
| `lock.rs`   | `PerPortLock::acquire(host, port)`；`Drop` 释放                       |
| `spawn.rs`  | `resolve_binary()` + `spawn_server(args)`；Windows 标志位            |
| `meta.rs`   | meta.json 读写                                                       |

依赖：`apps/cli/Cargo.toml` 已含 `reqwest`（含 `blocking`、`json`）、`tokio::net`。
无需新增 crate。

---

## 6. 错误类型草案

```rust
pub enum EnsureError {
    NotLoopback { host: String },
    HttpsNotSupported { url: String },
    PortInUseByOther { host: String, port: u16 },
    AuthRequired { host: String, port: u16 },
    BinaryNotFound,
    SpawnFailed { code: Option<i32>, log: PathBuf },
    ReadinessTimeout { log: PathBuf },
    LockPoisoned { path: PathBuf },
}
```

`Display` 实现统一前缀 `loom acp --websocket: `，便于 `eprintln!` 直出。

---

## 7. 文档与运维

- 本文档沉淀在 `docs/opencode-protocol/acp-adjacent/acp-websocket-cli-ensure.md`。
- 用户面 README 段落（在 `docs/design/acp-websocket.md` 末尾追加）：

  > 如果 IDE 报告 `ws://...` 拒连，可在仓库根目录先执行：
  >
  > ```powershell
  > loom acp --websocket
  > ```
  >
  > 命令会探测端口、缺失时后台启动 `loom-server`，完成后立即退出，不接管终端。
  > 子进程作为共享 detached daemon 运行；如需关闭：`pkill -f loom-server`。

- 故障排查表：

  | 现象                       | 排查点                                                  |
  |----------------------------|--------------------------------------------------------|
  | exit 2 端口被占用          | `ss -ltnp 'sport = :3030'` / `Get-NetTCPConnection`    |
  | exit 3 鉴权失败            | 检查 `LOOM_AUTH_TOKEN` 是否与 `loom-server` 启动端一致 |
  | exit 4 启动失败            | 查看 `$LOOM_HOME/acp/loom-server-3030.log`             |
  | exit 5 找不到 binary       | `cargo build -p loom-server` 或设置 `LOOM_SERVER_BIN`   |

---

## 8. 测试

### 8.1 单元（`apps/cli/src/server_process/tests.rs`，沿用 `apps/cli/Cargo.toml` 现有 dev-dep）

- URL 解析：loopback 接受；远程 host/非零端口 → `Err(NotLoopback)`；`wss://` → `Err(HttpsNotSupported)`。
- 锁：`acquire("127.0.0.1", 31337)` 第二次调用阻塞，直到第一个 drop。
- meta.json 路径生成与序列化往返。

### 8.2 集成（`apps/cli/tests/acp_websocket_ensure.rs`）

- mock HTTP server（`httpmock` 或自建 `tokio::net::TcpListener`）：
  - 拒连 → 调用一个 stub `loom-server` 脚本（`echo pid; sleep`）作为 fake binary；
    验证 `ensure_loom_server` 调用 `Command` 的 argv、stdout 重定向、meta.json 字段。
  - 已返回 `healthy:true` + `kind=external-kernel` → 立刻退出，不 spawn。
  - 已返回 401 → `EnsureError::AuthRequired`。

### 8.3 e2e（可选，需真实 `loom-server`）

- 真实端口 `127.0.0.1:18081`；风格对齐 `apps/server/tests/acp_ws_mega_e2e.rs`。

### 8.4 跳过项

- 不验证父进程退出后子进程仍存活（设计为 detached；测试中不验证“杀子进程”）。
- 不验证 Windows `creation_flags`（CI 覆盖困难）；本地手测一次。

---

## 9. 风险与权衡

| 风险                                            | 缓解                                                                 |
|-------------------------------------------------|----------------------------------------------------------------------|
| 共享 daemon 寿命长于父进程，IDE 退出后不杀      | 文档告知 `pkill -f loom-server`；后续可加 systemd unit（独立工作）   |
| 配置 / 工作目录与 IDE 预期不一致                | 显式 `--directory <cwd>`，文档提示用户保持 IDE 工作目录             |
| 子进程崩溃无人值守                              | 下一次 `--websocket` 重新探测；运行中不复活，避免状态丢失           |
| WS 路径硬编码 `/acp`                            | 派生点单一 (`server_process::ws_base`)，与服务端路由同源             |
| `--server` 鉴权变量不一致 (`LOOM_SERVER_AUTH` vs `LOOM_AUTH_TOKEN`) | 本模块统一读 `LOOM_AUTH_TOKEN` / `OPENCODE_SERVER_PASSWORD`；暂不修旧路径 |

---

## 10. 落地步骤（按 `loom-development` “分阶段”）

1. **Phase 1 底层依赖**：`lock.rs`、`meta.rs`、`probe.rs`、`spawn.rs`；
2. **Phase 2 公共 API**：`mod.rs::ensure_loom_server` + `EnsureError`；
3. **Phase 3 CLI 接入**：`AcpArgs.websocket` 字段 + `apps/cli/src/main.rs:73` 分支；
4. **Phase 4 日志 / 错误**：统一 stderr 输出 + 退出码映射；
5. **Phase 5 验证**：单元 + 集成 +（可选）e2e；
6. 每步执行 `cargo build -p cli && cargo test -p cli --lib` 验证编译。

完成 Phase 3 后即可手动执行 `loom acp --websocket` 烟测；Phase 5 收尾。

---

## 11. 关联文件

- `apps/cli/src/args.rs:740` — `AcpArgs`
- `apps/cli/src/main.rs:73` — ACP 早期分支
- `apps/cli/Cargo.toml:34` — `reqwest` (blocking, json)
- `apps/cli/Cargo.toml:47` — `tokio-tungstenite`（供未来 WS 客户端复用）
- `apps/server/src/routes.rs:29` — `GET /acp`
- `apps/server/src/handlers/acp.rs:23` — WS handler
- `apps/server/src/handlers/health.rs:12` — `/global/health`
- `apps/server/src/handlers/instance.rs:6` — `/instance`
- `apps/server/src/auth.rs:162` — 鉴权
- `apps/acp/src/server.rs:105` — stdio server（保持不变）
- `experimental/curator/src/review.rs:497` — detached spawn 模板
- `docs/design/acp-websocket.md` — 现有 WS 入口说明（追加用户段落）
