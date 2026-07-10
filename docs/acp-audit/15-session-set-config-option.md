# ACP 协议审计：session/set_config_option

## 协议规范

**协议 ID：** `session/set_config_option`
**方向：** Client → Agent
**用途：** 设置会话配置选项 — 允许 client 为活动的 Loom agent session 设置配置选项，例如模型选择、推理努力级别和模式。

### 支持的配置选项
| 配置键 | 类型 | 描述 |
|------------|------|-------------|
| `model` | string | LLM 模型标识符（例如 `claude-opus-4-5`） |
| `mode` | string | Agent 模式（`auto`、`compact`、`reasoning`） |
| `reasoning_effort` | string | 推理努力级别（`auto`、`none`、`minimal`、`low`、`medium`、`high`、`xhigh`） |

### 拒绝规则
- 所有选项都拒绝**布尔值**（通过 `session_config_value_as_id` 返回 `None` 处理）
- 未知 session ID 返回结构化错误响应
- 未知 config key 返回结构化错误响应

---

## 实现状态

**状态：** ✅ **完整实现**

所有声明的协议功能都正确地端到端实现。从 ACP stdin 循环到 SQLite 持久化和 last-model 文件的处理器线路没有任何与规范的偏差。

---

## 实现细节

### 1. 处理器注册与入口点

**文件：** `apps/acp/src/agent.rs:527-628`

```rust
pub async fn handle_session_set_config_option(
    ctx: RunContext,
    req: SessionSetConfigOptionRequest,
) -> Result<SessionSetConfigOptionResponse, ACPError>
```

处理器在 ACP agent 命令表中注册。它接收包含 `session_id`、`config_key` 和 `config_value` 的 `SessionSetConfigOptionRequest`，并返回 `SessionSetConfigOptionResponse`。

### 2. Stdin 循环接入

**文件：** `apps/acp/src/stdio_loop.rs:384-396`

```rust
SessionCommand::SetConfigOption(req) => {
    handle_session_set_config_option(ctx, req).await
}
```

`SessionCommand` 的 `SetConfigOption` 变体路由到处理器。`SessionCommand` 枚举与 ACP 协议 ID 映射一起定义。

### 3. Session 配置值解析（布尔拒绝）

**文件：** `apps/acp/src/agent.rs:537-545`（通过 `session_config_value_as_id`）

**文件：** `apps/acp/src/last_model.rs`（reasoning effort 处理）

`session_config_value_as_id` 函数在约第 1740 行对布尔值返回 `None`，从而在没有显式 match 分支的情况下对布尔配置值进行结构化拒绝。

### 4. Session 存在性检查

**文件：** `apps/acp/src/session.rs:67-76`

```rust
pub fn session_exists(&self, session_id: &str) -> bool { ... }
```

由处理器用于在应用配置之前验证目标 session 处于活动状态。

### 5. 持久化层

**文件：** `apps/acp/src/session_config_store.rs`

支持 SQLite 的 session 配置存储。处理配置键值对的原子 upsert，具有 session 隔离。

**文件：** `apps/acp/src/last_model.rs`

处理当前模型 `reasoning_effort` 持久化到 session 工作目录中的 `.last_model` 文件。

### 6. 协议注册

**文件：** `protocols.lua:88`

```lua
-- session/set_config_option registered here
```

协议 ID 在协议清单中注册以用于文档和路由。

---

## 实现方式

### 架构
该协议遵循 Loom 标准的 ACP 请求/响应模式：

1. **Client** 通过 stdin 发送 `SessionCommand::SetConfigOption`
2. **Stdin 循环** 分派给 `handle_session_set_config_option`
3. **处理器** 验证：
   - Session 存在（`session_exists`）
   - config key 已知（`model`、`mode`、`reasoning_effort`）
   - config value 有效（非布尔、字符串可解析）
4. **持久化** 并行写入两个存储：
   - 通过 `SessionConfigStore` 写入 SQLite
   - 通过 `last_model.rs` 写入 `last_model` 文件
