# ACP v0.10 → v0.11 升级支持方案

> 项目: loom-acp | 当前版本: 0.10 | 目标版本: 0.11.1
>
> 编写日期: 2025-08-19

---

## 一、项目现状分析

### 1.1 依赖配置

```toml
# loom-acp/Cargo.toml:19
agent-client-protocol = { version = "0.10", features = [
    "unstable_boolean_config",
    "unstable_session_model",
    "unstable_session_fork",
] }
```

### 1.2 ACP API 使用面分析

#### 核心 Trait 实现

| 文件 | ACP API | 用途 |
|------|---------|------|
| `src/agent.rs:300` | `impl Agent for LoomAcpAgent` | ACP Agent 核心协议实现 |
| `src/lib.rs:185` | `use agent_client_protocol::Client` | Client trait 引用 |
| `src/lib.rs:258` | `AgentSideConnection::new()` | stdio 传输层连接 |

#### 请求/响应类型使用清单

| 类型 | 使用位置 | 用途 |
|------|---------|------|
| `InitializeRequest` / `InitializeResponse` | `src/agent.rs:302` | 握手和能力协商 |
| `AuthenticateRequest` / `AuthenticateResponse` | `src/agent.rs:353` | 认证（当前跳过） |
| `NewSessionRequest` / `NewSessionResponse` | `src/agent.rs:361` | 创建新会话 |
| `PromptRequest` / `PromptResponse` | `src/agent.rs:599` | 用户 prompt 处理 |
| `CancelNotification` | `src/agent.rs:404` | 取消正在执行的 prompt |
| `SetSessionConfigOptionRequest` / `Response` | `src/agent.rs:411` | 会话配置（model/mode） |
| `SetSessionModeRequest` / `Response` | `src/agent.rs:484` | 切换会话模式 |
| `SetSessionModelRequest` / `Response` | `src/agent.rs:502` | 切换会话模型 |
| `ForkSessionRequest` / `Response` | `src/agent.rs:527` | 会话分叉 |
| `LoadSessionRequest` / `Response` | `src/agent.rs:752` | 恢复已有会话 |
| `ListSessionsRequest` / `Response` | `src/agent.rs:932` | 列出所有会话 |
| `SessionId` | `src/agent.rs:371` 等 | 会话标识 |
| `StopReason` | `src/agent.rs:622` | prompt 结束原因 |
| `SessionNotification` | `src/agent.rs:43` | 会话更新通知 |
| `SessionConfigOptionValue` | `src/agent.rs:1250` | 配置值解析 |
| `SessionMode` / `SessionModeId` | `src/agent.rs:1277` | 会话模式类型 |

#### Client Trait 方法使用（通过 `AcpClientBridge`）

| 方法 | 文件 | 用途 |
|------|------|------|
| `read_text_file()` | `src/client_methods.rs:10` | 读取客户端文件 |
| `write_text_file()` | `src/client_methods.rs` | 写入客户端文件 |
| `create_terminal()` | `src/client_methods.rs` | 创建终端 |
| `terminal_output()` | `src/client_methods.rs` | 获取终端输出 |
| `kill_terminal()` | `src/client_methods.rs` | 终止终端 |
| `release_terminal()` | `src/client_methods.rs` | 释放终端 |
| `wait_for_terminal_exit()` | `src/client_methods.rs` | 等待终端退出 |

#### Schema 类型使用

| 类型 | 文件 | 用途 |
|------|------|------|
| `SessionConfigOption` / `SessionConfigSelectOption` | `src/agent.rs:1190` | 配置选项构建 |
| `SessionConfigValueId` | `src/agent.rs:1266` | 配置值 ID |
| `Implementation` | `src/agent.rs:320` | agent 信息标识 |
| `ContentBlock` 变体 | `src/content.rs` | 多模态内容解析 |
| `SessionUpdate` 变体 | `src/stream_bridge.rs` | 流式事件转换 |
| `ToolCall` / `ToolCallUpdate` | `src/stream_bridge.rs` | 工具调用状态 |
| `PlanEntry` / `PlanEntryPriority` / `PlanEntryStatus` | `tests/plan_bridge_test.rs` | 计划条目类型 |
| `Error` | 全局 | 错误处理 |
| `SessionInfo` | `src/agent.rs:947` | 会话信息 |

### 1.3 Unstable Feature 使用情况

| Feature | 状态 | 使用位置 |
|---------|------|---------|
| `unstable_boolean_config` | 已声明但未直接使用 | `Cargo.toml` |
| `unstable_session_model` | 活跃使用 | `src/agent.rs:502` `set_session_model()` |
| `unstable_session_fork` | 活跃使用 | `src/agent.rs:527` `fork_session()` |

### 1.4 测试覆盖

- `tests/agent_integration.rs` — 使用 `Agent` trait 直接测试
- `tests/agent_modes.rs` — 会话模式测试
- `tests/plan_bridge_test.rs` — 使用 `PlanEntry` 等类型
- 35+ 测试文件涉及 ACP 类型

---

## 二、v0.11 Breaking Changes 详细分析

### 2.1 SDK 架构重构（核心变更）

