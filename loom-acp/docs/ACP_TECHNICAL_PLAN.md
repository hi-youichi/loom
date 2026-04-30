# ACP v0.10 → v0.11 技术实施方案

> 项目: loom-acp | 当前: 0.10 | 目标: 0.11.1
>
> 2025-08-19

---

## 一、变更清单总览

本文档列出从 v0.10 升级到 v0.11 所需的每一项代码变更，按文件组织，按优先级排序。

### 变更统计

| 文件 | 变更数 | 类型 |
|------|--------|------|
| `Cargo.toml` | 1 | 版本升级 |
| `src/agent.rs` | 4 | 新增方法 + 替换 hack |
| `src/lib.rs` | 1 | 可能的签名适配 |
| `src/client_methods.rs` | 0-2 | 可能的签名适配 |
| `src/stream_bridge.rs` | 1 | 类型适配 |
| `src/tools/client_bridge.rs` | 0-1 | 可能的 Client trait 适配 |
| `tests/*.rs` | 若干 | 跟随类型变更 |

---

## 二、逐文件变更方案

### 2.1 `Cargo.toml` — 依赖升级

**文件**: `loom-acp/Cargo.toml:19`

**变更**:

```toml
# 变更前
agent-client-protocol = { version = "0.10", features = [
    "unstable_boolean_config",
    "unstable_session_model",
    "unstable_session_fork",
] }

# 变更后
agent-client-protocol = { version = "0.11.1", features = [
    "unstable_boolean_config",
    "unstable_session_model",
    "unstable_session_fork",
] }
```

**验证**: `cargo update -p agent-client-protocol && cargo check -p loom-acp`

---

### 2.2 `src/agent.rs` — Agent trait 实现

#### 变更 A: 新增 `ext_method()` 必需方法

**位置**: `src/agent.rs:301` — `impl Agent for LoomAcpAgent` 块内

**问题**: v0.11 的 `Agent` trait 新增两个必需方法，不实现则编译失败。

**方案**: 在 `impl Agent for LoomAcpAgent` 块末尾（`list_sessions()` 方法之后，约 line 1021）添加：

```rust
// --- v0.11 新增必需方法 ---

async fn ext_method(
    &self,
    args: agent_client_protocol::ExtRequest,
) -> agent_client_protocol::Result<agent_client_protocol::ExtResponse> {
    tracing::warn!(method = %args.method, "ext_method called, returning default response");
    Ok(agent_client_protocol::ExtResponse::new(
        serde_json::value::to_raw_value(&serde_json::json!({"status": "unsupported"}))
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?
            .into(),
    ))
}

async fn ext_notification(
    &self,
    args: agent_client_protocol::ExtNotification,
) -> agent_client_protocol::Result<()> {
    tracing::warn!(method = %args.method, "ext_notification received, ignoring");
    Ok(())
}
```

**同时需要在 imports 中添加**（`src/agent.rs:15-23`）：

```rust
use agent_client_protocol::{
    Agent, AuthenticateRequest, AuthenticateResponse, CancelNotification, ExtNotification,
    ExtRequest, ExtResponse, ForkSessionRequest, ForkSessionResponse, InitializeRequest,
    InitializeResponse, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    SessionConfigOptionValue, SessionId, SessionNotification, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse,
    SetSessionModelRequest, SetSessionModelResponse, StopReason,
};
```

**注意**: 实际的 `ExtRequest`、`ExtResponse`、`ExtNotification` 类型名和构造方式需在 `cargo check` 后确认。如果 v0.11 中这些类型的构造 API 不同，按编译错误调整。

---

#### 变更 B: 替换 `initialize()` 中的 serde_json hack

**位置**: `src/agent.rs:319-351`

**当前代码**:

```rust
// Line 320-322: 部分使用 builder
let base_response = InitializeResponse::new(args.protocol_version).agent_info(
    agent_client_protocol::Implementation::new("loom", env!("CARGO_PKG_VERSION")),
);

// Line 324-346: serde_json hack 注入 agentCapabilities
let mut json = serde_json::to_value(&base_response)?;
if let Some(obj) = json.as_object_mut() {
    obj.insert("agentCapabilities", serde_json::json!({...}));
}
let response: InitializeResponse = serde_json::from_value(json)?;
```

**方案**: 检查 v0.11 是否提供 `InitializeResponse::agent_capabilities()` builder 方法。

如果提供，替换为：

```rust
let response = InitializeResponse::new(args.protocol_version)
    .agent_info(
        agent_client_protocol::Implementation::new("loom", env!("CARGO_PKG_VERSION"))
    )
    .agent_capabilities(
        serde_json::json!({
            "loadSession": true,
            "sessionCapabilities": {
                "list": {},
                "fork": {}
            },
            "promptCapabilities": {
                "embeddedContext": true,
                "image": true,
                "audio": true
            }
        })
    );
```

