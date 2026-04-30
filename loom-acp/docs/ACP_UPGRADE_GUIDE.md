# Agent Client Protocol (ACP) Rust SDK 升级指南

> 当前版本: `0.10` | 最新版本: `0.11.1` | Schema 最新版本: `0.12.1`
>
> 生成日期: 2025-08-19

## 概述

`agent-client-protocol` Rust SDK 从 v0.10 升级到 v0.11 是一次重大更新，包含 SDK 架构重构和多个协议功能的变化。

## 版本对比

| 组件 | 当前版本 | 最新版本 |
|------|---------|---------|
| `agent-client-protocol` | 0.10 | 0.11.1 |
| `agent-client-protocol-schema` | (0.10.x) | 0.12.1 |

## v0.11.0 变更详情

### 重大变更（Breaking Changes）

- **SDK 设计重构** — v0.11 迁移到全新的 SDK 架构（[#117](https://github.com/agentclientprotocol/rust-sdk/pull/117)）
  - API 接口有 breaking changes，需要按迁移指南修改代码
  - 官方迁移指南: <https://agentclientprotocol.github.io/rust-sdk/migration_v0.11.x.html>

### RPC 层修复

- 发送响应到对端失败时记录错误日志（[#101](https://github.com/agentclientprotocol/rust-sdk/pull/101)）
- 修复 `handle_io` 循环中的写入失败处理（[#99](https://github.com/agentclientprotocol/rust-sdk/pull/99)）
- 使用 `RawValue::NULL` 常量替代 `from_string().unwrap()`（[#96](https://github.com/agentclientprotocol/rust-sdk/pull/96)）

## v0.11.1 变更详情

- 移除 `boxfnonce` 依赖，改用标准库 `Box<dyn FnOnce>`（[#137](https://github.com/agentclientprotocol/rust-sdk/pull/137)）

## Schema 变更（0.10.x → 0.12.1）

### 已稳定的功能

| 功能 | 说明 |
|------|------|
| `session/list` | 客户端可发现 agent 已有的会话，支持历史切换和清理 |
| `session_info_update` | Agent 实时推送会话元数据更新（标题等），无需轮询 |
| `session/config options` | 会话级配置：模型、模式、推理级别等选择器 |
| `ExtMethod` | 扩展方法支持，允许添加自定义功能同时保持协议兼容 |
| `clientInfo` / `agentInfo` | 初始化期间共享实现信息，便于识别和兼容性诊断 |

### 新增 Unstable 功能（需 feature flag 启用）

| Feature Flag | 说明 |
|-------------|------|
| `unstable_elicitation` | Session / tool call / requests 中的交互式确认机制 |
| `unstable_logout` | 登出方法支持 |
| `unstable_session_close` | 会话关闭方法 |
| `unstable_session_additional_directories` | 额外工作目录支持 |
| `unstable_nes` | NES 实现 |
| `unstable_message_id` | 消息 ID 追踪 |
| `unstable_auth_methods` | 多种认证方式支持 |
| `unstable_session_usage` | 会话使用量追踪 |
| `unstable_llm_providers` | LLM 提供商支持 |
| `unstable_cancel_request` | 取消请求支持 |
| `unstable_session_resume` | 恢复会话支持 |
| `unstable_session_model` | 会话模型切换 |
| `unstable_session_fork` | 会话分叉 |
| `unstable_boolean_config` | 布尔类型配置选项 |

### v0.12.0 Schema 变更

- 移除未使用的 RPC message schema 类型（schema.json 不变）
- 改进可选字段反序列化的容错处理
- 保留 `_` 前缀用于扩展方法，拒绝空的 ext 方法名

## 当前项目使用的 Features

```toml
# loom-acp/Cargo.toml:19
agent-client-protocol = { version = "0.10", features = [
    "unstable_boolean_config",
    "unstable_session_model",
    "unstable_session_fork",
] }
```

这些 feature flags 在 v0.11 中仍然存在，可以继续使用。

## 升级步骤

1. **阅读迁移指南**: <https://agentclientprotocol.github.io/rust-sdk/migration_v0.11.x.html>
2. **更新 Cargo.toml 版本**: `"0.10"` → `"0.11"`
3. **按迁移指南修改 API 调用** — SDK 架构重构可能导致接口变化
4. **运行 `cargo clippy -- -D warnings`** 检查代码
5. **运行测试**: `cargo test` 确保功能正常

## 参考链接

- Rust SDK 仓库: <https://github.com/agentclientprotocol/rust-sdk>
- Protocol 规范: <https://agentclientprotocol.com>
- 迁移指南: <https://agentclientprotocol.github.io/rust-sdk/migration_v0.11.x.html>
- API 文档: <https://docs.rs/agent-client-protocol>
- crates.io: <https://crates.io/crates/agent-client-protocol>
