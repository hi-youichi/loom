# ACP 协议审计：authenticate

## 协议规范

`authenticate` ACP 协议方法是 Client → Agent 请求，允许 client 使用指定的认证方法进行身份验证。根据 ACP 规范，agent 首先在 `InitializeResponse` 中声明可用的认证方法（通过 `auth_methods` 字段）。当需要认证时，agent 在其响应中返回 `auth_required`，提示 client 使用 `method_id` 和可选凭证调用 `authenticate`。由于 `authenticate` 以声明 `auth_methods` 为前提，它是可选的、由 client 发起的协议方法。

## 实现状态

**部分实现** — 处理器存根存在且结构正确，但由于 Loom 从未在 `InitializeResponse` 中声明任何 `auth_methods`，故设计上不可达。

## 实现细节

### 处理器注册

**文件：** `apps/acp/src/protocol.rs:20-22`

```rust
pub const AUTHENTICATE: &str = "authenticate";
```

该协议常量与其他 ACP 方法常量一同声明。

### 处理器定义

**文件：** `apps/acp/src/agent.rs:403-409`

```rust
fn handle_authenticate(
    _agent: Arc<Agent>,
    _args: AuthenticateRequest,
) -> ACPResult<AuthenticateResponse> {
    Ok(AuthenticateResponse::default())
}
```

一个同步处理器，丢弃其参数并返回 `AuthenticateResponse::default()`。

### 处理器注册

**文件：** `apps/acp/src/agent.rs:352-400`

处理器通过 `define_acp_method!` 宏注册在 `agent.rs` 第 352-400 行附近。注册将 `AUTHENTICATE` 常量映射到 `handle_authenticate`。

### Stdio 循环桥接

**文件：** `apps/acp/src/stdio_loop.rs:29-30` 和 `303-314`

stdio 循环读取请求并通过与其他方法相同的 ACP 调度机制分派给处理器。

**已确认的文件引用：**
- `apps/acp/src/agent.rs:15` — `AuthenticateRequest` / `AuthenticateResponse` 类型的导入
- `apps/acp/src/stdio_loop.rs:29-30` — stdio 分派中的协议常量使用

## 实现方式

Loom 将 `authenticate` 实现为**有意为之的存根**：

- `AuthenticateResponse` 类型（来自 `agent-client-protocol` v0.15.1，schema v1）只有一个字段 `meta: Option<Map<String, Value>>`。`Default` derive 给出 `meta: None`，序列化为实际上的空响应 `{}`。
- 处理器接收 `AuthenticateRequest`（包含必需的 `method_id: AuthMethodId` 字段）但完全丢弃它。
- `AuthenticateResponse::default()` 是符合规范的正确响应 — 由 GitHub 上的官方 `rust-sdk` 示例确认，该示例使用相同模式。
- **该处理器不可达**，因为 Loom 从未在 `InitializeResponse` 中包含 `auth_methods`。根据 ACP 规范，client 仅在收到 `auth_required` 后才调用 `authenticate`，而 Loom 从未发出过此信号。这是经过深思熟虑的。

## 差距与问题

| 问题 | 严重程度 | 状态 |
|-------|----------|------|
| 处理器丢弃 `method_id` | 低 | 非差距 — 处理器按设计不可达 |
| `InitializeResponse` 中没有 `auth_methods` | 无 | 有意为之；使 `authenticate` 不可达 |
| 没有 `authenticate` 的集成测试 | 无 | 已通过穷尽的工作区 grep 确认 |

**陈旧产物：** `tests/e2e/main.rs:4` 包含一个 Phase 3 计划注释，承诺了一个 `authenticate.rs` 测试模块。该模块从未创建，与 `session_load`、`terminal`、`llm_error` 测试一同列为未来工作。

工作区中不存在其他实现、`auth_required` 发出、或 `auth_methods` 声明。

## 验证

对抗性验证已确认：
- 引用的全部 6 个文件位置存在且内容完全如所述
- `AuthenticateResponse::default()` 定义良好（单一可选 `meta` 字段，默认为 `None` → 序列化为 `{}`）
- 全工作区穷尽 grep 未发现 `auth_methods` 声明、无 `auth_required` 响应，也无 `authenticate` 测试
- 官方 `rust-sdk` 示例同样使用 `AuthenticateResponse::default()`，确认这符合规范
- `tests/e2e/main.rs:4` 是陈旧的 Phase 3 计划注释，非测试占位符

**结论：已验证（含澄清）** — 原始分析准确。`authenticate` ACP 方法是一个正确实现的有意存根。未发现遗漏的实现。

## 总结

`authenticate` ACP 协议方法**作为功能性特性被有意未实现**，但**作为协议存根被正确实现**。Loom 不声明 `auth_methods` 的设计决策使该处理器对任何符合规范的 client 都不可达，这是不需要认证的系统的正确行为。无需任何操作。唯一建议的清理是：如果不会兑现，则移除 `tests/e2e/main.rs:4` 中陈旧的 Phase 3 测试计划注释。
