# Loom Server `/acp` 持久 Agent 接入

> 状态：草案 v2（2025-08）
> 范围：把 `LoomAcpAgent` 从 `loom acp` 子进程搬到 `loom-server` 进程内（`AcpHub`），使 `loom acp` 降级为纯 stdio↔WS 转发桥；Zed 断线重连后，正在运行的 prompt 不中断，重连即可看到结果。

---

## 1. 动机与现状

### 1.1 当前链路

```
Zed ←stdio→ loom acp 子进程 (agent 在这里)
                ↑ server 收到 WS 后 spawn 此子进程并桥接
          loom-server (WS endpoint + stdio 桥)
```

- `loom-server` 的 `GET /acp`（`apps/server/src/routes.rs:29`）handler 在 `apps/server/src/handlers/acp.rs:89-188` `handle_socket` 中 **spawn `loom acp` 子进程**，把 WS text frame ↔ 子进程 stdio 双向透传。
- `LoomAcpAgent` 在子进程内构造（`apps/acp/src/stdio_loop.rs:154-162`），处理所有 ACP 请求。
- 已有进程内 `AcpHub`（`apps/server/src/acp_hub.rs:13`）实现了「单例 `Arc<LoomAcpAgent>` + 512 条 replay buffer + lease 取消」（`attach`，`acp_hub.rs:36-94`），但 **handler 不使用它**（`handlers/acp.rs:36-39` 注释明确说明）。

### 1.2 断线时丢失什么

| 丢失项 | 原因 | 能否恢复 |
|---|---|---|
| **正在运行的 prompt** | `kill_on_drop(true)`（`handlers/acp.rs:95`）杀子进程 | 不能，需重新发 |
| MCP server 配置（内存） | `SessionStore` 随子进程销毁 | `session/new`/`session/load` 时客户端重传 |
| client capabilities（内存） | 同上 | `initialize` 时客户端重报 |
| ~~session 历史~~ | ~~SQLite 持久化~~ | **不丢**（`agent.rs:1107`、`agent.rs:1435` 直接查 SQLite） |

### 1.3 目标链路

```
Zed ←stdio→ loom acp 子进程 (纯转发桥, 不跑 agent)
                ↕ WS
          loom-server (AcpHub + LoomAcpAgent, 持久)
```

- `loom acp` 降级为 thin bridge：stdin → WS、WS → stdout，不含 `LoomAcpAgent`。
- Agent 在 server 进程内由 `AcpHub` 持有；WS 断开只断桥，agent 继续跑。
- `loom acp` 重连后，`AcpHub::attach` 返回同一个 agent + replay buffer。

---

## 2. 目标 / 非目标

### 目标

1. **`loom acp` 新增 bridge 模式**：`loom acp --bridge --server ws://127.0.0.1:18081/acp`，纯 stdio↔WS 转发，不构造 `LoomAcpAgent`。
2. **server handler 接 AcpHub**：`handle_socket` 不再 spawn 子进程；改为 `state.acp_hub.attach()` + `spawn_drain_task` + `run_transport_with_agent`。
3. **transport 抽泛型**：在 `loom-acp` 中抽出 `run_transport_with_agent(agent, conn_shared, transport, eof_signal)`，stdio 和 WS 两种 transport 复用同一套 handler 注册逻辑。
4. **断线 persist 语义**：WS 断开 → agent 继续跑 → replay buffer 持续累积 → 重连时灌入。
5. **通知投递完整**：`notif_rx` 由调用方 spawn `spawn_drain_task` 消费，`conn_shared` 传入 `run_transport_with_agent` 供 `initialize` handler + `tools::set_connection` 使用。

### 非目标

- **不**升级到 ClientKey 池（`HashMap<ClientKey, HubInner>`）。单例即可满足「同一 Zed 重连不丢」。多用户隔离留到 P2（`acp-websocket-todo.md:46-50`）。
- **不**实现 per-session actor 串行化（`acp-websocket-todo.md:21`）。
- **不**改 `LoomAcpAgent` 内部结构（`apps/acp/src/agent.rs`）。
- **不**改 `loom acp` 原有 stdio 模式（不带 `--bridge` 时行为不变，agent 仍在子进程内跑）。

