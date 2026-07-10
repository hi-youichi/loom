# ACP 协议审计：session/set_mode

## 协议规范

`session/set_mode` 协议提供了一种机制，使 client 可以请求切换 Agent 的操作模式。这是 client 到 agent 的请求（`Client -> Agent`），能够在运行时动态重新配置 agent 行为，无需重启 session。

## 实现状态

**已实现** — 所有组件确认存在并功能正常。

## 实现细节

### 处理器注册
**文件：** `apps/acp/src/agent.rs:312-339`

通过 `handle_session_set_mode` 函数注册处理器，将协议连接到 agent 的命令路由。

### 核心实现
**文件：** `apps/acp/src/agent.rs:630-646`

`set_session_mode` 函数处理实际的模式转换逻辑：
- 验证传入的模式请求
- 更新内部 agent 状态
- 触发通知发出

**文件：** `apps/acp/src/agent.rs:560-565`

`apply_session_mode` 将验证的模式更改应用于 agent 的运行时配置。

### 通知桥接
**文件：** `apps/acp/src/stream_bridge.rs:892-900`

发出通知以通知订阅者（CLI、其他监听器）模式更改事件。

### Agent Registry 集成
**文件：** `apps/acp/src/agent_registry.rs:39-41`

通过 agent registry 持久化模式状态以实现跨 session 连续性。

### 持久化层
**文件：** `apps/acp/src/stdio_loop.rs:398-409`

基于 SQLite 的 session 模式状态持久化，确保模式在 agent 重启后仍然保留。

### 测试覆盖
**文件：** `apps/acp/tests/e2e_mega.rs:86-97` 和 `apps/acp/tests/e2e_mega.rs:223-234`

两个端到端测试用例验证完整的请求-响应周期：
- 第 86-97 行的测试：主要 happy-path 验证
- 第 223-234 行的测试：边缘情况 / 错误处理验证

## 实现方式

Loom 通过分层架构实现 `session/set_mode`：

1. **处理器层**（`agent.rs`）：协议请求反序列化和路由
2. **逻辑层**（`set_session_mode` / `apply_session_mode`）：模式转换的状态机
3. **通知层**（`stream_bridge.rs`）：向订阅者广播更改事件
4. **持久化层**（`stdio_loop.rs`）：基于 SQLite 的持久状态存储
5. **Registry 层**（`agent_registry.rs`）：内存和持久化的 agent 元数据

该设计遵循 Loom 已确立的 ACP 模式，在命令处理、状态变更和副作用传播之间清晰分离。

## 差距与问题

未识别出差距。对抗性验证确认：
- 处理器注册：存在
- `set_session_mode` 实现：存在
- `apply_session_mode` 逻辑：存在
- 通知发出：存在
- SQLite 持久化：存在
- 端到端测试覆盖：两个测试用例均存在

## 验证

**过程：** 跨所有 8 个已确认位置的独立文件验证。

**已验证文件：**
- `apps/acp/src/stdio_loop.rs:398-409` — 持久化
- `apps/acp/src/agent.rs:312-339` — 处理器注册
- `apps/acp/src/agent.rs:630-646` — 核心实现
- `apps/acp/src/agent.rs:560-565` — 应用逻辑
- `apps/acp/src/stream_bridge.rs:892-900` — 通知
- `apps/acp/src/agent_registry.rs:39-41` — Registry
- `apps/acp/tests/e2e_mega.rs:86-97` — 主要端到端
- `apps/acp/tests/e2e_mega.rs:223-234` — 次要端到端

**结论：完整实现** — 所有发现已独立确认。无需更正。

## 总结

`session/set_mode` 协议在 Loom 中已完整且正确地实现。所有必需的组件（处理器、逻辑、通知、持久化、registry）均存在，并具有足够的测试覆盖。规范无偏差，没有未解决的问题。已准备好投入生产使用。
