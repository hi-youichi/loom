# Agent Dashboard 技术方案

## 1. 背景与目标

当前 CLI 启动 agent 后缺乏可见性，用户无法实时观察 agent 执行状态、日志输出，也无法中途干预。

本方案提供一个**独立 TUI 仪表盘**，监控所有活跃 agent 会话，与 task 工具解耦。

## 2. 设计目标

- 实时展示所有活跃 agent 的运行状态
- 支持选中 agent 查看流式日志输出
- 提供基础干预能力（暂停、取消）
- Dashboard 作为独立进程运行，不影响 agent 自身生命周期

## 3. 核心问题：同进程架构下的进程间通信

**当前架构**：cli_run / goal_runner / ACP 全部是同进程库调用，Dashboard 作为独立 OS 进程无法访问其内存状态。

**解决方案**：通过共享文件系统（`~/.loom/`）解耦，Agent 端写文件，Dashboard 端读文件。

```
┌─────────────────────────────┐         ┌─────────────────────────────┐
│  Agent 进程 (cli/ACP/goal)  │         │  Dashboard 进程              │
│                             │         │                             │
│  cli_run/agent.rs          │  write   │  轮询 sessions_index.json    │
│    → EventEmitter 写事件    │ ──────→ │    → ratatui TUI 渲染       │
│    → SessionIndexer 注册   │  文件    │    → notify 监听 events    │
└─────────────────────────────┘         └─────────────────────────────┘
              ↓                                    ↑
              └── ~/.loom/thread/<id>/events.jsonl ─┘
              └── ~/.loom/sessions_index.json
```

**关键代码路径**：
- `cli_run/agent.rs:338` — `run_agent()` 入口，初始化 session 目录并注册到索引
- `runner_common.rs:83-136` — ReAct 主循环，`while let Some(event) = stream.next().await` 消费事件
- `act_node.rs:537` — `ToolStart` 事件发射点（工具开始执行）
- `think_node.rs:113` — `ToolCall` 事件发射点（LLM 决定调用工具）

## 4. 技术方案

### 4.1 整体架构

```
┌──────────────────────────────────────────────────────────┐
│  loom agent dashboard (独立进程, ratatui TUI)            │
├──────────────────┬─────────────────────────────────────┤
│ agent list (左栏) │ selected agent log stream (右栏)     │
│ 按状态着色        │ events.jsonl tail                   │
└──────┬───────────┴─────────────────────────────────────┘
       │ poll (500ms)          │ notify (100ms)
       ▼                       ▼
┌──────────────────────────────────────────────────────────┐
│  ~/.loom/                                               │
│    sessions_index.json  ← O(1) 发现活跃 session          │
│    thread/<session_id>/                                │
│      session.json     ← 元数据 + heartbeat              │
│      events.jsonl     ← 事件流（每行一个 JSON）          │
└──────────────────────────────────────────────────────────┘
```

### 4.2 事件契约：复用现有 AnyStreamEvent

不新建 `AgentEvent` 枚举，复用现有 `AnyStreamEvent`（`cli_run/agent.rs:274-329`）：

```rust
// AnyStreamEvent 已有的转换能力
any_stream_event.to_format_a()  // → serde_json::Value
// 写入 events.jsonl，每行一个 JSON 对象
```

**Dashboard 订阅的事件子集**（对应 `StreamEvent<S>` 变体）：

| Dashboard 视图 | 对应 StreamEvent 变体 | 发射位置 |
|----------------|----------------------|---------|
| Thinking 内容 | `Messages` (kind=Thinking) | `think_node.rs:95` |
| 工具调用 | `ToolCall` | `think_node.rs:113` |
| 工具开始 | `ToolStart` | `act_node.rs:537` |
| 工具结束 | `ToolEnd` | `act_node.rs:607` |
| LLM 输出 | `Messages` (kind=Message) | `think_node.rs:95` |
| 工具输出 | `ToolOutput` | `act_node.rs:507` |
| Session 完成 | `Values` (含 final_state) | `runner_common.rs` |

