# Architecture — telegram-bot

## Overview

基于 [teloxide](https://github.com/teloxide/teloxide) 的 Telegram 多机器人框架，集成 Loom Agent 提供流式对话、媒体下载、模型切换等功能。

```
┌─────────────────────────────────────────────────────────┐
│                      main.rs                            │
│  load config → setup logging → spawn health server      │
│                → run_with_config (per bot)               │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│                     bot.rs                               │
│  DispatcherBuilder → long polling loop                   │
│  each bot: CancellationToken for graceful shutdown       │
└──────────────────────┬──────────────────────────────────┘
                       │ incoming Message
┌──────────────────────▼──────────────────────────────────┐
│                   router.rs                              │
│  teloxide Update handler → extract Message              │
│  → call handle_message_with_deps(deps, msg)             │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│                pipeline/mod.rs                           │
│  handle_common_message(ctx)                              │
│  1. resolve download dir                                 │
│  2. route: text flow | media flow                        │
│     text:  commands → mention gate → agent               │
│     media: download file → attach to prompt → agent      │
└──────────────────────┬──────────────────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        │              │              │
┌───────▼──────┐ ┌─────▼──────┐ ┌────▼──────────┐
│ command/     │ │ download.rs│ │ streaming/    │
│ /model       │ │            │ │               │
│ /reset       │ │ File-      │ │ agent.rs      │
│ /help        │ │ Downloader │ │  ↓            │
└──────────────┘ │            │ │ event_mapper  │
                 └────────────┘ │  ↓            │
                                │ message_      │
                                │ handler.rs    │
                                └───────┬───────┘
                                        │ StreamCommand
                                 ┌──────▼──────┐
                                 │ sender.rs   │
                                 │ (Telegram   │
                                 │  API calls) │
                                 └─────────────┘
```

## Module Responsibilities

| Module | 职责 | 关键类型 |
|--------|------|----------|
| `config/` | 配置加载、环境变量插值 | `Settings`, `BotConfig`, `TelegramBotConfig` |
| `bot.rs` | 多机器人生命周期管理、Dispatcher 构建 | `BotRunner` |
| `router.rs` | teloxide 入口，委托给 pipeline | `default_handler` |
| `pipeline/` | 消息处理流水线：下载→路由→Agent | `MessageContext`, `handle_common_message` |
| `command/` | 斜杠命令 (Command pattern) | `CommandDispatcher`, `BotCommand` |
| `streaming/agent.rs` | 调用 Loom Agent，产生流式事件 | `run_loom_agent_streaming` |
| `streaming/event_mapper.rs` | 适配器: Loom 事件 → StreamCommand | `StreamEventMapper` |
| `streaming/message_handler.rs` | 消费 StreamCommand，节流更新 Telegram 消息 | `stream_message_handler` |
| `streaming/retry.rs` | Telegram API 重试 (固定间隔) | `send_message_with_retry` |
| `sender.rs` | teloxide Bot 的 MessageSender 实现 | `TeloxideSender` |
| `download.rs` | 文件下载 (photo/video/document) | `TeloxideDownloader`, `FileMetadata` |
| `model_selection.rs` | 模型搜索/切换，SQLite 存储 | `ModelSelectionService`, `ModelCatalog` |
| `session.rs` | 会话重置 | `SqliteSessionManager` |
| `health.rs` | HTTP 健康检查 (/health, /ready) | `HealthState` |
| `metrics.rs` | 原子计数器指标 | `BotMetrics` |
| `formatting/` | MarkdownV2/HTML 格式化 + fallback | `FormattedMessage` |
| `traits.rs` | 依赖注入 trait 定义 | `AgentRunner`, `MessageSender`, `FileDownloader` |
| `handler_deps.rs` | 依赖组装容器 | `HandlerDeps` |
| `mock.rs` | 测试 mock 实现 | `MockAgentRunner`, `MockSender` |

## Data Flow: Streaming Message

```
User sends message
       │
       ▼
  router.rs: default_handler()
       │
       ▼
  pipeline/mod.rs: handle_common_message()
       │  → create MessageContext
       │  → mention gate check
       │  → rate limit check
       ▼
  streaming/agent.rs: run_loom_agent_streaming()
       │  → call loom::run_agent_with_options()
       │  → receive AnyStreamEvent stream
       ▼
  streaming/event_mapper.rs: StreamEventMapper
       │  → map AnyStreamEvent → StreamCommand
       │  → manage phase state (Think → Act → Tool → Done)
       │  → send commands via mpsc channel
       ▼
  streaming/message_handler.rs: stream_message_handler()
       │  → consume StreamCommand from channel
       │  → throttle edits (300ms interval)
       │  → format text (MarkdownV2 with fallback)
       ▼
  sender.rs: TeloxideSender
       │  → send/edit Telegram messages
       │  → retry on transient failures
       ▼
  Telegram API
```

## Configuration

```
~/.loom/
├── config.toml              # Loom 主配置 (LLM provider, API keys)
├── telegram-bot.toml        # Bot 专用配置
└── .env                     # 环境变量 (TELOXIDE_TOKEN, etc.)
```

配置加载顺序:
1. `config` crate 加载 `~/.loom/config.toml` + `.env`
2. `config/loader.rs` 定位 `telegram-bot.toml`
3. 环境变量插值: `${VAR}` → `std::env::var("VAR")`

## Testing Strategy

| 层级 | 文件 | 方法 |
|------|------|------|
| 单元测试 | `src/tests/formatting_tests.rs` | 直接测试格式化函数 |
| 单元测试 | `src/tests/handler_tests.rs` | 测试 handler 辅助函数 |
| 单元测试 | `src/config/tests.rs` | 测试配置加载 |
| Mock 集成 | `tests/integration_test.rs` | MockAgentRunner + MockSender |
| Mock 集成 | `tests/handler_dispatch_mock_test.rs` | 合成 Message → dispatch |
| Mock 集成 | `tests/streaming_message_handler_test.rs` | StreamCommand → handler |
| E2E | `tests/bot_startup_test.rs` | 验证启动/空配置行为 |

所有测试使用 `mock.rs` 中的 Mock 实现，不依赖真实 Telegram API。

## Key Dependencies

| Crate | 用途 |
|-------|------|
| `teloxide` 0.13 | Telegram Bot API 框架 |
| `loom` (workspace) | Agent 运行时 + 流式事件 |
| `config` (workspace) | 配置加载 + 环境变量 |
| `axum` | 健康检查 HTTP server |
| `rusqlite` | 模型选择持久化 |
| `tokio` | 异步运行时 |
| `thiserror` | 错误类型定义 |