如果不提供（类型仍然 `non_exhaustive`），保留现有 serde_json hack，不做变更。

**决策点**: 在 Phase 1 执行 `cargo check` 后确认。

---

#### 变更 C: 替换 `build_session_config_options()` 中的 serde_json hack

**位置**: `src/agent.rs:1189-1233`

**当前代码**:

```rust
fn build_session_config_options(...) -> Result<Vec<SessionConfigOption>, serde_json::Error> {
    // Line 1212-1232: 手动构造 JSON 再反序列化
    let json = serde_json::json!([
        {
            "id": "mode",
            "name": "Mode",
            "type": "select",
            "currentValue": current_mode,
            "options": mode_options
        },
        {
            "id": "model",
            "name": "Model",
            "type": "select",
            "currentValue": current_model,
            "options": model_options
        }
    ]);
    serde_json::from_value(json)
}
```

**方案**: 检查 v0.11 是否提供 `SessionConfigOption::select()` builder。

如果提供，替换为：

```rust
fn build_session_config_options(
    current_mode: &str,
    current_model: &str,
    modes: &[agent_client_protocol::SessionMode],
    model_options: &[ModelOption],
) -> Result<Vec<agent_client_protocol::SessionConfigOption>, serde_json::Error> {
    let current_model = normalize_current_model_for_acp(current_model, model_options);

    let mode_select_options: Vec<_> = modes
        .iter()
        .map(|m| {
            agent_client_protocol::SessionConfigSelectOption::new(
                agent_client_protocol::SessionConfigValueId::new(m.id.to_string()),
                m.name.to_string(),
            )
        })
        .collect();

    let model_select_options: Vec<_> = model_options
        .iter()
        .map(|m| {
            agent_client_protocol::SessionConfigSelectOption::new(
                agent_client_protocol::SessionConfigValueId::new(m.id.clone()),
                m.name.clone(),
            )
        })
        .collect();

    let mode_option = agent_client_protocol::SessionConfigOption::select(
        agent_client_protocol::SessionConfigValueId::new("mode".to_string()),
        "Mode",
        agent_client_protocol::SessionConfigValueId::new(current_mode.to_string()),
        mode_select_options,
    )
    .description("Session behavior mode.")
    .category("mode");

    let model_option = agent_client_protocol::SessionConfigOption::select(
        agent_client_protocol::SessionConfigValueId::new("model".to_string()),
        "Model",
        agent_client_protocol::SessionConfigValueId::new(current_model),
        model_select_options,
    )
    .description("LLM model for this session.")
    .category("model");

    Ok(vec![mode_option, model_option])
}
```

如果不提供，保留现有代码。

**决策点**: 在 Phase 1 执行 `cargo check` 后确认。

---

#### 变更 D: 替换 `load_session()` 和 `list_sessions()` 中的 serde_json hack

**位置**:
- `src/agent.rs:922-930` — `LoadSessionResponse` 构造
- `src/agent.rs:946-1021` — `SessionInfo` 和 `ListSessionsResponse` 构造
- `src/agent.rs:1244-1248` — `SetSessionConfigOptionResponse` 构造

**方案**: 这些位置的 serde_json hack 是因为协议类型标记了 `non_exhaustive`。如果 v0.11 提供了 builder，替换；否则保留。

**关键点**: `list_sessions()` 中对 `SessionInfo` 的构造（line 948-1011）涉及大量字段拼接，即使有 builder 也需要逐字段迁移。

**决策点**: 在 Phase 1 确认每个类型是否有 builder 后逐一处理。

---

### 2.3 `src/lib.rs` — 传输层

**位置**: `src/lib.rs:258-265`

**当前代码**:

```rust
let (connection, io_future) = agent_client_protocol::AgentSideConnection::new(
    agent,
    stdout_compat,
    stdin_compat,
    |fut| {
        tokio::task::spawn_local(fut);
    },
);
```

**方案**: 根据迁移指南，v0.11 的 `AgentSideConnection::new()` 签名预计保持兼容。如果签名变更：

1. **参数顺序/类型变更** — 按新签名调整参数
2. **返回值类型变更** — 调整解构方式
3. **spawn 回调签名变更** — 调整闭包签名

**具体操作**:

```bash
# Phase 1: 先运行 cargo check，观察编译错误
cargo check -p loom-acp 2>&1 | grep -A5 "AgentSideConnection"
```

如果 `session_notification()` 方法签名变更，同时调整 line 279:

