# Cargo Nextest 使用指南

## Profile 说明

| Profile | 用途 | 超时 | 命令 |
|---------|------|------|------|
| `default` | 完整测试 | 60s | `cargo nextest run` |
| `fast` | 快速验证，自动标记慢测试 | 10s | `cargo nextest run --profile fast` |
| `ci` | CI 环境，立即输出失败 | 60s | `cargo nextest run --profile ci` |

## 按包运行

```bash
cargo nextest run -p config
cargo nextest run -p loom-acp --profile fast
```

## 配置文件

`.config/nextest.toml`

## 慢测试清单

以下测试超过 10s，属于已知慢测试：

### loom-acp::agent_modes（每个 ~40s）

- **根因**：`LoomAcpAgent::new_session()` 调用 `get_available_models()`，触发真实网络请求获取模型列表（`loom-acp/src/agent.rs:393`）
- **影响测试**：`test_new_session_returns_modes_with_ask_and_default`、`test_set_session_mode_and_load_preserves_mode` 等 8 个
- **修复方向**：mock `get_available_models()`，预计可将每个测试从 ~40s 降到 <1s

### loom-acp e2e 测试（每个 spawn 子进程，10-30s）

以下测试文件通过 `AcpChild::spawn_with_mock()` 启动真实的 `loom-acp` 二进制子进程：

| 测试二进制 | 说明 |
|-----------|------|
| `agent_plan_e2e` | Agent plan e2e |
| `cancellation_e2e` | 取消请求 e2e |
| `capabilities_structure` | 能力声明结构验证 |
| `diff_protocol_e2e` | Diff 协议 e2e |
| `e2e_tests` | 通用 e2e 入口 |
| `initialization_state_machine` | 初始化状态机 |
| `log_file_subprocess` | 日志文件子进程 |
| `mcp_capabilities` | MCP 能力 |
| `model_priority_resolution_e2e` | 模型优先级解析 |
| `multi_turn_session_e2e` | 多轮会话 e2e |
| `prompt_capabilities_e2e` | Prompt 能力 e2e |
| `prompt_responder_e2e` | Prompt 响应 e2e |
| `prompt_turn_e2e` | Prompt 轮次 e2e |
| `session_capabilities_e2e` | Session 能力 e2e |
| `session_load_e2e` | Session 加载 e2e |
| `stream_event_sequence_e2e` | 流事件序列 e2e |
| `terminal_e2e` | Terminal e2e |
| `title_tier_resolution_e2e` | 标题层级解析 e2e |

- **修复方向**：这些是集成测试，无法简单 mock。建议在 CI 中单独运行，本地开发时排除。

### 其他已知慢测试

| 包 | 测试 | 原因 |
|----|------|------|
| `serve` | `tests/e2e/*` | 启动 axum 服务器，30-90s read timeout |
| `cli` | `server_e2e` | 启动 loom CLI 服务器 |
| `loom` | `mcp_session` / `mcp_tool_source` | spawn mcp-filesystem-server（已 `#[ignore]`） |

## 快速测试基线

以下测试在 10s 内完成：

| 包 | 测试数 | 耗时 |
|----|--------|------|
| `config` | 96 | ~0.5s |
| `loom-acp` (test_content_types) | 4 | ~0.01s |
| `loom-acp` (test_fs_tools_integration) | 7 | ~0.04s |
| `loom-acp` (test_location) | 7 | ~0.01s |
| `loom-acp` (test_terminal_integration) | 7 | ~0.25s |
| `loom-acp` (src 内联单元测试) | ~100 | ~1s |

## 快速运行命令

```bash
# 只跑快速测试（<10s）
cargo nextest run --profile fast -p config
cargo nextest run --profile fast -p loom-acp -E 'binary(test_content_types) + binary(test_fs_tools_integration) + binary(test_location) + binary(test_terminal_integration)'

# 排除 e2e 跑 loom-acp
cargo nextest run --profile fast -p loom-acp -E 'not (binary(=~"_e2e") | binary(=~"e2e_tests") | binary(=~"log_file_subprocess") | binary(=~"agent_modes") | binary(=~"agent_integration") | binary(=~"initialization_state_machine") | binary(=~"capabilities_structure") | binary(=~"mcp_capabilities"))'
```

## CI 集成

`telegram-bot` CI 已更新为使用 nextest：

```yaml
- name: Install nextest
  uses: taiki-e/install-action@v2
  with:
    tool: cargo-nextest

- name: Test
  run: cargo nextest run -p telegram-bot
```