每行格式：
```jsonl
{"ts":"2025-08-19T10:00:00Z","kind":"ToolStart","name":"Bash","duration_ms":null,"content":"...","raw":{}}
{"ts":"2025-08-19T10:00:01Z","kind":"ToolEnd","name":"Bash","duration_ms":1234,"content":"...","raw":{}}
```

### 4.3 会话索引（O(1) 发现）

```json
// ~/.loom/sessions_index.json
{
  "version": 1,
  "active": {
    "abc123": {
      "path": "thread/abc123",
      "agent": "dev",
      "profile": "dev",
      "started_at": "2025-08-19T10:00:00Z",
      "last_heartbeat": "2025-08-19T10:00:05Z"
    }
  },
  "archived": {
    "def456": {
      "path": "thread/def456",
      "agent": "dev",
      "started_at": "2025-08-19T09:00:00Z",
      "exit_code": 0,
      "ended_at": "2025-08-19T09:30:00Z"
    }
  }
}
```

**注册时机**：`cli_run/agent.rs:run_agent()` L344 构建完 config 后、L391 调用 `stream_with_config()` 前。

**心跳机制**：`session.json` 中 `last_heartbeat` 由 agent 每 N 秒更新（如 5s），Dashboard 超过 30s 无心跳则标记为 stale。

### 4.4 会话存储

```json
// ~/.loom/thread/<session_id>/session.json
{
  "session_id": "abc123",
  "agent": "dev",
  "profile": "dev",
  "status": "running",
  "started_at": "2025-08-19T10:00:00Z",
  "last_heartbeat": "2025-08-19T10:00:05Z",
  "last_event_kind": "ToolEnd",
  "current_step": 12
}
```

**status**: `pending` | `running` | `paused` | `done` | `failed` | `stale`

**退出时更新**：`runner_common.rs` 的 `stream.next()` 返回 `None`（流结束）时，agent 端更新 `session.json` status 并将 session 从 `sessions_index.json` 的 `active` 移入 `archived`。

### 4.5 存活检测

**本地进程**（cli_run / ACP）：通过 `last_heartbeat` 超时判断，不依赖 pid（因为都是同一进程内，pid 相同）。

**分布式场景**（goal_runner 派生的独立子进程）：
```rust
fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    #[cfg(windows)]
    {
        use std::os::windows::process::OpenProcess;
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        !handle.is_null()
    }
}
```

**Dashboard 检测 stale 流程**：
1. 读取 `sessions_index.json` 的 `active` map（500ms 轮询）
2. 对每个 session，比对 `last_heartbeat` 与当前时间
3. 超过 30s 无更新 → 标记 `status: "stale"`，不删除（保留可见性）
4. 收到新事件时自动恢复为 `running`

### 4.6 TUI 框架选型

| 框架 | 特点 | 适合场景 |
|------|------|---------|
| **ratatui** | 成熟、轻量、无 C 依赖、sub-ms 渲染、活跃维护 | 通用首选（基准方案） |
| **reratui** | React hooks/components 封装 ratatui，状态管理更清晰 | 复杂 UI + 多 agent 状态 |
| **textual-rs** | CSS 样式、响应式信号、30fps diff 渲染 | 现代 UI 风格 |
| **weavetui** | ratatui + tokio 封装、action-based | async 工作流 |
| **rmux** | 终端多路复用器 + PTY pane widget | 嵌入 live PTY（进阶） |

**Phase 2/3 轻量版**：`ratatui`
**Phase 3 完整版**：`reratui` 或 `weavetui`
**进阶**：`rmux pane` widget

### 4.7 TUI 布局（ratatui 基准）