```rust
// 当前
match conn.session_notification(n).await {
```

**决策点**: Phase 1 `cargo check` 后确认。

---

### 2.4 `src/client_methods.rs` — Client trait 方法

**位置**: `src/client_methods.rs` 全文

**当前使用的 Client 方法**:

```rust
client.read_text_file(request).await?;        // line ~20
client.write_text_file(request).await?;       // line ~40
client.create_terminal(request).await?;       // line ~70
client.terminal_output(request).await?;       // line ~90
client.kill_terminal(request).await?;         // line ~100
client.release_terminal(request).await?;      // line ~110
client.wait_for_terminal_exit(request).await?;// line ~120
```

**方案**: 这些是 Client trait 的标准方法，v0.11 大概率保持兼容。如果签名变更（如返回类型从 `Result<T>` 变为 `Result<T, E>`），按编译错误调整。

**决策点**: Phase 1 `cargo check` 后确认。

---

### 2.5 `src/tools/client_bridge.rs` — Client Bridge

**位置**: `src/tools/client_bridge.rs:139`

**当前代码**:

```rust
pub fn new<C: agent_client_protocol::Client + 'static>(client: Arc<C>) -> Self {
```

**方案**: 如果 `Client` trait 新增必需方法，`AcpClientBridge` 通过 `Arc<C>` 委托，只要 `C` 是 `AgentSideConnection`（它实现了 `Client`），就不需要变更。

**决策点**: Phase 1 确认。

---

### 2.6 `src/stream_bridge.rs` — 流事件桥接

**位置**: `src/stream_bridge.rs:33-38`, `328-414`

**当前使用的类型**:

```rust
use agent_client_protocol::{
    ContentBlock, ContentChunk, CurrentModeUpdate, Diff, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, SessionId, SessionInfoUpdate, SessionModeId, SessionNotification,
    SessionUpdate, Terminal, TerminalId, TextContent, ToolCall, ToolCallId, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind, ToolCallContent,
};
```

**方案**: 如果 `SessionUpdate` 枚举新增变体（如 `ExtMethod` 相关），match 表达式需要处理新变体。如果类型构造方式变更（如 `ToolCall::new()` 签名改变），需要适配。

**关键点**: `stream_update_to_session_notification()` 函数中的 `match` 必须覆盖所有 `SessionUpdate` 变体。如果 v0.11 新增变体，需要添加 `_ => None` 通配或显式处理。

**决策点**: Phase 1 `cargo check` 后确认，特别关注 `non_exhaustive` 警告。

---

### 2.7 `src/content.rs` — ContentBlock 处理

**位置**: `src/content.rs:101-154`

**当前代码**:

```rust
match self {
    agent_client_protocol::ContentBlock::Text(t) => Some(t.text.clone()),
    agent_client_protocol::ContentBlock::Resource(r) => { ... }
    agent_client_protocol::ContentBlock::ResourceLink(rl) => { ... }
    _ => None,
}
```

**方案**: 已有 `_ => None` 通配，即使 `ContentBlock` 新增变体也能编译通过。无需变更。

---

### 2.8 `src/agent_registry.rs` — Session Mode 类型

**位置**: `src/agent_registry.rs:6`

```rust
use agent_client_protocol::{SessionMode, SessionModeId, SessionModeState};
```

**方案**: `SessionMode::new()`、`SessionModeId::new()`、`SessionModeState::new()` 这些基础构造器大概率保持兼容。如果不兼容，按编译错误调整。

---

### 2.9 测试文件

**涉及文件**: `tests/` 目录下所有 `.rs` 文件

**方案**:
1. 先修复主代码编译错误
2. 运行 `cargo test -p loom-acp 2>&1` 收集所有测试编译错误
3. 按相同模式批量修复（主要是类型签名变更）

---

## 三、执行流程

### Phase 1: 编译探测（0.5 天）

```
1. git checkout -b feat/upgrade-acp-0.11
2. 修改 Cargo.toml 版本为 "0.11.1"
3. cargo update -p agent-client-protocol
4. cargo check -p loom-acp 2>&1 | tee /tmp/acp-upgrade-errors.txt
5. 分析编译错误，分类为：
   a. 缺失 trait 方法 → 需要实现
   b. 签名不匹配 → 需要适配
   c. 类型不存在 → 需要替换
   d. non_exhaustive 匹配不完整 → 需要添加分支
```

### Phase 2: 逐项修复（1-2 天）

根据 Phase 1 的错误分析结果，按以下顺序修复：