---

## 3. 架构与数据流

### 3.1 目标态

```
Zed (IDE, acp.command = "loom acp --bridge --server ws://127.0.0.1:18081/acp")
   ↕ stdio (ACP JSON-RPC, 每行一条)
loom acp 子进程 (run_bridge: stdin→WS, WS→stdout, 纯字节透传)
   ↕ ws://127.0.0.1:18081/acp (每条 ACP JSON-RPC 一个 text frame)
loom-server
   ├─ handlers::acp::connect → ws.on_upgrade(move |socket| handle_socket(state, socket))
   ├─ handlers::acp::handle_socket(state, socket)
   │    ├─ build_ws_transport(socket)   → Lines<S, R> + eof_signal
   │    ├─ state.acp_hub.attach()       → Arc<LoomAcpAgent>, notif_rx, lease
   │    ├─ conn_shared = Arc::new(RwLock::new(None))
   │    ├─ spawn_drain_task(notif_rx, conn_shared.clone())  ← 投递 session/update
   │    └─ run_transport_with_agent(agent, conn_shared, transport, eof_signal)
   ├─ run_transport_with_agent(agent, conn_shared, transport, eof_signal)   [apps/acp]
   │    ├─ Agent::builder().on_receive_request(...10 个).connect_with(transport, ...)
   │    └─ initialize handler 内调用 tools::set_connection(conn_shared)
   └─ AcpHub (单例, apps/server/src/acp_hub.rs:13)
        └─ HubInner { agent, recipient, replay: VecDeque<512>, lease_cancel }

关键：notif_rx（session/update 通道）必须由 handle_socket 的 drain task 消费，
否则通知无法到达 WS 客户端。conn_shared 传入 run_transport_with_agent 是因为
initialize handler 需要写入它并注册到 tools::set_connection。
```

### 3.2 断线与重连时序

```
─── 正常运行 ───
Zed ←stdio→ loom acp ←WS→ server(AcpHub/agent)
                              ├─ prompt 执行中
                              ├─ session/update → notif_rx → drain task → conn_shared → WS
                              └─ session/update → replay buffer

─── Zed 断开 ───
loom acp 子进程退出 (Zed 关闭 stdio)
WS 断开
server: handle_socket 的 eof_signal 触发（oneshot，由 WS Close 帧触发）
server: agent 不停止, prompt 继续跑
server: session/update 继续写入 replay buffer（drain task 随 WS 断开终止，但 replay 在 AcpHub 内部）

─── Zed 重连 ───
Zed 启动新 loom acp --bridge → 新 WS 连接
server: handle_socket → acp_hub.attach()
  ├─ 返回同一个 Arc<LoomAcpAgent>
  ├─ 重绑 notification 投递目标
  ├─ 重新构造 conn_shared + spawn drain task
  └─ 灌入 replay buffer (acp_hub.rs:82-92)
Zed: 看到断线期间所有 session/update + 实时后续
```

### 3.3 与现状对比

| 维度 | 现状（stdio 桥） | 目标（AcpHub 持久） |
|---|---|---|
| `LoomAcpAgent` 所在 | `loom acp` 子进程内 | `loom-server` 进程内 |
| `loom acp` 角色 | 跑 agent | 纯 stdio↔WS 转发 |
| WS 断开 → 运行中 prompt | 中断（子进程被 kill） | 继续（agent 在 server 内） |
| session 历史 | SQLite（不丢） | 同左 |
| MCP config / capabilities | 丢失 | AcpHub 保留 |
| `session/update` 重放 | 无 | 512 条 buffer |
| 进程数 | 1 server + N 子进程（每子进程 1 agent） | 1 server（1 agent）+ N 桥子进程（无 agent） |

