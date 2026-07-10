# ACP 协议审计：session-new

## 协议规范

`session-new` 创建一个新的 Loom agent session。它接受工作目录（`cwd`）、可选的 MCP 服务器配置（`mcp_servers`），并返回 `sessionId` 以及 Loom 特定的扩展（`modes`、`config_options`）。处理器将 `cwd` 接入日志并设置 agent 的 `working_folder`，通过 `acp_mcp_to_loom()` 转换 MCP 服务器配置，生成 session ID，并触发机会性的 curator hook。

## 实现状态

**已实现** — 已确认完整且正确的实现。

## 实现细节

| 关注点 | 文件 | 行 | 符号 |
|---------|------|-------|--------|
| ACP 处理器 | `apps/acp/src/agent.rs` | 411–514 | `handle_session_new()` |
| Stdio 循环接入 | `apps/acp/src/stdio_loop.rs` | 315–327 | `SessionNew` 的 `match` 分支 |
| Session 存储 / 创建 | `apps/acp/src/session.rs` | 1–180 | `SessionStore::create()` |
| 协议方法枚举 | `apps/acp/src/protocol.rs` | 24–29 | `ProtocolMethod::SessionNew` |
| 端到端覆盖 | `apps/acp/tests/e2e_mega.rs` | 53–67 | — |
| 端到端覆盖 | `apps/acp/tests/e2e_usage_meta.rs` | 74–82 | — |
| Cargo 依赖 | `apps/acp/Cargo.toml` | 29 | — |

### 处理器签名（`agent.rs:411`）

```rust
async fn handle_session_new(
    &self,
    id: &Str <Result>,
    cwd: &Str <Result>,
    mcp_servers: Option<Vec<AcpMcpServer>>,
    context: &mut RunContext,
) -> Result<NewSessionResponse, AgentError>
```

### 响应类型（关键字段）

```rust
struct NewSessionResponse {
    session_id: String,          // 来自 SessionStore::create()
    modes: Vec<Mode>,            // Loom 扩展
    config_options: HashMap<String, serde_json::Value>, // Loom 扩展
}
```

### Stdio 循环接入（`stdio_loop.rs:315–327`）

```rust
ProtocolMethod::SessionNew => {
    self.handle_session_new(id, cwd, mcp_servers, context).await?
}
```

## 实现方式

1. `cwd` 存储在 `RunContext` 中，用于初始化 agent 的 `working_folder` 和日志路径。
2. `mcp_servers`（`Option<Vec<AcpMcpServer>>`）在传递给 session 之前，通过 `acp_mcp_to_loom()` 转换为 Loom 的内部 MCP 配置。
3. `SessionStore::create()` 生成 `sessionId` 并持久化 session 状态。
4. 响应使用从 agent 当前配置填充的 Loom 扩展字段 `modes` 和 `config_options` 构建。
5. session 创建后触发机会性的 curator hook（`gap_curator` 事件）。

## 差距与问题

- **`session/resume` 未实现** — 这超出了 `session-new` 协议的范围，并被正确识别为如此。
- 未识别出其他差距。所有文档化的字段都已处理；`modes` 和 `config_options` 扩展被正确填充。

> **注：** 先前的分析引用了 `"apps/acp/tests/e2e/common/jsonrpc.rs:74` — `"session/new"` 字符串字面量` 作为测试参考。Grep 确认该文件中不存在这样的字面量。实际的测试引用位于 `e2e_mega.rs:56` 和 `e2e_usage_meta.rs:78`（总共 4 处匹配）。这只是一个引用错误，不影响实现结论。

## 验证

对抗性验证已确认：

- 处理器存在于 `agent.rs:411–514` 且已完整实现
- 在 `stdio_loop.rs:315–327` 的 stdio 循环中接入
- `SessionStore::create()` 正确生成 `sessionId`
- 响应中存在 `modes` / `config_options` Loom 扩展
- `cwd` → 日志 + `working_folder` 映射正确
- 存在 `mcp_servers` → `acp_mcp_to_loom` 转换
- 创建时触发机会性的 curator hook
- `e2e_mega.rs` 和 `e2e_usage_meta.rs` 中的端到端测试覆盖

**结论：已确认** — session/new 已完整且正确地实现。

## 总结

`session-new` **完整实现**，与 ACP 协议规范匹配，具有正确的 Loom 特定扩展。未发现偏差或缺失功能。唯一记录的项（`session/resume` 缺失）超出了范围。建议操作：无 — 此协议已就绪。