```
优先级 1: Agent trait 新增方法（ext_method, ext_notification）
           → 文件: src/agent.rs
           → 操作: 添加空实现

优先级 2: AgentSideConnection 签名变更
           → 文件: src/lib.rs
           → 操作: 按新签名适配

优先级 3: 类型构造方式变更
           → 文件: src/agent.rs (InitializeResponse, SessionConfigOption 等)
           → 操作: 替换 hack 或调整 builder 调用

优先级 4: 其他编译错误
           → 文件: src/stream_bridge.rs, src/client_methods.rs 等
           → 操作: 按错误信息逐一修复
```

### Phase 3: 验证（0.5 天）

```bash
# 编译检查
cargo clippy -p loom-acp -- -D warnings

# 单元测试
cargo test -p loom-acp

# 集成测试（如果有 IDE 测试环境）
# 在 Zed 中配置 loom-acp 二进制路径，测试以下流程：
# - 连接和 initialize
# - 创建新会话
# - 发送 prompt 并接收流式输出
# - 切换模型/模式
# - 会话分叉
# - 加载已有会话
# - 列出会话
```

---

## 四、回滚方案

如果遇到无法解决的编译错误：

```bash
# 恢复 Cargo.toml
git checkout Cargo.toml

# 恢复 Cargo.lock
git checkout Cargo.lock

# 验证恢复
cargo check -p loom-acp
```

---

## 五、serde_json Hack 清理决策矩阵

项目中有多处使用 `serde_json::json!` + `serde_json::from_value()` 构造 `non_exhaustive` 类型。v0.11 是否需要清理取决于是否提供了原生 builder。

| 位置 | 类型 | 当前方式 | v0.11 处理 |
|------|------|---------|-----------|
| `agent.rs:324-349` | `InitializeResponse` | builder + serde_json hack 注入 `agentCapabilities` | 检查 `agent_capabilities()` builder |
| `agent.rs:922-930` | `LoadSessionResponse` | serde_json hack 注入 `configOptions` + `modes` | 检查 builder |
| `agent.rs:946-1021` | `SessionInfo` | serde_json hack 逐字段构造 | 检查 builder |
| `agent.rs:1014-1021` | `ListSessionsResponse` | serde_json hack | 检查 builder |
| `agent.rs:1212-1232` | `SessionConfigOption` | serde_json hack 构造 select 选项 | 检查 `select()` builder |
| `agent.rs:1244-1248` | `SetSessionConfigOptionResponse` | serde_json hack | 检查 builder |

**策略**: 如果 v0.11 提供了 builder，替换 hack 以获得类型安全和前向兼容；如果不提供，保留现有代码。不要在不确定的情况下强行重构。

---

## 六、风险和缓解

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| `Agent` trait 新增未知必需方法 | 高 | 编译失败 | Phase 1 先探测，按错误实现 |
| `AgentSideConnection::new()` 签名变更 | 中 | 传输层不可用 | 准备参数重排方案 |
| `SessionUpdate` 新增枚举变体 | 中 | match 不完整 | 添加 `_` 通配分支 |
| `ContentBlock` 新增变体 | 低 | 已有 `_` 通配 | 无需变更 |
| `Error` 构造 API 变更 | 低 | 全局错误处理失效 | Phase 1 探测后全局替换 |
| builder API 不可用 | 中 | 保留 hack | 不影响功能，只是技术债 |

---

## 七、详细开发计划

### 7.1 开发环境准备

| 步骤 | 命令/操作 | 预期结果 |
|------|----------|---------|
| 7.1.1 | `git checkout -b feat/upgrade-acp-0.11` | 创建升级分支 |
| 7.1.2 | `cargo test -p loom-acp` | 确认当前测试全部通过，记录基准 |
| 7.1.3 | `cargo clippy -p loom-acp -- -D warnings` | 确认当前无 clippy 警告 |
| 7.1.4 | `cp Cargo.lock Cargo.lock.v10.bak` | 备份当前 lock 文件 |
| 7.1.5 | 阅读官方迁移指南和 CHANGELOG | 了解 breaking changes |

### 7.2 Phase 1: 依赖升级和编译探测

**目标**: 升级依赖版本，收集所有编译错误，制定精确修复方案。

| 步骤 | 操作 | 验证 |
|------|------|------|
| 1.1 | 修改 `Cargo.toml` 版本为 `"0.11.1"` | 文件已修改 |
| 1.2 | `cargo update -p agent-client-protocol` | 依赖已更新 |
| 1.3 | `cargo check -p loom-acp 2>&1 \| tee /tmp/acp-phase1-errors.txt` | 收集编译错误 |
| 1.4 | 分析错误，分类为: A) 缺失 trait 方法 B) 签名不匹配 C) 类型不存在 D) non_exhaustive 匹配不完整 | 错误分类报告 |