v0.11 进行了全新 SDK 设计（[#117](https://github.com/agentclientprotocol/rust-sdk/pull/117)），主要变化：

1. **连接 API 可能变更** — `AgentSideConnection::new()` 签名或行为可能改变
2. **Trait 方法签名可能变更** — `Agent` / `Client` trait 的方法可能调整
3. **类型构造方式可能变更** — builder 模式或构造函数可能改变

**影响范围**: `src/lib.rs:258`, `src/agent.rs:300`, `src/client_methods.rs`

### 2.2 Schema 0.10 → 0.11 变更

| 变更 | 影响等级 | 说明 |
|------|---------|------|
| `session/list` 稳定化 | 低 | feature flag 不再需要，但保留不影响 |
| `session_info_update` 稳定化 | 低 | 同上 |
| `session/config options` 稳定化 | 低 | 同上 |
| `ExtMethod` 新增 | 中 | 扩展方法类型需要适配 |
| `unstable_session_close` 新增 | 低 | 新 feature，可选启用 |
| `unstable_logout` 新增 | 低 | 新 feature，可选启用 |
| `unstable_elicitation` 新增 | 中 | 交互式确认机制 |
| `unstable_message_id` 新增 | 低 | 消息 ID 追踪 |
| `unstable_session_additional_directories` 新增 | 低 | 额外目录 |
| `unstable_nes` 新增 | 低 | NES 实现 |
| `unstable_auth_methods` 新增 | 低 | 多种认证 |
| `unstable_session_usage` 新增 | 低 | 使用量追踪 |
| `unstable_llm_providers` 新增 | 低 | LLM 提供商 |
| `unstable_cancel_request` 新增 | 低 | 取消请求 |
| `unstable_session_resume` 新增 | 低 | 恢复会话 |

### 2.3 具体代码影响评估

#### 高影响区域

**1. `AgentSideConnection::new()` 调用**（`src/lib.rs:258-265`）

```rust
// 当前代码
let (connection, io_future) = agent_client_protocol::AgentSideConnection::new(
    agent,
    stdout_compat,
    stdin_compat,
    |fut| { tokio::task::spawn_local(fut); },
);
```

v0.11 可能的变更:
- 构造函数签名改变（参数顺序、类型）
- 返回值类型改变
- spawn 回调签名改变
- 新增 `subscribe()` 方法用于接收流更新

**风险**: 这是最核心的变更点，如果 API 改变，整个 stdio 传输层需要重写。

**2. `Agent` trait 方法**（`src/agent.rs:300-1021`）

```rust
#[async_trait(?Send)]
impl Agent for LoomAcpAgent {
    async fn initialize(&self, args: InitializeRequest) -> Result<InitializeResponse> { ... }
    async fn authenticate(&self, args: AuthenticateRequest) -> Result<AuthenticateResponse> { ... }
    async fn new_session(&self, args: NewSessionRequest) -> Result<NewSessionResponse> { ... }
    async fn prompt(&self, args: PromptRequest) -> Result<PromptResponse> { ... }
    async fn cancel(&self, args: CancelNotification) -> Result<()> { ... }
    async fn set_session_config_option(&self, args: ...) -> Result<...> { ... }
    async fn set_session_mode(&self, args: ...) -> Result<...> { ... }
    async fn set_session_model(&self, args: ...) -> Result<...> { ... }
    async fn fork_session(&self, args: ...) -> Result<...> { ... }
    async fn load_session(&self, args: ...) -> Result<...> { ... }
    async fn list_sessions(&self, args: ...) -> Result<...> { ... }
}
```

v0.11 可能的变更:
- 方法签名调整（参数类型、返回类型）
- 新增必需方法（trait 扩展）
- 方法重命名
- `async_trait` 使用方式变更

**3. `Client` trait 使用**（`src/client_methods.rs`, `src/tools/client_bridge.rs`）

```rust
use agent_client_protocol::Client;

pub async fn read_text_file(client: &dyn Client, session_id: &SessionId, path: &str, ...) { ... }
```

v0.11 可能的变更:
- Client trait 方法签名变更
- 新增 Client 方法
- 请求/响应类型变更

#### 中等影响区域

**4. JSON 序列化 workaround**（多处使用 serde_json 构造 non_exhaustive 类型）

```rust
// src/agent.rs:325-348 — InitializeResponse 构造
let mut json = serde_json::to_value(&base_response)?;
if let Some(obj) = json.as_object_mut() {
    obj.insert("agentCapabilities", serde_json::json!({...}));
}
let response: InitializeResponse = serde_json::from_value(json)?;
```

```rust
// src/agent.rs:1190-1232 — SessionConfigOption 构造
let json = serde_json::json!([{...}, {...}]);
serde_json::from_value(json)
```

v0.11 可能的变更:
- 类型可能不再是 `non_exhaustive`，提供了原生构造方法
- builder API 改变
- 字段名或结构变更

**5. `Error` 类型使用**（全局）

```rust
agent_client_protocol::Error::new(-32602, "unknown session")
agent_client_protocol::Error::internal_error().data(e.to_string())
```

v0.11 可能的变更:
- Error 构造 API 改变
- 新增错误变体

#### 低影响区域

**6. 测试文件** — 使用 ACP 类型但逻辑简单，跟随类型变更即可

**7. Feature flags** — 当前使用的 3 个 unstable features 在 v0.11 中均保留

---

## 三、分阶段升级计划

### Phase 0: 准备工作（预计 0.5 天）

- [ ] **0.1** 创建升级分支 `feat/upgrade-acp-0.11`
- [ ] **0.2** 确保当前所有测试通过: `cargo test -p loom-acp`
- [ ] **0.3** 确保当前 clippy 通过: `cargo clippy -p loom-acp -- -D warnings`
- [ ] **0.4** 记录当前编译时间基准
- [ ] **0.5** 本地备份当前 Cargo.lock

### Phase 1: 依赖升级和编译修复（预计 1-2 天）

- [ ] **1.1** 更新 `Cargo.toml` 依赖版本

  ```toml
  # 将
  agent-client-protocol = { version = "0.10", features = [...] }
  # 改为
  agent-client-protocol = { version = "0.11", features = [
      "unstable_boolean_config",
      "unstable_session_model",
      "unstable_session_fork",
  ] }
  ```

- [ ] **1.2** 执行 `cargo update -p agent-client-protocol` 更新依赖
- [ ] **1.3** 执行 `cargo check -p loom-acp` 识别编译错误
- [ ] **1.4** 逐文件修复编译错误，优先级排序:
  1. `src/lib.rs` — `AgentSideConnection::new()` 签名变更
  2. `src/agent.rs` — `Agent` trait 方法签名变更
  3. `src/client_methods.rs` — `Client` trait 方法变更
  4. `src/stream_bridge.rs` — `SessionUpdate` 等类型变更
  5. `src/content.rs` — `ContentBlock` 类型变更
  6. 其他文件

### Phase 2: API 适配（预计 1-2 天）

- [ ] **2.1** 适配 `AgentSideConnection` 新 API
  - 阅读迁移指南确认 `new()` 签名
  - 适配 spawn 回调
  - 适配 `session_notification()` 方法
  - 检查是否需要使用新的 `subscribe()` 方法

- [ ] **2.2** 适配 `Agent` trait 新方法
  - 检查是否有新增必需方法
  - 适配已有方法签名变更
  - 移除或更新 `async_trait` 使用

- [ ] **2.3** 适配 `Client` trait 使用
  - 更新 `AcpClientBridge` 实现
  - 适配文件系统和终端操作方法

- [ ] **2.4** 适配 JSON 序列化 workaround
  - 检查 `InitializeResponse` 是否支持直接设置 `agentCapabilities`
  - 检查 `SessionConfigOption` 是否有新的 builder 方法
  - 检查 `LoadSessionResponse` / `ListSessionsResponse` 构造方式
  - 尽量使用原生 API 替代 `serde_json::from_value` hack

- [ ] **2.5** 适配 `Error` 类型
  - 检查 `Error::new()` 和 `Error::internal_error()` 是否改变
  - 检查 `.data()` 方法是否保留

### Phase 3: 测试修复（预计 1 天）

- [ ] **3.1** 修复 `tests/agent_integration.rs` — Agent trait 使用
- [ ] **3.2** 修复 `tests/agent_modes.rs` — 会话模式类型
- [ ] **3.3** 修复 `tests/plan_bridge_test.rs` — PlanEntry 等类型
- [ ] **3.4** 修复所有 e2e 测试文件（35+ 文件）
- [ ] **3.5** 修复 mock 文件中的 ACP 类型使用
- [ ] **3.6** 运行完整测试套件: `cargo test -p loom-acp`

### Phase 4: 新功能评估与可选集成（预计 0.5-1 天）

- [ ] **4.1** 评估是否启用 `unstable_elicitation`
  - 用途: 在 session/tool call/requests 中添加交互式确认
  - 影响: 需要实现新的 trait 方法
  - 建议: 暂不启用，待稳定后再考虑

- [ ] **4.2** 评估是否启用 `unstable_session_close`
  - 用途: 显式关闭会话，释放资源
  - 影响: 需要实现 `session/close` 方法
  - 建议: 启用，可以改善会话生命周期管理

- [ ] **4.3** 评估是否启用 `unstable_session_resume`
  - 用途: 增强会话恢复能力
  - 影响: 与现有 `load_session` 可能重叠或互补
  - 建议: 评估后决定

- [ ] **4.4** 评估是否启用 `unstable_message_id`
  - 用途: 消息 ID 追踪，改善调试
  - 影响: 需要在发送通知时填充 message_id
  - 建议: 可选启用

- [ ] **4.5** 评估是否启用 `unstable_logout`
  - 用途: 登出方法
  - 影响: 需要实现 logout handler
  - 建议: 暂不启用

### Phase 5: 验证和收尾（预计 0.5 天）

- [ ] **5.1** 完整 clippy 检查: `cargo clippy -p loom-acp -- -D warnings`
- [ ] **5.2** 完整测试: `cargo test -p loom-acp`
- [ ] **5.3** 编译检查: `cargo build -p loom-acp --release`
- [ ] **5.4** 集成测试: 与 IDE（Zed）实际连接测试
- [ ] **5.5** 更新 `loom-acp/ACP_UPGRADE_GUIDE.md` 文档
- [ ] **5.6** 更新 `src/protocol.rs` 协议文档
- [ ] **5.7** 提交代码审查

---

## 四、风险评估

### 高风险

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| `AgentSideConnection::new()` API 不兼容 | 整个传输层不可用 | 先在独立分支测试编译 |
| `Agent` trait 新增必需方法 | 编译失败 | 查看迁移指南，逐一实现 |
| 类型构造方式变更 | 多处 serde_json hack 失效 | 逐个替换为原生 API |

### 中等风险

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| `Client` trait 方法签名变更 | 文件/终端操作失败 | 适配 client_methods.rs |
| `ContentBlock` 变体变更 | 内容解析失败 | 检查 content.rs |
| 测试大量失败 | 开发效率降低 | 分批修复，优先修复核心测试 |

### 低风险

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Feature flag 行为变更 | 次要功能异常 | 逐一验证 |
| 性能回归 | 运行速度变慢 | 编译基准对比 |

---

## 五、回滚策略

如果升级遇到无法解决的阻塞问题:

1. 恢复 `Cargo.toml` 中的版本号为 `"0.10"`
2. 恢复 `Cargo.lock` 备份
3. 验证 `cargo test -p loom-acp` 通过
4. 记录阻塞原因，跟踪上游修复进度

---

## 六、参考资源

- **官方迁移指南**: https://agentclientprotocol.github.io/rust-sdk/migration_v0.11.x.html
- **Rust SDK 仓库**: https://github.com/agentclientprotocol/rust-sdk
- **SDK CHANGELOG**: https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol/CHANGELOG.md
- **Schema CHANGELOG**: https://github.com/agentclientprotocol/agent-client-protocol/blob/main/CHANGELOG.md
- **API 文档**: https://docs.rs/agent-client-protocol
- **Protocol 规范**: https://agentclientprotocol.com
- **v0.11 Design 文档**: https://agentclientprotocol.github.io/rust-sdk/design.html

---

## 七、时间估算总结

| 阶段 | 预计耗时 | 累计 |
|------|---------|------|
| Phase 0: 准备 | 0.5 天 | 0.5 天 |
| Phase 1: 编译修复 | 1-2 天 | 1.5-2.5 天 |
| Phase 2: API 适配 | 1-2 天 | 2.5-4.5 天 |
| Phase 3: 测试修复 | 1 天 | 3.5-5.5 天 |
| Phase 4: 新功能评估 | 0.5-1 天 | 4-6.5 天 |
| Phase 5: 验证收尾 | 0.5 天 | 4.5-7 天 |

**总计: 4.5-7 个工作日**（乐观 4.5 天，悲观 7 天）

关键路径在 Phase 1-2，取决于 `AgentSideConnection` 和 `Agent` trait 的变更幅度。建议在 Phase 0 完成后先阅读官方迁移指南，准确评估 Phase 1-2 的工作量。

---

## 八、v0.11 新功能详细说明

> 以下按「稳定功能」和「Unstable 功能」分类，每个功能包含：协议背景、Rust SDK API 变化、对 loom-acp 的影响、推荐操作。

### 8.1 稳定功能（无需 feature flag）

#### 8.1.1 `session/list` — 会话列表发现

**协议背景**

客户端通过 `session/list` 方法查询 agent 当前维护的所有会话。返回 `Vec<SessionInfo>`，每个条目包含 `session_id`、标题、创建时间等元数据。这让 IDE 可以在 UI 中展示历史会话、支持切换和清理。

**Rust SDK API 变化**

v0.10 中 `session/list` 需要 feature flag 启用；v0.11 中已稳定，不再需要 flag。

```rust
// Agent trait 中的方法（loom-acp 已实现）
async fn list_sessions(
    &self,
    args: ListSessionsRequest,
) -> Result<ListSessionsResponse>;
```

**对 loom-acp 的影响**

- `src/agent.rs:932` 已实现 `list_sessions()`，无需代码变更
- `Cargo.toml` 中如果有 `unstable_session_list` feature，可以安全移除
- `SessionInfo` 结构可能新增字段（如 `last_active_at`），需要检查是否需要适配

**推荐操作**

- 从 `Cargo.toml` 移除 `unstable_session_list`（如有）
- 检查 `ListSessionsResponse` 构造是否需要适配新字段

---

#### 8.1.2 `session_info_update` — 会话元数据实时推送

**协议背景**

Agent 通过 `session_info_update` 通知主动向客户端推送会话元数据变化（标题、状态等）。这是一个 notification（不需要响应），避免客户端轮询。

**Rust SDK API 变化**

v0.11 稳定了此功能。相关类型：

```rust
// SessionInfoUpdate 结构
pub struct SessionInfoUpdate {
    pub session_id: SessionId,
    pub title: Option<String>,
    // ... 其他可选字段
}

// 通过 connection 发送
connection.session_notification(SessionNotification::SessionInfoUpdate(update)).await
```

**对 loom-acp 的影响**

- 当前 loom-acp 通过 `stream_bridge` 发送 `SessionNotification`，可能已部分覆盖
- 检查是否需要在 `stream_bridge.rs` 中添加 `SessionInfoUpdate` 转换
- 可以利用此功能向 IDE 推送会话标题变化（如从 LLM 生成标题推断）

**推荐操作**

- 评估在 `stream_bridge` 中新增 `SessionInfoUpdate` 输出的场景
- 在 `SessionStore::create()` 后发送初始 `SessionInfoUpdate`

---

#### 8.1.3 `session/config options` — 会话级配置

**协议背景**

Agent 在 `new_session` 响应中返回 `config_options`，声明可配置的选项列表（下拉选择器、布尔开关等）。客户端在 UI 中渲染这些选项，用户选择后通过 `set_session_config_option` 更新。

支持两种配置类型：
- **Select（下拉选择器）**: `SessionConfigOption::select()` — 如模型选择
- **Boolean（开关）**: `SessionConfigOption::boolean()` — 如开关式配置（需 `unstable_boolean_config`）

**Rust SDK API 变化**

v0.11 中 `session/config options` 已稳定。Builder API 示例（来自官方 example）：

```rust
// 构建配置选项
let option = acp::SessionConfigOption::select(
    args.config_id,         // SessionConfigValueId
    "Example Option",       // 标题
    value,                  // 当前选中值 (SessionConfigValueId)
    vec![
        acp::SessionConfigSelectOption::new("option1", "Option 1"),
        acp::SessionConfigSelectOption::new("option2", "Option 2"),
    ],
);
Ok(acp::SetSessionConfigOptionResponse::new(vec![option]))
```

**对 loom-acp 的影响**

- `src/agent.rs:1190-1232` 当前使用 `serde_json::json!` 手动构造 `SessionConfigOption`，v0.11 可能提供原生 builder
- `src/agent.rs:411` 的 `set_session_config_option()` 已实现
- 需要检查 `NewSessionResponse` 中 `config_options` 的构造方式是否改变

**推荐操作**

- 替换 `serde_json::json!` hack 为 `SessionConfigOption::select()` / `SessionConfigOption::boolean()` builder
- 测试配置选项在 IDE（Zed）中是否正确渲染

---

#### 8.1.4 `ExtMethod` — 扩展方法支持

**协议背景**

ACP 允许 agent 和 client 发送协议规范之外的自定义请求/通知，通过 `_` 前缀的方法名标识（如 `_myCustom/method`）。这保证了协议兼容性的同时允许实验性功能。

涉及三个类型：
- `ExtRequest` — 扩展请求（`method` + `params`）
- `ExtResponse` — 扩展响应（任意 JSON）
- `ExtNotification` — 扩展通知（`method` + `params`，无需响应）

**Rust SDK API 变化**

v0.11 中 `Agent` trait 新增两个必需方法：

```rust
#[async_trait(?Send)]
impl Agent for LoomAcpAgent {
    // ... 已有方法 ...

    // 新增必需方法
    async fn ext_method(
        &self,
        args: ExtRequest,
    ) -> Result<ExtResponse, Error> {
        // 处理扩展请求，返回自定义响应
        Ok(ExtResponse::new(
            serde_json::value::to_raw_value(&json!({"status": "ok"}))?.into(),
        ))
    }

    // 新增必需方法
    async fn ext_notification(
        &self,
        args: ExtNotification,
    ) -> Result<(), Error> {
        // 处理扩展通知（无需响应）
        Ok(())
    }
}
```

同样，`Client` trait 也新增了对应的扩展方法，用于向 client 发送扩展请求。

**对 loom-acp 的影响**

- **必须实现**: `ext_method()` 和 `ext_notification()` 是 `Agent` trait 的必需方法
- 当前 `loom-acp/src/agent.rs` 未实现这两个方法，升级后会编译失败
- 初期可以用空实现（返回默认响应），后续可以用于自定义 IDE-Loom 协议扩展

**推荐操作**

- 在 `LoomAcpAgent` 中实现 `ext_method()` 和 `ext_notification()`
- 初期用日志记录 + 默认响应的空实现
- 未来可用于 Loom 特有的 IDE 集成功能（如自定义快捷操作、状态查询等）

---

#### 8.1.5 `clientInfo` / `agentInfo` — 实现信息交换

**协议背景**

在 `initialize` 握手阶段，client 和 agent 交换实现信息（名称、版本、标题等），便于识别、日志记录和兼容性诊断。

**Rust SDK API 变化**

```rust
// InitializeResponse builder API
Ok(InitializeResponse::new(ProtocolVersion::V1)
    .agent_info(Implementation::new("loom-acp", env!("CARGO_PKG_VERSION"))
        .title("Loom Agent")))
```

**对 loom-acp 的影响**

- `src/agent.rs:320-348` 当前通过 serde_json hack 设置 `agent_info`
- v0.11 的 `InitializeResponse` 提供原生 builder，可以替代 hack

**推荐操作**

- 替换 `src/agent.rs:325-348` 的 JSON workaround 为 `InitializeResponse::new().agent_info()` builder
- 检查 `client_info` 是否在 `InitializeRequest` 中可读取（用于识别 IDE 类型）

---

### 8.2 Unstable 功能（需要 feature flag）

#### 8.2.1 `unstable_elicitation` — 交互式确认机制

**协议背景**

Elicitation 是一种通用的交互式确认机制，允许 agent 在三种上下文中向用户请求确认：
1. **Session 级别** — 会话开始时的配置确认
2. **Tool Call 级别** — 工具执行前的操作确认（比 `request_permission` 更灵活）
3. **Request 级别** — 请求过程中的信息收集

支持多种响应类型：确认/拒绝/取消，可以携带结构化数据。

**Rust SDK API 变化**

需要 feature flag `unstable_elicitation`。可能在 `Agent` trait 中新增可选方法，或在现有方法中增加 elicitation 相关字段。

**对 loom-acp 的影响**

- 不实现不影响编译（unstable 方法通常是可选的）
- 可以增强 tool call 的用户体验（比纯 request_permission 更丰富的交互）
- 与当前 `src/agent.rs` 中的 `request_permission` 流程互补

**推荐操作**

- 暂不启用，等 feature 稳定后再评估
- 关注 schema 变更，了解 API 设计趋势

---

#### 8.2.2 `unstable_session_close` — 会话显式关闭

**协议背景**

允许客户端通过 `session/close` 方法显式关闭一个会话，释放资源。当前 ACP 没有标准的会话关闭机制，会话通常在进程退出时自动清理。

**Rust SDK API 变化**

需要 feature flag `unstable_session_close`。在 schema 0.11.1 中从 `session/stop` 重命名为 `session/close`。

可能新增 `Agent` trait 方法：

```rust
#[cfg(feature = "unstable_session_close")]
async fn close_session(
    &self,
    args: CloseSessionRequest,
) -> Result<CloseSessionResponse>;
```

**对 loom-acp 的影响**

- 可以在 `SessionStore` 中添加 `close()` 方法，清理线程和取消标志
- 改善资源管理：IDE 关闭 tab 时 agent 能立即释放资源
- 需要与 `SessionEntry` 的 cancel 机制协调

**推荐操作**

- 建议启用此 feature
- 在 `LoomAcpAgent` 中实现 `close_session()`，调用 `SessionStore::remove()` 并取消正在运行的 prompt

---

#### 8.2.3 `unstable_logout` — 登出方法

**协议背景**

允许客户端发起登出请求，agent 应清除认证状态。

**Rust SDK API 变化**

需要 feature flag `unstable_logout`。

**对 loom-acp 的影响**

- 当前 loom-acp 的 `authenticate()` 方法是空实现（返回默认响应）
- logout 同样可以为空实现

**推荐操作**

- 暂不启用

---

#### 8.2.4 `unstable_session_resume` — 会话恢复

**协议背景**

增强的会话恢复能力，与现有的 `load_session` 互补。可能提供更细粒度的恢复控制（如恢复到特定状态点、恢复上下文窗口等）。

**Rust SDK API 变化**

需要 feature flag `unstable_session_resume`。

**对 loom-acp 的影响**

- 当前 `src/agent.rs:752` 已实现 `load_session()`
- 如果 `session/resume` 提供额外能力（如恢复 tool call 状态），值得评估
- 可能与 Loom 的 checkpointer 功能协同

**推荐操作**

- 先评估与 `load_session` 的差异，再决定是否启用

---

#### 8.2.5 `unstable_message_id` — 消息 ID 追踪

**协议背景**

为每条消息（请求、响应、通知）分配唯一 ID，支持：
- 消息追踪和调试
- 去重和顺序保证
- 请求-响应关联

**Rust SDK API 变化**

需要 feature flag `unstable_message_id`。可能在 `SessionNotification` 等类型中新增 `message_id` 字段。

**对 loom-acp 的影响**

- 需要在发送通知时填充 `message_id`
- 改善调试体验（可以追踪消息流）
- 对 `stream_bridge.rs` 和 `run_stdio_loop()` 有轻微影响

**推荐操作**

- 可选启用，对调试有帮助
- 需要在 `SessionStore` 中添加消息 ID 计数器

---

#### 8.2.6 `unstable_session_additional_directories` — 额外目录

**协议背景**

允许客户端在 `session/new` 时指定多个工作目录，而不仅仅是单个 `working_directory`。这对多项目工作区场景很有用。

**Rust SDK API 变化**

需要 feature flag `unstable_session_additional_directories`。可能在 `NewSessionRequest` 中新增 `additional_directories` 字段。

**对 loom-acp 的影响**

- 当前 `SessionEntry` 只存储单个 `working_directory`
- 如果启用，需要扩展 `SessionEntry` 支持多目录
- Loom 的文件操作工具可能需要感知多目录

**推荐操作**

- 暂不启用，等需求明确后再考虑

---

#### 8.2.7 `unstable_nes` — NES 实现

**协议背景**

NES (Network Extension Service) 是一种网络扩展机制，具体用途尚在设计中。

**推荐操作**

- 暂不启用

---

#### 8.2.8 `unstable_auth_methods` — 多认证方式

**协议背景**

支持多种认证方式（API key、OAuth、token 等），不仅限于当前的单一 authenticate 流程。Schema 中新增 `AuthMethod` 枚举和相关类型。

**Rust SDK API 变化**

需要 feature flag `unstable_auth_methods`。新增类型：
- `AuthMethodId` — 认证方式标识
- `AuthMethodAgent` — Agent 自行处理认证
- 其他变体可能由 schema 定义

**对 loom-acp 的影响**

- 当前 `authenticate()` 是空实现
- 如果启用，需要实现多种认证方式的支持
- 可能与 Loom 的 provider 配置（API key 管理）集成

**推荐操作**

- 暂不启用

---

#### 8.2.9 `unstable_session_usage` — 会话使用量追踪

**协议背景**

允许 agent 报告会话的资源使用情况（token 消耗、API 调用次数等），客户端在 UI 中展示。

**Rust SDK API 变化**

需要 feature flag `unstable_session_usage`。

**对 loom-acp 的影响**

- 可以从 Loom 的 `AnyStreamEvent` 中提取 token 使用量
- 通过 `session/update` 或专门的 `session/usage` 通知推送

**推荐操作**

- 可选启用，对用户有价值（可以看到 token 消耗）
- 需要在 `stream_bridge` 中添加 usage 事件转换

---

#### 8.2.10 `unstable_llm_providers` — LLM 提供商支持

**协议背景**

允许 agent 向客户端声明支持的 LLM 提供商列表，客户端可以在 UI 中选择。

**Rust SDK API 变化**

需要 feature flag `unstable_llm_providers`。

**对 loom-acp 的影响**

- Loom 已通过 `session/config options` 的 `model` 选择器提供模型切换
- 此功能可能是更标准的替代方案

**推荐操作**

- 暂不启用，当前 `session/config options` 已满足需求

---

#### 8.2.11 `unstable_cancel_request` — 取消请求

**协议背景**

允许客户端取消正在处理的请求（不仅仅是 prompt，还包括其他长时间运行的请求）。

**对 loom-acp 的影响**

- 当前 `cancel()` 只处理 `CancelNotification`
- 新增的 `cancel_request` 可能提供更精确的取消控制

**推荐操作**

- 暂不启用

---

### 8.3 SDK 架构重构详情

#### 8.3.1 `AgentSideConnection` 变化

**v0.10 构造方式**

```rust
let (connection, io_future) = agent_client_protocol::AgentSideConnection::new(
    agent,           // impl Agent
    stdout_compat,   // impl AsyncWrite (outgoing)
    stdin_compat,    // impl AsyncRead (incoming)
    |fut| {          // spawn 函数
        tokio::task::spawn_local(fut);
    },
);
```

**v0.11 变化**

根据 docs.rs API 文档，v0.11 `AgentSideConnection::new()` 签名为：

```rust
pub fn new<A, W, R, S>(
    agent: A,
    outgoing_bytes: W,
    incoming_bytes: R,
    spawn: S,
) -> (Self, impl Future<Output = Result<(), Error>>)
where
    A: Agent + 'static,
    W: AsyncWrite + Unpin + 'static,
    R: AsyncRead + Unpin + 'static,
    S: Fn(Pin<Box<dyn Future<Output = ()>>>) + 'static,
```

**主要变更点**：
- 构造函数签名 **看起来兼容**，参数顺序和类型基本不变
- 新增 `subscribe()` 方法，返回 `StreamReceiver` 用于接收客户端流更新
- `session_notification()` 方法保留，用于发送通知到客户端
- 实现了 `Client` trait，可以直接通过 `connection` 调用 client 方法

**对 loom-acp 的影响**

- `src/lib.rs:258-265` 的 `AgentSideConnection::new()` 调用 **可能无需修改**
- 新增的 `subscribe()` 方法可以考虑用于接收客户端的流式更新（如实时取消）
- `AcpClientBridge`（`src/tools/`）通过 connection 调用 client 方式的模式保留

---

#### 8.3.2 `Agent` trait 方法清单

**v0.11 Agent trait 必需方法完整清单**：

```rust
pub trait Agent {
    // 核心协议方法（v0.10 已有）
    fn initialize(&self, args: InitializeRequest) -> Result<InitializeResponse>;
    fn authenticate(&self, args: AuthenticateRequest) -> Result<AuthenticateResponse>;
    fn new_session(&self, args: NewSessionRequest) -> Result<NewSessionResponse>;
    fn prompt(&self, args: PromptRequest) -> Result<PromptResponse>;
    fn cancel(&self, args: CancelNotification) -> Result<()>;

    // 会话管理方法（v0.10 已有）
    fn set_session_config_option(&self, args: ...) -> Result<...>;
    fn list_sessions(&self, args: ListSessionsRequest) -> Result<ListSessionsResponse>;

    // 新增必需方法（v0.11）
    fn ext_method(&self, args: ExtRequest) -> Result<ExtResponse>;
    fn ext_notification(&self, args: ExtNotification) -> Result<()>;

    // Unstable 可选方法（根据 feature flag）
    #[cfg(feature = "unstable_session_model")]
    fn set_session_model(&self, args: SetSessionModelRequest) -> Result<SetSessionModelResponse>;

    #[cfg(feature = "unstable_session_fork")]
    fn fork_session(&self, args: ForkSessionRequest) -> Result<ForkSessionResponse>;

    #[cfg(feature = "unstable_session_close")]
    fn close_session(&self, args: CloseSessionRequest) -> Result<CloseSessionResponse>;

    // ... 其他 unstable 方法
}
```

**loom-acp 需要新增实现的方法**：

| 方法 | 是否必需 | 建议 |
|------|---------|------|
| `ext_method()` | 必需 | 空实现 + 日志 |
| `ext_notification()` | 必需 | 空实现 + 日志 |
| `close_session()` | 可选（unstable） | 建议实现 |

---

#### 8.3.3 `Client` trait 方法清单

**v0.11 Client trait 方法完整清单**：

```rust
pub trait Client {
    // 已有方法（v0.10）
    fn read_text_file(&self, args: ReadTextFileRequest) -> Result<ReadTextFileResponse>;
    fn write_text_file(&self, args: WriteTextFileRequest) -> Result<WriteTextFileResponse>;
    fn create_terminal(&self, args: CreateTerminalRequest) -> Result<CreateTerminalResponse>;
    fn terminal_output(&self, args: TerminalOutputRequest) -> Result<TerminalOutputResponse>;
    fn kill_terminal(&self, args: KillTerminalRequest) -> Result<KillTerminalResponse>;
    fn release_terminal(&self, args: ReleaseTerminalRequest) -> Result<ReleaseTerminalResponse>;
    fn wait_for_terminal_exit(&self, args: WaitForTerminalExitRequest) -> Result<WaitForTerminalExitResponse>;

    // 权限请求（已有）
    fn request_permission(&self, args: RequestPermissionRequest) -> Result<RequestPermissionResponse>;

    // 新增方法（v0.11）
    fn ext_method(&self, args: ExtRequest) -> Result<ExtResponse>;
    fn ext_notification(&self, args: ExtNotification) -> Result<()>;

    // Session notification 发送
    fn session_notification(&self, notification: SessionNotification) -> Result<()>;

    // 新增：订阅流更新
    fn subscribe(&self) -> StreamReceiver;
}
```

**对 loom-acp 的影响**

- `src/client_methods.rs` 的方法签名可能需要适配
- `AcpClientBridge` 可能需要实现 `ext_method` 和 `ext_notification` 的代理
- `subscribe()` 方法可以用于接收客户端的取消和流控制消息

---

#### 8.3.4 Builder API 变化

v0.11 为多个类型提供了原生 builder，替代了 `serde_json::json!` hack：

```rust
// InitializeResponse — v0.11 builder
let response = InitializeResponse::new(ProtocolVersion::V1)
    .agent_info(Implementation::new("loom-acp", "0.1.0").title("Loom Agent"))
    .agent_capabilities(AgentCapabilities::new()
        .session_list(SessionListCapabilities::new())
        // ...
    );

// SessionConfigOption — v0.11 builder
let option = SessionConfigOption::select(
    config_id,
    "Model",
    current_value,
    vec![
        SessionConfigSelectOption::new("claude-sonnet-4", "Claude Sonnet 4"),
        SessionConfigSelectOption::new("gpt-4o", "GPT-4o"),
    ],
);

// Boolean 配置（需要 unstable_boolean_config）
let bool_option = SessionConfigOption::boolean(
    config_id,
    "Enable Caching",
    true, // 默认值
);

// ExtResponse — v0.11
let response = ExtResponse::new(
    serde_json::value::to_raw_value(&json!({"result": "ok"}))?.into(),
);
```

**对 loom-acp 的影响**

| 当前 hack 位置 | v0.11 替代方案 |
|---------------|--------------|
| `src/agent.rs:325-348` JSON 构造 `InitializeResponse` | `InitializeResponse::new().agent_info().agent_capabilities()` |
| `src/agent.rs:1190-1232` JSON 构造 `SessionConfigOption` | `SessionConfigOption::select()` / `SessionConfigOption::boolean()` |
| `src/agent.rs:947` 构造 `SessionInfo` | 检查是否有 builder |

---

### 8.4 功能与 loom-acp 文件映射总表

| v0.11 功能 | 涉及文件 | 影响类型 | 优先级 |
|-----------|---------|---------|--------|
| `ext_method` / `ext_notification` | `src/agent.rs` | 必须实现 | P0 |
| Builder API（InitializeResponse） | `src/agent.rs:320-348` | 代码优化 | P1 |
| Builder API（SessionConfigOption） | `src/agent.rs:1190-1232` | 代码优化 | P1 |
| `session/list` 稳定化 | `src/agent.rs:932` | 移除 feature flag | P2 |
| `session_info_update` | `src/stream_bridge.rs` | 新功能 | P2 |
| `session/config options` 稳定化 | `src/agent.rs:411` | 移除 feature flag | P2 |
| `unstable_session_close` | `src/agent.rs`, `src/session.rs` | 新功能 | P2 |
| `unstable_message_id` | `src/stream_bridge.rs`, `src/lib.rs` | 新功能 | P3 |
| `unstable_session_usage` | `src/stream_bridge.rs` | 新功能 | P3 |
| `unstable_elicitation` | `src/agent.rs` | 新功能 | P3 |
| `unstable_session_resume` | `src/agent.rs` | 新功能 | P3 |
| `unstable_auth_methods` | `src/agent.rs` | 新功能 | P4 |
| `unstable_llm_providers` | `src/agent.rs` | 新功能 | P4 |
| `unstable_cancel_request` | `src/agent.rs` | 新功能 | P4 |
| `unstable_logout` | `src/agent.rs` | 新功能 | P4 |
| `unstable_session_additional_directories` | `src/session.rs` | 新功能 | P4 |
| `unstable_nes` | — | 暂不需要 | P5 |