```
┌─ loom agents ─────────────────────────────────────────┐
│  abc123  dev        ● running   00:03:21   thinking... │
│  def456  ask        ● running   00:01:05   tool:bash  │
│  ghi789  dev        ○ stale    00:45:00   --         │
├─────────────────────────────┬────────────────────────┤
│ ▼ [abc123] dev ● running    │ ▼ ToolCall: Bash       │
│                             │   cmd: "ls -la"        │
│   Thinking: 分析登录失败...  │ ▼ ToolStart: Bash      │
│   > ToolCall: auth/check    │   Running...           │
│   > ToolCall: db/query_user │ ▼ ToolEnd: Bash        │
│   last: "Token已过期..."    │   234ms ✓              │
│                             │   Output: total 12...   │
├─────────────────────────────┴────────────────────────┤
│ ↑↓ navigate  p pause  c cancel  r restart  q quit    │
└──────────────────────────────────────────────────────┘
```

**交互**：
- `↑↓` 切换选中 agent
- `→` 展开选中 agent 详情
- `p` pause（向 agent 发 `CancellationToken`）
- `c` cancel（强制终止）
- `r` restart（重新运行同 session）
- `q` quit dashboard（不影响 agent）
- `t` 创建新 session

### 4.8 Agent 侧改造

#### 4.8.1 核心改动：runner_common.rs 事件注入

文件：`loom/src/runner_common.rs:83-136`

```rust
// 在 stream.next().await 循环中，on_event() 调用旁路写入文件
while let Some(event) = stream.next().await {
    on_event(event.clone());  // 现有逻辑，不变

    // 新增：写入 events.jsonl
    if let Some(emitter) = &self.event_emitter {
        emitter.emit(&event);
    }

    if let StreamEvent::Values(s) = &event {
        final_state = Some(s.clone());
    }
}
```

#### 4.8.2 新增 EventEmitter trait 和实现

```rust
// loom/src/agent/dashboard/emitter.rs

pub trait EventEmitter: Send + Sync {
    fn emit(&self, event: &AnyStreamEvent);
    fn flush(&self);
    fn session_id(&self) -> &str;
}

pub struct FileEventEmitter {
    session_id: String,
    events_file: PathBuf,
    session_file: PathBuf,
    index_path: PathBuf,
    last_heartbeat: Instant,
}

impl FileEventEmitter {
    pub fn new(session_id: &str) -> Self {
        let dir = thread_session_dir(session_id);
        std::fs::create_dir_all(&dir).ok();
        Self {
            session_id: session_id.to_string(),
            events_file: dir.join("events.jsonl"),
            session_file: dir.join("session.json"),
            index_path: loom_home().join("sessions_index.json"),
            last_heartbeat: Instant::now(),
        }
    }
}

impl EventEmitter for FileEventEmitter {
    fn emit(&self, event: &AnyStreamEvent) {
        // 1. 写入 events.jsonl（追加，每行一个 JSON）
        let json = event.to_format_a();
        let line = serde_json::to_string(&json).unwrap();
        // O_APPEND 写入，不依赖缓冲区
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events_file)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", line)
            }).ok();

        // 2. 更新 session.json heartbeat（每 5s 最多一次，避免过度 IO）
        if self.last_heartbeat.elapsed() > Duration::from_secs(5) {
            self.update_heartbeat();
        }
    }

    fn flush(&self) {
        // 确保所有写操作落盘（对 events.jsonl 影响小，events 本身是追加）
        // 主要确保 session.json 更新
    }
}
```

#### 4.8.3 cli_run/agent.rs 初始化

`run_agent()` 函数（L338-487）改动：

```rust
// L340 处新增
let event_emitter = Arc::new(FileEventEmitter::new(opts.session_id.as_deref().unwrap_or("default")));

// L391 前，将 emitter 注入 runner
// 需要在 build_runner() 或 stream_with_config() 中传递
// 方案：在 RunOptions 中新增可选字段 event_emitter: Option<Arc<dyn EventEmitter>>
// 然后在 runner_common 中读取

// L395 stream_with_config 调用处
runner.stream_with_config(msg, config, on_event, {
    let emitter = event_emitter.clone();
    Some(Arc::new(move |ev: AnyStreamEvent| {
        emitter.emit(&ev);
    }))
})
```