**错误分类处理策略**:

- **A 类（缺失 trait 方法）**: 添加空实现，添加 `tracing::warn!` 日志
- **B 类（签名不匹配）**: 按新签名调整参数和返回值
- **C 类（类型不存在）**: 查阅 docs.rs 确认新类型名，替换
- **D 类（non_exhaustive）**: 添加 `_ =>` 通配分支或显式处理新变体

### 7.3 Phase 2: 逐文件修复（按优先级排序）

**修复顺序**: 每个文件修复后立即 `cargo check`，确保增量通过。

#### 7.3.1 `src/agent.rs` — Agent trait 实现

| 步骤 | 变更 | 预期影响 |
|------|------|---------|
| 2.1 | 添加 `ExtRequest`, `ExtResponse`, `ExtNotification` 到 imports | 编译依赖 |
| 2.2 | 实现 `ext_method()` — 返回 `unsupported` 默认响应 | 消除 trait 缺失错误 |
| 2.3 | 实现 `ext_notification()` — 记录日志并返回 `Ok(())` | 消除 trait 缺失错误 |
| 2.4 | 检查 `InitializeResponse` builder: 尝试 `.agent_capabilities()` | 如果存在则替换 serde_json hack |
| 2.5 | 检查 `SessionConfigOption::select()` builder | 如果存在则替换 serde_json hack |
| 2.6 | 检查 `LoadSessionResponse` / `ListSessionsResponse` / `SessionInfo` builder | 按结果逐个替换或保留 |
| 2.7 | `cargo check -p loom-acp` | agent.rs 编译通过 |

#### 7.3.2 `src/lib.rs` — 传输层

| 步骤 | 变更 | 预期影响 |
|------|------|---------|
| 2.8 | 检查 `AgentSideConnection::new()` 签名是否兼容 | 如果不兼容则按新签名调整 |
| 2.9 | 检查 `session_notification()` 方法签名 | 如果变更则调整调用 |
| 2.10 | 评估是否使用 `subscribe()` 方法接收流更新 | 可选 |
| 2.11 | `cargo check -p loom-acp` | lib.rs 编译通过 |

#### 7.3.3 `src/stream_bridge.rs` — 流事件桥接

| 步骤 | 变更 | 预期影响 |
|------|------|---------|
| 2.12 | 检查 `SessionUpdate` 枚举是否有新变体 | 添加 match 分支 |
| 2.13 | 检查 `ToolCall`, `ToolCallUpdate` 构造 API | 按变更调整 |
| 2.14 | 检查 `ContentChunk`, `Diff`, `Plan` 等类型 | 按变更调整 |
| 2.15 | `cargo check -p loom-acp` | stream_bridge.rs 编译通过 |

#### 7.3.4 `src/client_methods.rs` — Client trait 方法

| 步骤 | 变更 | 预期影响 |
|------|------|---------|
| 2.16 | 检查各 Client 方法签名是否兼容 | 按编译错误调整 |
| 2.17 | 检查请求/响应类型构造方式 | 按变更调整 |
| 2.18 | `cargo check -p loom-acp` | client_methods.rs 编译通过 |

#### 7.3.5 其他源文件

| 步骤 | 文件 | 变更 |
|------|------|------|
| 2.19 | `src/content.rs` | 已有 `_ => None` 通配，预期无需变更 |
| 2.20 | `src/tools/client_bridge.rs` | 检查 Client trait bound 兼容性 |
| 2.21 | `src/agent_registry.rs` | 检查 SessionMode 构造 API |
| 2.22 | `src/session.rs` | 检查是否有类型依赖变更 |
| 2.23 | `src/protocol.rs` | 检查是否有协议常量变更 |
| 2.24 | `src/session_config_store.rs` | 检查配置类型兼容性 |
| 2.25 | `cargo check -p loom-acp` | **全部源文件编译通过** |

### 7.4 Phase 3: 测试修复

#### 7.4.1 测试文件分组和修复策略

**组 1: 核心集成测试**（优先修复，其他测试依赖这些基础设施）

| 步骤 | 文件 | 内容 | 修复策略 |
|------|------|------|---------|
| 3.1 | `tests/common/mod.rs` | 测试公共模块 | 修复 ACP 类型 imports |
| 3.2 | `tests/common/acp_child.rs` | 子进程启动 | 检查 Agent 构造 |
| 3.3 | `tests/common/test_setup.rs` | 测试设置 | 检查 AgentSideConnection |
| 3.4 | `tests/common/config_helpers.rs` | 配置辅助 | 检查类型使用 |
| 3.5 | `tests/common/plan_types.rs` | Plan 类型 | 检查 PlanEntry 等类型 |
| 3.6 | `tests/common/terminal_handler.rs` | 终端处理 | 检查 Terminal 类型 |