---

## 4. 改动方案

### 4.1 `apps/acp` 加 `transport_loop.rs`（新文件）

把 `apps/acp/src/stdio_loop.rs:260-441` `register_handlers_and_connect` 的 handler 注册逻辑抽出为泛型函数：

- **`build_agent_and_channel`**（已存在 `stdio_loop.rs:154-162`）：构造 `Arc<LoomAcpAgent>` + notification receiver。**保留**。
- **`spawn_drain_task`**（已存在 `stdio_loop.rs:166-183`）：drain notification rx 到 `ConnectionTo<Client>`。**保留**，改为 `pub`。
- **`run_transport_with_agent`**（**新增**）：

```rust
pub async fn run_transport_with_agent(
    agent: Arc<LoomAcpAgent>,
    conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>>,
    transport: Lines<
        impl futures::Sink<String, Error = std::io::Error> + Unpin + Send + 'static,
        impl futures::Stream<Item = std::io::Result<String>> + Unpin + Send + 'static,
    >,
    eof_signal: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), agent_client_protocol::Error>
```

`conn_shared` 是必需的：`initialize` handler 内将其写入连接对象并调用 `tools::set_connection(conn_shared.clone())`（`stdio_loop.rs:296`），供 `request_permission` 等工具通过 `AcpClientBridge` 向客户端发起请求。

内部 `LocalSet::new().run_until(async { ... })` 包裹整个 `connect_with(transport, ...)`（`Agent` 是 `!Send`，必须 LocalSet）。handler 注册（10 个 `on_receive_request` + 1 个 `on_receive_notification`）照搬 `stdio_loop.rs:280-421`。

> **注意**：调用方负责构造 `conn_shared` 并在 `run_transport_with_agent` 之前 spawn `spawn_drain_task(notif_rx, conn_shared.clone())`，将 `session/update` 通知投递到连接。`run_transport_with_agent` 不负责 drain——它的职责仅限于驱动 ACP request/response 循环。

### 4.2 `apps/acp/src/stdio_loop.rs` 改为消费 transport_loop

```rust
pub async fn run_stdio_loop() -> Result<StdioLoopResult, Box<dyn std::error::Error + Send + Sync>> {
    logging::init_logging(None);
    let local = tokio::task::LocalSet::new();
    local.run_until(async {
        let (agent, rx) = build_agent_and_channel()?;
        let conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>> =
            Arc::new(tokio::sync::RwLock::new(None));
        let drain = spawn_drain_task(rx, conn_shared.clone());

        let (transport, eof_signal) = build_stdio_transport();
        let result = run_transport_with_agent(agent, conn_shared, transport, eof_signal).await;

        agent.cancel_all();
        let _ = tokio::time::timeout(Duration::from_millis(200), drain).await;
        result.map_err(|e| Box::new(e) as _)
    }).await
}
```

`stdio_loop.rs:260-441` 的私有 `register_handlers_and_connect` 删除。`build_stdio_transport`（`:198-252`）保留。

### 4.3 `apps/acp/src/lib.rs` 导出

```rust
pub use transport_loop::{run_transport_with_agent};
pub use stdio_loop::spawn_drain_task; // server 侧 handle_socket 需要
```

### 4.4 `apps/acp` 加 `ws_bridge.rs`（新文件）— `loom acp --bridge`

`loom acp` 子进程的 **thin bridge 模式**：不构造 `LoomAcpAgent`，纯转发 stdio ↔ WS。

**依赖**：需在 `apps/acp/Cargo.toml` 的 `[dependencies]` 新增 `tokio-tungstenite = "0.24"`（当前仅在 `apps/server/Cargo.toml` 的 `[dev-dependencies]` 中）。bridge 模式不依赖 `LoomAcpAgent`，可使用 feature gate 避免影响 stdio-only 构建。

