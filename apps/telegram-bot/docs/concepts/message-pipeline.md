# 消息处理管线

从 Telegram 消息到 Agent 回复的完整处理流程。

## 什么时候用

- 你需要理解消息在系统中的流转路径
- 你要排查消息丢失、未响应等问题
- 你要扩展消息处理逻辑（如添加新的过滤规则）

## 核心概念

### 处理流程

每条消息按以下顺序处理：

```
Telegram API
    ↓ (teloxide long polling)
Router (router.rs)
    ↓ handle_message_with_deps()
Pipeline (pipeline/mod.rs)
    ├─ 1. 媒体下载（图片/视频/文档）→ 保存到 download_dir
    ├─ 2. 斜杠命令检查 → CommandDispatcher
    ├─ 3. 群聊 Mention 过滤 → 仅 @bot 或回复 Bot 时响应
    └─ 4. Agent 执行 → agent_orchestrator → streaming pipeline
```

### 阶段详解

**1. Router**（`router.rs`）

入口函数 `handle_message_with_deps()` 是所有消息的统一入口。它递增指标计数后，将消息委托给 Pipeline。

**2. 媒体下载**（`download.rs`）

`TeloxideDownloader` 实现 `FileDownloader` trait，处理：
- 图片（`PhotoSize`）— 下载最大分辨率
- 视频（`Video`）— 下载视频文件
- 文档（`Document`）— 下载附件

下载的文件保存到 `settings.download_dir`，返回 `FileMetadata`（路径、大小、MIME 类型）。

**3. 命令分发**

检查文本是否以 `/` 开头。如果是，尝试匹配 `CommandDispatcher` 中注册的命令。匹配成功则执行命令并返回，不进入后续阶段。

**4. Mention 过滤**（仅群聊）

在群组中，Bot 只响应：
- 直接 @提及 Bot 的消息
- 回复 Bot 已有消息的消息

私聊中此过滤被跳过。

**5. Agent 执行**（`pipeline/agent_orchestrator.rs`）

`run_agent_for_chat()` 是最终的 Agent 调用入口：
- 通过 `ChatRunRegistry.try_acquire()` 防止并发
- 构造 `AgentRunContext`（含 chat_id、model、文件信息）
- 调用 `AgentRunner.run()` 进入流式响应管线

### 错误处理

每个阶段可能返回 `BotError`：

- `BotError::Config` — 配置错误
- `BotError::Network` — Telegram API 网络错误
- `BotError::Io` — 文件下载/读写错误
- `BotError::Agent` — Agent 执行错误
- `BotError::RateLimit` — API 频率限制

错误会被格式化后发送给用户（不暴露内部细节）。

## 最佳实践

✅ 扩展处理逻辑时，在 Pipeline 中添加新的处理阶段，不要修改 Router
✅ 文件下载路径使用配置的 `download_dir`，不要硬编码路径
⚠️ 修改处理顺序时注意：命令分发必须在 Mention 过滤之前，否则群聊中的命令可能被忽略

## 本页覆盖范围

- 覆盖：完整消息处理流程、各阶段职责、错误处理
- 不覆盖：流式响应的内部机制（见 [流式 Agent 响应](streaming-agent.md)）、命令实现（见 [斜杠命令系统](slash-commands.md)）

## 下一步

- [流式 Agent 响应](streaming-agent.md) — Agent 执行后的流式输出
- [Bot API 参考](../reference/bot-api.md) — 核心 trait 定义