**组 2: Mock 服务**（依赖组 1）

| 步骤 | 文件 | 内容 | 修复策略 |
|------|------|------|---------|
| 3.7 | `tests/mocks/mod.rs` | Mock 模块 | 修复 Agent trait 实现 |
| 3.8 | `tests/mocks/multi_tier_server.rs` | 多层 Mock 服务 | 新增 ext_method 等空实现 |
| 3.9 | `tests/mocks/plan_aware_server.rs` | Plan 感知 Mock | 新增 ext_method 等空实现 |

**组 3: Agent 功能测试**（依赖组 1+2）

| 步骤 | 文件 | 测试内容 |
|------|------|---------|
| 3.10 | `tests/agent_integration.rs` | Agent trait 集成 |
| 3.11 | `tests/agent_modes.rs` | 会话模式切换 |
| 3.12 | `tests/agent_model_resolution.rs` | 模型解析 |
| 3.13 | `tests/agent_plan_e2e.rs` | Plan 端到端 |

**组 4: 协议和功能测试**（依赖组 1+2）

| 步骤 | 文件 | 测试内容 |
|------|------|---------|
| 3.14 | `tests/e2e_tests.rs` | 通用 e2e 测试 |
| 3.15 | `tests/e2e/initialization.rs` | 初始化流程 |
| 3.16 | `tests/e2e/initialization_detailed.rs` | 初始化详细 |
| 3.17 | `tests/e2e/model_resolution.rs` | 模型解析 |
| 3.18 | `tests/e2e/session_lifecycle.rs` | 会话生命周期 |
| 3.19 | `tests/initialization_state_machine.rs` | 初始化状态机 |
| 3.20 | `tests/capabilities_structure.rs` | 能力结构 |
| 3.21 | `tests/mcp_capabilities.rs` | MCP 能力 |
| 3.22 | `tests/prompt_capabilities_e2e.rs` | Prompt 能力 |

**组 5: 会话和流式测试**（依赖组 1+2）

| 步骤 | 文件 | 测试内容 |
|------|------|---------|
| 3.23 | `tests/session_capabilities_e2e.rs` | 会话能力 |
| 3.24 | `tests/multi_turn_session_e2e.rs` | 多轮会话 |
| 3.25 | `tests/prompt_turn_e2e.rs` | Prompt 轮次 |
| 3.26 | `tests/stream_event_sequence_e2e.rs` | 流事件序列 |
| 3.27 | `tests/dynamic_model_switching.rs` | 动态模型切换 |
| 3.28 | `tests/cancellation_e2e.rs` | 取消流程 |

**组 6: 模型解析测试**（依赖组 1+2）

| 步骤 | 文件 | 测试内容 |
|------|------|---------|
| 3.29 | `tests/model_persistence.rs` | 模型持久化 |
| 3.30 | `tests/model_priority_resolution_e2e.rs` | 模型优先级解析 |
| 3.31 | `tests/model_tier_override.rs` | 模型层级覆盖 |
| 3.32 | `tests/title_tier_resolution_e2e.rs` | 标题层级解析 |
| 3.33 | `tests/subagent_tier_independence.rs` | 子代理层级独立 |

**组 7: 工具和终端测试**（依赖组 1+2）

| 步骤 | 文件 | 测试内容 |
|------|------|---------|
| 3.34 | `tests/test_fs_tools_integration.rs` | 文件工具集成 |
| 3.35 | `tests/test_terminal_integration.rs` | 终端工具集成 |
| 3.36 | `tests/terminal_e2e.rs` | 终端端到端 |
| 3.37 | `tests/test_content_types.rs` | 内容类型 |
| 3.38 | `tests/test_location.rs` | 位置信息 |

**组 8: 其他测试**

| 步骤 | 文件 | 测试内容 |
|------|------|---------|
| 3.39 | `tests/plan_bridge_test.rs` | Plan 桥接 |
| 3.40 | `tests/diff_protocol_e2e.rs` | Diff 协议 |
| 3.41 | `tests/log_file_subprocess.rs` | 日志子进程 |

#### 7.4.2 测试修复通用模式

测试文件中的常见修复模式：