```rust
pub async fn run_bridge(ws_url: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (ws, _) = connect_async(ws_url).await?;
    let (mut ws_sink, mut ws_stream) = ws.split();
    let (stdin_rx, stdout_tx) = {
        let (tx, rx) = mpsc::channel::<String>(64);
        // tokio::io::stdin → lines → tx
        // rx → ws_sink.send(Message::Text(...))
        (rx, tx)
    };

    // stdin → WS
    let send = async {
        use tokio::io::AsyncBufReadExt;
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if ws_sink.send(Message::Text(line.into())).await.is_err() { break; }
        }
    };

    // WS → stdout
    let recv = async {
        use futures::StreamExt;
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(t)) => { println!("{}", t); }
                Ok(Message::Close(_)) => break,
                _ => {}
            }
        }
    };

    tokio::select! {
        _ = send => {}
        _ = recv => {}
    }
    Ok(())
}
```

**注意**：bridge 模式不依赖 `LoomAcpAgent`、不依赖 `Agent::builder()`，因此 **不** 受 `!Send` / `LocalSet` 约束。它只是一个 WS 客户端 + stdio 读写。

### 4.5 CLI `loom acp` 子命令加参数

`apps/cli/src/main.rs:74`（或 `apps/acp/src/server.rs` 的命令解析处）增加：

```
loom acp                                    # 原有 stdio 模式（agent 在本进程内）
loom acp --bridge --server ws://host:port/acp   # 新增 bridge 模式（纯转发）
```

### 4.6 `apps/server/src/handlers/acp.rs` 重写

> **Axum WS 升级约束**：`on_upgrade` 的回调只接收 `WebSocket`，不经过 middleware/State extractor。因此 `State<SharedState>` 必须在 `connect()` 中 clone，通过 `move` 闭包传入 `handle_socket`。

```rust
/// 路由 handler（升级前）
pub async fn connect(
    State(state): State<SharedState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(MAX_ACP_WS_MESSAGE_BYTES)
        .max_frame_size(MAX_ACP_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_socket(state, socket))
}

/// 升级后 handler
async fn handle_socket(
    state: SharedState,
    socket: WebSocket,
) {
    let (agent, notif_rx, _lease) = match state.acp_hub.attach().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "AcpHub::attach failed");
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    // 构造 conn_shared——initialize handler 会写入它，并调用 tools::set_connection
    let conn_shared: Arc<tokio::sync::RwLock<Option<ConnectionTo<Client>>>> =
        Arc::new(tokio::sync::RwLock::new(None));

    // spawn drain task：notif_rx → conn_shared 中的连接（投递 session/update）
    let drain = loom_acp::spawn_drain_task(notif_rx, conn_shared.clone());

    let (transport, eof_signal) = build_ws_transport(socket);
    if let Err(e) = loom_acp::run_transport_with_agent(
        agent.clone(),
        conn_shared,
        transport,
        eof_signal,
    ).await {
        tracing::warn!(error = %e, "ACP transport loop ended with error");
    }

    agent.cancel_all();
    let _ = tokio::time::timeout(Duration::from_millis(200), drain).await;
}
```

`build_ws_transport` 把 `WebSocket` 适配为 `Lines<S, R>`：

- **outgoing**（`Sink<String>`）：`String` → `Message::Text(line)` → `ws.send()`
- **incoming**（`Stream<Item=io::Result<String>>`）：`Message::Text(t)` → `Ok(t.to_string())`；`Message::Close` → stream end；ping/pong/binary → 过滤
- **eof_signal**：`oneshot` channel，在 incoming stream 适配器内遇到 `Message::Close` 时 `tx.send(())` 触发

`build_ws_transport` 需要处理类型适配：
- `SplitSink<WebSocket, Message>` → `Sink<String, Error = io::Error>`：包装 `unfold`，将 `String` 转为 `Message::Text`，并将 tungstenite 错误映射为 `io::Error`
- `SplitStream<WebSocket>` → `Stream<Item = io::Result<String>>`：过滤非 Text 帧，将 `tungstenite::Error` 映射为 `io::Error`