#### 4.8.4 索引注册与注销

在 `run_agent()` 中：

```rust
// L344 build_helve_config 后
SessionIndexer::register_running(
    session_id,
    agent_name,
    profile_name,
    thread_session_dir(session_id),
).await;

// L484 函数退出前（任何分支）
SessionIndexer::archive(
    session_id,
    exit_code,
).await;
```

```rust
// loom/src/agent/dashboard/indexer.rs

pub struct SessionIndexer;

impl SessionIndexer {
    /// 读取 sessions_index.json，插入/更新 active 项
    pub async fn register_running(session_id: &str, agent: &str, profile: &str, path: PathBuf) {
        let index_path = loom_home().join("sessions_index.json");
        let mut index = Self::read_index(&index_path).await;
        index.active.insert(session_id.to_string(), ActiveSession {
            path: path.to_string_lossy().to_string(),
            agent: agent.to_string(),
            profile: profile.to_string(),
            started_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
        });
        Self::write_index(&index_path, &index).await;
    }

    /// 从 active 移到 archived
    pub async fn archive(session_id: &str, exit_code: i32) {
        let index_path = loom_home().join("sessions_index.json");
        let mut index = Self::read_index(&index_path).await;
        if let Some(entry) = index.active.remove(session_id) {
            index.archived.insert(session_id.to_string(), ArchivedSession {
                path: entry.path,
                agent: entry.agent,
                started_at: entry.started_at,
                ended_at: chrono::Utc::now(),
                exit_code,
            });
        }
        Self::write_index(&index_path, &index).await;
    }
}
```

### 4.9 命令行接口

```bash
# 启动 agent 仪表盘（独立进程）
loom agent dashboard

# 查看单个 agent 实时日志（轻量替代）
loom agent watch <session_id>

# agent 列表（静态，快速查看）
loom agent list

# 查看已归档的 agent 历史
loom agent history [--limit 20]
```

## 5. 实施计划

### Phase 1：事件基础设施

**目标**：不改变现有 agent 行为，只是将事件写入文件

- [ ] 新建 `loom/src/agent/dashboard/` 模块（emitter.rs, indexer.rs, types.rs）
- [ ] 定义 `EventEmitter` trait 和 `FileEventEmitter` 实现
- [ ] `AnyStreamEvent.to_format_a()` 的 JSON 格式验证（已有，复用）
- [ ] `SessionIndexer::register_running()` / `archive()` 实现
- [ ] `cli_run/agent.rs`：在 `run_agent()` 入口处初始化 `FileEventEmitter` 并注册到索引
- [ ] `cli_run/mod.rs`：在 `RunOptions` 新增 `event_emitter: Option<Arc<dyn EventEmitter>>`
- [ ] `runner_common.rs`：在 `stream.next()` 循环中将事件传给 emitter（通过 `any_stream_event_sender` 已有通道）
- [ ] 验证：启动 `loom run` 后 `~/.loom/thread/<id>/events.jsonl` 有内容写入

### Phase 2：轻量观测

**目标**：提供 `agent watch` 命令，不依赖 TUI

- [ ] 实现 `loom agent watch <session_id>` 命令
- [ ] 使用 `tokio::fs::Notify` 或 `notify` crate 监听 `events.jsonl` 尾部变化
- [ ] 类似 `tail -f`，实时打印事件行
- [ ] 实现 `loom agent list` 命令，读取 `sessions_index.json` 展示活跃 session

### Phase 3：TUI Dashboard

**目标**：ratatui 完整仪表盘

- [ ] 新建 `cli/src/agent/dashboard/` 子模块（独立二进制？不，放在 `cli/src/agent/dashboard/`）
- [ ] ratatui 布局：左右分栏，左侧 agent 列表，右侧选中 session 日志
- [ ] `sessions_index.json` 500ms 轮询更新列表
- [ ] `events.jsonl` notify 监听（100ms）更新选中 session 日志区
- [ ] 心跳 stale 检测（30s 超时标记）
- [ ] 键盘交互：↑↓导航，p/c/r 操作
- [ ] 非 TTY 环境自动降级为 `agent watch` 模式