```rust
// 模式 1: Mock Agent 新增 ext_method/ext_notification 空实现
async fn ext_method(&self, args: ExtRequest) -> Result<ExtResponse> {
    Ok(ExtResponse::new(
        serde_json::value::to_raw_value(&serde_json::json!({"status": "ok"}))
            .map_err(|e| agent_client_protocol::Error::internal_error().data(e.to_string()))?
            .into(),
    ))
}

async fn ext_notification(&self, _args: ExtNotification) -> Result<()> {
    Ok(())
}

// 模式 2: 类型构造方式变更 — 如果 builder 可用，替换 serde_json hack
// 模式 3: match 表达式新增变体 — 添加 _ => ... 通配或显式分支
// 模式 4: 方法签名变更 — 按新签名调整测试调用
```

### 7.5 Phase 4: 质量验证

| 步骤 | 命令 | 通过标准 |
|------|------|---------|
| 4.1 | `cargo clippy -p loom-acp -- -D warnings` | 零警告 |
| 4.2 | `cargo test -p loom-acp` | 全部测试通过 |
| 4.3 | `cargo build -p loom-acp --release` | Release 构建成功 |
| 4.4 | `cargo test -p loom-acp -- --nocapture` | 运行并审查输出日志 |

### 7.6 Phase 5: IDE 集成验证（手动测试）

| 步骤 | 操作 | 验证点 |
|------|------|--------|
| 5.1 | 在 Zed 中配置 loom-acp 二进制路径 | 连接成功 |
| 5.2 | 发送 `initialize` 请求 | 返回正确的 agentInfo 和 capabilities |
| 5.3 | 创建新会话 `new_session` | config_options 正确显示（mode + model 选择器） |
| 5.4 | 发送 prompt 并观察流式输出 | SessionUpdate 事件正确推送 |
| 5.5 | 切换模型 `set_session_config_option` | 模型切换成功 |
| 5.6 | 切换模式 `set_session_mode` | 模式切换成功 |
| 5.7 | 会话分叉 `fork_session` | 分叉会话独立运行 |
| 5.8 | 加载已有会话 `load_session` | 恢复成功 |
| 5.9 | 列出会话 `list_sessions` | 返回正确列表 |
| 5.10 | 发送取消 `cancel` | 正在执行的 prompt 被取消 |

### 7.7 里程碑和检查点

| 里程碑 | 完成标准 | 预计耗时 |
|--------|---------|---------|
| M1: 编译通过 | `cargo check -p loom-acp` 零错误 | 1-2 天 |
| M2: Clippy 通过 | `cargo clippy -p loom-acp -- -D warnings` 零警告 | M1 + 0.5 天 |
| M3: 测试全通过 | `cargo test -p loom-acp` 全绿 | M2 + 1-2 天 |
| M4: IDE 集成验证 | Zed 连接测试全流程通过 | M3 + 0.5 天 |
| M5: 代码审查就绪 | 文档更新、PR 创建 | M4 + 0.5 天 |

---

## 八、详细测试计划

### 8.1 测试策略概述

升级采用 **分层验证** 策略，从编译 → 单元测试 → 集成测试 → 手动测试，逐层确认兼容性。

```
编译检查 (cargo check)
  → Clippy 静态分析
    → 单元测试 (cargo test)
      → 集成测试 (Agent + Client trait 实现验证)
        → IDE 手动测试 (Zed 连接)
```

### 8.2 测试分类

#### 8.2.1 编译时测试（自动）

| 测试项 | 命令 | 关注点 |
|--------|------|--------|
| Agent trait 完整性 | `cargo check` | 所有必需方法已实现 |
| 类型兼容性 | `cargo check` | 无类型不匹配错误 |
| non_exhaustive 覆盖 | `cargo check` | match 表达式覆盖所有变体 |
| Clippy 规则 | `cargo clippy -- -D warnings` | 无代码质量问题 |

#### 8.2.2 功能回归测试（自动）

| 测试域 | 覆盖文件 | 关键验证点 |
|--------|---------|-----------|
| 初始化握手 | `tests/e2e/initialization*.rs` | InitializeResponse 字段完整 |
| 会话创建 | `tests/e2e/session_lifecycle.rs` | NewSessionResponse 包含 config_options |
| Prompt 流式输出 | `tests/prompt_turn_e2e.rs` | SessionUpdate 事件序列正确 |
| 模型切换 | `tests/dynamic_model_switching.rs` | set_session_model 切换成功 |
| 模式切换 | `tests/agent_modes.rs` | set_session_mode 切换成功 |
| 会话分叉 | `tests/agent_integration.rs` | fork_session 返回新 SessionId |
| 会话恢复 | `tests/e2e/session_lifecycle.rs` | load_session 恢复上下文 |
| 会话列表 | `tests/session_capabilities_e2e.rs` | list_sessions 返回完整列表 |
| 取消操作 | `tests/cancellation_e2e.rs` | cancel 中断正在执行的 prompt |
| 内容解析 | `tests/test_content_types.rs` | ContentBlock 各变体正确处理 |
| 工具调用 | `tests/test_fs_tools_integration.rs` | 文件读写正确委托 |
| 终端操作 | `tests/terminal_e2e.rs` | 终端创建/输出/退出正确 |