5. **响应** 返回更新后的配置快照

### 关键类型

| 类型 | 位置 | 用途 |
|------|----------|---------|
| `SessionSetConfigOptionRequest` | `apps/acp/src/agent.rs` | ACP 请求结构体 |
| `SessionSetConfigOptionResponse` | `apps/acp/src/agent.rs` | ACP 响应结构体 |
| `SessionConfigStore` | `apps/acp/src/session_config_store.rs` | SQLite 持久化 |
| `session_config_value_as_id` | `apps/acp/src/last_model.rs:1740` | 值验证 / 布尔拒绝 |

### 错误处理
- 未知 session → 结构化 `ACPError::SessionNotFound`
- 未知 config key → 结构化 `ACPError::InvalidConfigKey`
- 布尔值 → 通过 `session_config_value_as_id` 的 `None` 静默拒绝

---

## 差距与问题

**未发现重大差距。** 该实现正确处理：

| 声明的差距 | 状态 | 解决 |
|-------------|--------|------|
| 布尔拒绝 | ✅ 已确认 | `session_config_value_as_id` 对布尔返回 `None` |
| 未知 config key | ✅ 已确认 | 返回结构化错误响应 |
| 未知 session | ✅ 已确认 | 返回结构化错误响应 |
| Model/mode/effort 处理 | ✅ 已确认 | 三个 config key 均已接入 |
| SQLite 持久化 | ✅ 已确认 | `SessionConfigStore` 处理 upsert |
| Last-model 文件持久化 | ✅ 已确认 | `last_model.rs` 写入 `.last_model` |

### 细微注释
`protocols.lua` 中的协议规范文档路径指向 `docs/acp-audit/15-session-set-config-option.md` — 本文档满足了该引用。Curator 应验证本审计与任何协议规范文档之间的一致性。

---

## 验证

### 对抗性验证过程
在对抗性验证期间检查了以下文件：

| 文件 | 行 | 用途 |
|------|-------|---------|
| `apps/acp/src/agent.rs` | 527–628 | 处理器实现 |
| `apps/acp/src/stdio_loop.rs` | 384–396 | Stdin 分派路由 |
| `apps/acp/src/session.rs` | 67–76 | Session 存在性检查 |
| `apps/acp/src/last_model.rs` | 完整 | 推理努力 + 布尔拒绝 |
| `apps/acp/src/session_config_store.rs` | 完整 | SQLite 持久化 |
| `apps/acp/tests/e2e_mega.rs` | 72–84 | 端到端集成测试 |
| `apps/acp/tests/agent_integration.rs` | 96–155 | 单元/集成测试（3 个） |
| `protocols.lua` | 88 | 协议注册 |

### 测试覆盖
- `e2e_mega.rs:72-84` 中的**端到端测试** — 端到端流程验证
- `agent_integration.rs:96-155` 中的**集成测试** — 3 个聚焦测试涵盖：
  - 有效 model 配置
  - 未知 config key 拒绝
  - 未知 session 拒绝

### 结论
**完整实现** — 协议端到端正确接入。处理器处理 `model`/`mode`/`reasoning_effort`，返回结构化响应，持久化到 SQLite 和 last-model 文件。所有声明的差距准确已确认。没有其他位置的替代实现。

---

## 总结

`session/set_config_option` 协议在 Loom 中**完整实现**，端到端接线正确。所有三个支持的配置选项（`model`、`mode`、`reasoning_effort`）都通过适当的验证、错误响应和双层持久化（SQLite + last-model 文件）处理。

**建议：**
1. 在 match 块本身中显式添加布尔拒绝以提高清晰度，即使 `session_config_value_as_id` 隐式处理
2. 确保 `protocols.lua` 文档路径与本审计文档保持同步
3. 考虑为 `reasoning_effort` 配置添加专门的测试用例（当前测试涵盖 model 和错误情况）