### Phase 4（可选）：进阶功能

- [ ] `reratui` 升级：状态管理 hooks 化
- [ ] `rmux` 集成：agent PTY 直接嵌入 dashboard pane
- [ ] 分布式存活检测：跨机器 agent 监控

## 6. 依赖变更

```toml
# loom/Cargo.toml 新增依赖

[dependencies]
# ratatui TUI
ratatui = "0.30"
crossterm = "0.29"

# 文件监控
notify = "6"        # 跨平台文件系统监听（替代 kqueue/inotify）
notify-debouncer-mini = "0.4"  # 防抖版本，避免频繁触发

# 已存在，无需新增
# tokio = { version = "1", features = ["fs", "sync", "rt"] }
# serde = { version = "1", features = ["derive"] }
# serde_json = "1"
# chrono = { version = "0.4", features = ["serde"] }
```

**注意**：`notify` 需要在 Windows 上单独测试，`OpenProcess` 存活检测跨平台实现。

## 7. 文件改动清单

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `loom/src/cli_run/mod.rs` | 修改 | `RunOptions` 新增 `event_emitter` 字段 |
| `loom/src/cli_run/agent.rs` | 修改 | `run_agent()` 初始化 emitter 并注册/注销索引 |
| `loom/src/runner_common.rs` | 修改 | 事件循环中将事件传给 emitter |
| `loom/src/agent/dashboard/emitter.rs` | **新建** | `EventEmitter` trait + `FileEventEmitter` |
| `loom/src/agent/dashboard/indexer.rs` | **新建** | `SessionIndexer` 读写 `sessions_index.json` |
| `loom/src/agent/dashboard/types.rs` | **新建** | 共享类型定义（`ActiveSession`, `ArchivedSession`, `DashboardEvent`） |
| `cli/src/cmd/agent.rs` | **新建** | `agent dashboard` / `agent watch` / `agent list` 子命令 |
| `cli/src/agent/dashboard/tui.rs` | **新建** | ratatui TUI 实现 |

## 8. 风险与备选

- **风险**：ratatui 与现有 CLI 输出格式冲突（双重 TTY）
- **备选**：检测非 TTY 环境时自动降级为 `agent watch` 模式
- **风险**：`events.jsonl` 无限增长
- **备选**：按大小轮转，单文件上限 10MB，超出 rename 为 `.1`
- **风险**：`ratatui` 状态管理全靠手动同步（reratui 可缓解）
- **备选**：Phase 3 升级到 reratui，用 hooks 管理复杂状态
- **风险**：`sessions_index.json` 并发写入冲突（多个 cli_run 同时注册）
- **备选**：使用 `RwLock` 文件锁或 `tokio::sync::Mutex` 保护写操作
- **风险**：`notify` 在 Windows 网络驱动器上不稳定
- **备选**：降级为 100ms 定时轮询 `events.jsonl` 文件 mtime
- **风险**：GoalRunner 每个 iteration 调用一次 `run_agent()`（短生命周期），频繁创建/销毁 session 文件
- **备选**：GoalRunner 使用同一个 session_id 直到任务完成，只在 `tool.execute()` 内部复用已有 session 目录

## 9. 验收标准

- [ ] `loom agent dashboard` 正常启动并展示 agent 列表
- [ ] 新启动的 agent 事件在 500ms 内出现在 dashboard
- [ ] `q` 键退出 dashboard 不影响后台 agent
- [ ] `loom agent watch <session_id>` 正确打印实时日志
- [ ] dashboard 退出后重新进入，能恢复所有历史 session（done 状态）
- [ ] `loom agent list` 正确显示活跃 session 及状态
- [ ] `sessions_index.json` 在并发写入时不损坏（多 cli_run 同时运行）
- [ ] Windows 环境下存活检测正常（`OpenProcess`）