#### 8.2.3 协议合规测试（自动 + 手动）

| 测试项 | 方式 | 验证内容 |
|--------|------|---------|
| ExtMethod 默认响应 | 自动 | `ext_method()` 返回结构正确的 ExtResponse |
| ExtNotification 处理 | 自动 | `ext_notification()` 不 panic，返回 `Ok(())` |
| SessionConfigOption builder | 自动 | select/boolean 选项正确构造 |
| AgentCapabilities 声明 | 手动 | initialize 响应包含正确的能力声明 |
| 流式通知格式 | 手动 | SessionNotification 格式符合 v0.11 schema |

#### 8.2.4 性能和资源测试

| 测试项 | 方式 | 验证内容 |
|--------|------|---------|
| 编译时间对比 | `cargo build --timings` | 与 v0.10 基准对比 |
| 内存使用 | 手动观察 | 无明显内存增长 |
| 连接延迟 | 手动 | initialize 握手延迟无明显增加 |

### 8.3 新增测试用例

为 v0.11 新功能添加的测试：

#### 8.3.1 ExtMethod 测试

```rust
// 文件: tests/ext_method_e2e.rs (新增)
// 验证: ext_method 处理未知方法名返回 unsupported
// 验证: ext_method 处理带 params 的请求不 panic
// 验证: ext_notification 处理后返回 Ok(())
// 验证: 日志输出包含方法名
```

#### 8.3.2 Builder API 测试（如果替换了 serde_json hack）

```rust
// 文件: tests/builder_api_e2e.rs (新增)
// 验证: InitializeResponse::new().agent_info().agent_capabilities() 构造正确
// 验证: SessionConfigOption::select() 构造的选项在 IDE 中可渲染
// 验证: SessionConfigOption::boolean() 构造的开关选项默认值正确
```

#### 8.3.3 SessionInfo 更新测试（如果实现了 session_info_update）

```rust
// 文件: tests/session_info_update_e2e.rs (新增)
// 验证: SessionInfoUpdate 通知正确发送
// 验证: 标题更新后客户端收到通知
```

### 8.4 测试执行计划

| 阶段 | 执行时机 | 命令 | 通过标准 |
|------|---------|------|---------|
| T1: 源码编译 | Phase 2 每个文件修复后 | `cargo check -p loom-acp` | 零错误 |
| T2: Clippy | Phase 2 全部修复后 | `cargo clippy -p loom-acp -- -D warnings` | 零警告 |
| T3: 测试编译 | Phase 3 每组修复后 | `cargo test -p loom-acp --no-run` | 测试可执行文件生成成功 |
| T4: 单元测试 | Phase 3 组 1-2 修复后 | `cargo test -p loom-acp --lib` | lib 测试通过 |
| T5: 集成测试 | Phase 3 每组修复后 | `cargo test -p loom-acp --test <name>` | 该组测试通过 |
| T6: 全量测试 | Phase 3 完成后 | `cargo test -p loom-acp` | **全部测试通过** |
| T7: Release 构建 | Phase 4 | `cargo build -p loom-acp --release` | 构建成功 |
| T8: IDE 测试 | Phase 5 | 手动 | 全流程通过 |

### 8.5 失败处理流程

```
测试失败
  → 分析失败原因
    → 类型不兼容? → 检查 docs.rs 确认新 API
    → 逻辑变更? → 理解 v0.11 语义，调整测试预期
    → 新增必需字段? → 添加默认值填充
    → 无法解决? → 记录到 issue，考虑保留 hack 或回滚
```

### 8.6 测试报告模板

| 指标 | 值 |
|------|-----|
| 总测试数 | |
| 通过数 | |
| 失败数 | |
| 跳过数 | |
| 编译警告数 | |
| Clippy 警告数 | |
| 已知问题 | |

---

## 九、参考

- 迁移指南: https://agentclientprotocol.github.io/rust-sdk/migration_v0.11.x.html
- SDK CHANGELOG: https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol/CHANGELOG.md
- Schema CHANGELOG: https://github.com/agentclientprotocol/agent-client-protocol/blob/main/CHANGELOG.md
- API 文档: https://docs.rs/agent-client-protocol/0.11.1
- 产品说明书: `loom-acp/ACP_PRODUCT_GUIDE.md`
- 迁移计划: `loom-acp/ACP_MIGRATION_PLAN.md`