删除：`loom_binary()`（`:72-87`）、`Command::new().spawn()` 块（`:89-188`）、`kill_on_drop` 相关 imports。保留：`origin_allowed`（`:48-64`）、`MAX_ACP_WS_MESSAGE_BYTES`（`:24`）、`disconnect_cancels()`（`:190-195`，留作后续 `disconnect_policy=cancel` 实现）。

> **回退路径**：可选在 `connect()` 中检查 `LOOM_ACP_SUBPROCESS=1`，若设置则走旧的子进程桥接路径。这为调试提供安全网，建议 Phase 3 后保留至少一个迭代周期。

**注意**：
- `Lines` 类型来自 `agent-client-protocol`，需确认可见性；不可见则在 `apps/acp/src/lib.rs` 封装。
- `WebSocketSink: !Clone` 时 `build_ws_transport` 全部跑在 LocalSet 单线程（与 `run_transport_with_agent` 的 LocalSet 一致）。

---

## 5. 关键代码草图

**handler 调用链**（server 侧）：

```rust
// connect() 路由 handler：升级前检查，clone state
pub async fn connect(
    State(state): State<SharedState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&headers) { return StatusCode::FORBIDDEN.into_response(); }
    ws.max_message_size(MAX_ACP_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_socket(state, socket))
}

// handle_socket：升级后，拥有 WebSocket + State
async fn handle_socket(state: SharedState, socket: WebSocket) {
    let (agent, notif_rx, _) = state.acp_hub.attach().await.unwrap();
    let conn_shared = Arc::new(tokio::sync::RwLock::new(None));
    let drain = loom_acp::spawn_drain_task(notif_rx, conn_shared.clone());
    let (transport, eof) = build_ws_transport(socket);
    let _ = loom_acp::run_transport_with_agent(agent.clone(), conn_shared, transport, eof).await;
    agent.cancel_all();
    let _ = tokio::time::timeout(Duration::from_millis(200), drain).await;
}
```

**bridge 调用链**（`loom acp` 子进程侧）：

```rust
// CLI 解析 --bridge --server <url> 后
loom_acp::ws_bridge::run_bridge(&server_url).await
```

---

## 6. 风险与权衡

| 风险 | 缓解 |
|---|---|
| `Lines<S, R>` 不可从 `loom-acp` 外部构造 | 在 `apps/acp/src/lib.rs` 重新导出或封装 `pub struct` |
| `WebSocketSink: !Clone` | `build_ws_transport` 全部跑在 LocalSet 单线程，不拆 task |
| `Agent: !Send`，跨 `tokio::spawn` 边界报错 | `run_transport_with_agent` 整个函数体在 `LocalSet` 内；handler `await` 不 spawn |
| **`tools::set_connection` 使用进程级全局状态**（`tools/client_bridge.rs:110`） | stdio 模式下安全（每进程一个 agent）；server 模式下并发连接会覆盖。短期缓解：`AcpHub` 单例 + lease 机制保证同一时刻只有一个活跃连接；长期：将全局 bridge store 改为 per-agent 或 per-session |
| **WS 写入背压** | `ws.send()` 背压会阻塞整个 `LocalSet`，进而阻塞 ACP 请求处理。stdio 路径使用独立 OS 线程避免此问题。短期可接受（单客户端场景），长期可在 drain task 中加 bounded channel 解耦 |
| **WS Close EOF 信号构造** | Axum `WebSocket` 不提供独立的 Close-received future；使用 `oneshot` channel，在 incoming stream 适配器内遇到 `Message::Close` 时触发 |
| **`Lines` 泛型约束对 WS 的适配** | `SplitSink` 发送 `Message` 而非 `String`，`SplitStream` 产出 `Result<Message, tungstenite::Error>` 而非 `io::Result<String>`；需包装类型 + 错误映射 |
| **`tokio-tungstenite` 不在 `apps/acp/Cargo.toml`** | 当前仅在 `apps/server/Cargo.toml` dev-dependencies 中。需在 `apps/acp` 的 `[dependencies]` 新增 |
| 单例下多客户端共享 agent，并发 prompt 无串行化 | 记入 §9 后续工作；ACP SDK 默认拒绝同一 session 重复 prompt |
| bridge 模式下 `loom acp` 需要 WS 客户端依赖 | 同上：`tokio-tungstenite` 需加入 `apps/acp` 依赖 |
| Zed 配置变更 | Zed 的 `acp.command` 从 `"loom acp"` 改为 `"loom acp --bridge --server ws://..."` |
| server 地址发现 | 默认 `ws://127.0.0.1:18081/acp`；CLI `--server` 覆盖；`OPENCODE_HOST` 环境变量 |
| WS 断开时 bridge 子进程退出 | 正确行为：Zed 重启时重新 spawn `loom acp --bridge`，自动重连 |
| `kill_on_drop` 路径消失 | `loom acp` 原有 stdio 模式（不带 `--bridge`）不受影响 |
| **无回退到子进程模式** | 可选 `LOOM_ACP_SUBPROCESS=1` 环境变量保留旧路径，作为调试安全网 |

---

## 7. 测试

### 7.1 单元（先做）

- `apps/acp/src/transport_loop.rs`：mock `Sink<String>` + `Stream<Item=io::Result<String>>`，跑 `initialize` + `session/new` + `session/prompt`，验证响应在 sink 端按序收到。
- `apps/acp/src/ws_bridge.rs`：mock WS server（`tokio-tungstenite` listener），验证 stdin→WS、WS→stdout 双向透传。
- `apps/server/src/handlers/acp.rs::build_ws_transport`：模拟 WS 客户端，验证 text frame 编解码 + Close 触发 eof。

### 7.2 集成（`apps/server/tests/acp_ws_mega_e2e.rs`）

- **基本 case**：WS 连接 → `initialize` + `session/new` + `session/prompt("hello")` → 验证 `session/update` 在 WS 收到。
- **断线 persist case**：连接 1 发起 prompt → 关闭连接 1 → 等待 prompt 完成 → 开连接 2 → `session/load` → 验证 replay buffer 中的 `session/update` 被灌入。
- **多连接 case**：连接 1 和连接 2 同时连，连接 1 发起 prompt，连接 2 收到的通知应与连接 1 一致（`recipient` 切换 + replay 灌入）。

### 7.3 e2e（手动）

1. `cargo run -p loom-server -- serve --host 127.0.0.1 --port 18081`
2. Zed 配 `acp.command = "loom acp --bridge --server ws://127.0.0.1:18081/acp"`
3. 发起一个 prompt，关闭 Zed，等几秒重开，验证 prompt 结果 + session 历史都在。

---

## 8. 落地步骤

1. **Phase 1 — 抽 transport 泛型**
   - `apps/acp/src/transport_loop.rs`：新建 `run_transport_with_agent(agent, conn_shared, transport, eof_signal)`
   - 把 `stdio_loop.rs:260-441` 的 10 个 handler 搬入
   - `spawn_drain_task` 改为 `pub`，`stdio_loop.rs` 和 server 侧均复用
   - `run_stdio_loop` 改为调 `run_transport_with_agent`，传入 `conn_shared`
   - `cargo test -p acp` 通过

2. **Phase 2 — bridge 模式**
   - `apps/acp/src/ws_bridge.rs`：新建 `run_bridge`
   - CLI 加 `--bridge --server <url>` 参数
   - 手动验证：`echo '{"jsonrpc":"2.0"...}' | loom acp --bridge --server ws://...`

3. **Phase 3 — handler 接线**
   - `apps/acp/Cargo.toml` 新增 `tokio-tungstenite` 依赖
   - `apps/server/src/handlers/acp.rs::connect` 改为 clone state 后 `on_upgrade(move |socket| handle_socket(state, socket))`
   - `handle_socket` 改为 `acp_hub.attach()` + `spawn_drain_task(notif_rx, conn_shared)` + `build_ws_transport` + `run_transport_with_agent(agent, conn_shared, transport, eof)`
   - 实现 `build_ws_transport`：WS ↔ `Lines` 类型适配 + `oneshot` EOF 信号
   - 删子进程 spawn 块、`loom_binary()`
   - `cargo build -p loom-server` 通过

4. **Phase 4 — e2e + 收尾**
   - 跑 `acp_ws_mega_e2e.rs`，新增断线 persist case
   - Zed 手动烟测
   - 更新 `docs/design/acp-websocket.md` + `acp-websocket-todo.md`

---

## 9. 后续工作

- `tools::set_connection` 全局 bridge store 改为 per-agent 或 per-session，消除并发覆盖风险。
- WS 写入背压：drain task 改用 bounded channel + 独立 task 解耦，避免阻塞 LocalSet。
- `acp-websocket-todo.md:21` — per-session actor 串行化，拒绝同一 session 并发 prompt。
- `acp-websocket-todo.md:25-32` — `disconnect_policy=cancel` 可配置 + idle TTL 清理。
- `acp-websocket-todo.md:46-50` — Bearer 鉴权主体写入 `AcpConnection`，升级 AcpHub 到 ClientKey 池。
- bridge 模式自动重连（WS 断 → backoff 重试，对 Zed 透明）。

---

## 10. 关联文件

### 10.1 本文档修改

- `docs/design/acp-websocket.md`（追加链接）
- `docs/design/acp-websocket-todo.md`（P1 各项勾选 + 链接）

### 10.2 代码现状

- `apps/server/src/routes.rs:29` — `GET /acp` 路由
- `apps/server/src/handlers/acp.rs:27-43` — `connect()`（升级前 handler，改为 clone state 传入 on_upgrade）
- `apps/server/src/handlers/acp.rs:89-188` — `handle_socket`（stdio 桥，待替换）
- `apps/server/src/handlers/acp.rs:72-87` — `loom_binary()`（待删除）
- `apps/server/src/handlers/acp.rs:48-64` — `origin_allowed`（保留）
- `apps/server/src/handlers/acp.rs:190-195` — `disconnect_cancels()`（保留，留作后续 cancel 策略）
- `apps/server/src/acp_hub.rs:13-94` — `AcpHub` + `attach`（复用）
- `apps/acp/src/stdio_loop.rs:108` — `run_stdio_loop`（改为消费 transport_loop）
- `apps/acp/src/stdio_loop.rs:154-162` — `build_agent_and_channel`（保留）
- `apps/acp/src/stdio_loop.rs:166-183` — `spawn_drain_task`（保留，改为 `pub`，server 侧复用）
- `apps/acp/src/stdio_loop.rs:198-252` — `build_stdio_transport`（保留）
- `apps/acp/src/stdio_loop.rs:260-441` — `register_handlers_and_connect`（搬到 `transport_loop.rs`）
- `apps/acp/src/tools/client_bridge.rs:102-117` — `set_connection`（全局 bridge store，server 模式需关注覆盖问题）
- `apps/acp/src/agent.rs` — `LoomAcpAgent`（不改）
- `apps/acp/Cargo.toml` — 需新增 `tokio-tungstenite` 依赖

### 10.3 新增文件

- `apps/acp/src/transport_loop.rs` — `run_transport_with_agent`（泛型 transport，接收 `conn_shared`）
- `apps/acp/src/ws_bridge.rs` — `run_bridge`（`loom acp --bridge` 入口）

### 10.4 既有设计文档

- `docs/design/acp-websocket.md` — `/acp` WS 入口说明
- `docs/design/acp-websocket-todo.md` — P0/P1/P2 工作清单
- `docs/opencode-protocol/acp-adjacent/acp-websocket-cli-ensure.md` — `loom acp --websocket` 拉起服务
