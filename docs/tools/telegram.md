# Telegram 工具

Loom Agent 通过 Telegram 工具直接与 Telegram Bot API 交互，支持发送文本消息、投票和文件。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 主动推送消息 | ✅ 完美支持 | `telegram_send_message` 发送文本通知 |
| 文件分发 | ✅ 完美支持 | `telegram_send_document` 发送任意文件 |
| 互动投票 | ✅ 完美支持 | `telegram_send_poll` 收集用户意见 |
| 消息格式化 | ✅ 原生支持 | MarkdownV2 / HTML 两种格式 |
| 多 Chat 管理 | ✅ 灵活指定 | 通过 `chat_id` 参数定向发送 |

## 架构概览

```
┌─────────────────────────────────────────────────┐
│  Agent (ReAct)                                  │
│    ↓ ToolCall                                   │
│  TelegramToolsSource (loom)                     │
│    ↓ AggregateToolSource                        │
│  ┌─────────────┐ ┌───────────┐ ┌──────────────┐│
│  │SendMsgTool  │ │SendPollTool│ │SendDocTool   ││
│  └──────┬──────┘ └─────┬─────┘ └──────┬───────┘│
│         └──────────────┼──────────────┘         │
│                   TelegramApi trait             │
└───────────────────────┬─────────────────────────┘
                        │
┌───────────────────────┼─────────────────────────┐
│  telegram-bot (runtime)                         │
│         TeloxideTelegramApi                     │
│         (teloxide Bot → Telegram Bot API)       │
└─────────────────────────────────────────────────┘
```

### 核心组件

- **`TelegramApi` trait** (`loom/src/tools/telegram/mod.rs:20-46`) — 定义发送消息、投票、文档的异步接口
- **工具实现** (`loom/src/tools/telegram/`) — 三个独立的 Tool 实现
- **`TelegramToolsSource`** (`loom/src/tool_source/telegram_tools_source.rs`) — 聚合工具注册为 ToolSource
- **`TeloxideTelegramApi`** (`telegram-bot/src/telegram_tools/mod.rs`) — 基于 teloxide 的具体实现

## 通用参数

所有 Telegram 工具共享以下可选参数，实现时应统一支持：

| 参数 | 类型 | 说明 |
|------|------|------|
| `chat_id` | integer | 目标 Chat ID，默认使用当前聊天上下文 |
| `reply_to_message_id` | integer | 回复指定消息（引用回复） |
| `disable_notification` | boolean | 静默发送，不触发通知 |

> **注意**：当前已实现的 3 个工具缺少 `reply_to_message_id` 和 `disable_notification`，应在后续迭代中补全。

## 可用工具

### telegram_send_message

发送文本消息到 Telegram 聊天。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `text` | string | ✅ | 消息文本内容 |
| `chat_id` | integer | ❌ | 目标 Chat ID，默认使用当前聊天上下文 |
| `parse_mode` | string | ❌ | 格式化模式：`"MarkdownV2"` 或 `"HTML"` |

调用示例（Agent 自动生成）：

```json
{
  "text": "任务已完成 ✅",
  "parse_mode": "MarkdownV2"
}
```

返回：`"Message sent successfully (message_id: 42)"`

### telegram_send_poll

发送投票到 Telegram 聊天。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `question` | string | ✅ | 投票问题 |
| `options` | string[] | ✅ | 选项列表（2-10 个） |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `is_anonymous` | boolean | ❌ | 是否匿名投票，默认 `true` |
| `allows_multiple_answers` | boolean | ❌ | 是否允许多选，默认 `false` |

调用示例：

```json
{
  "question": "下次会议时间？",
  "options": ["周一", "周三", "周五"],
  "is_anonymous": false
}
```

返回：`"Poll sent successfully (poll_id: 42)"`

### telegram_send_document

发送文件到 Telegram 聊天。当前仅支持本地文件路径（teloxide `InputFile::file`），不支持 URL。

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file_path` | string | ✅ | 本地文件路径 |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `caption` | string | ❌ | 文件说明文字 |

调用示例：

```json
{
  "file_path": "/tmp/report.pdf",
  "caption": "本月报告"
}
```

返回：`"Document sent successfully (message_id: 43)"`

## Chat ID 上下文管理

工具通过全局状态自动获取当前 Chat ID，无需每次手动指定：

```rust
// 每次消息到来时设置（telegram-bot 运行时自动处理）
loom::tools::telegram::set_current_chat_id(chat_id);

// Agent 调用工具时自动使用
// chat_id 参数未提供 → 使用 get_current_chat_id()
```

如需发送到*其他*聊天，显式传入 `chat_id` 即可覆盖。

## 初始化流程

在 `telegram-bot` 启动时完成：

```rust
// telegram-bot/src/telegram_tools/mod.rs:87-90
let bot = Bot::from_env();
loom::tools::telegram::set_telegram_api(Arc::new(TeloxideTelegramApi::new(bot)));
```

1. 创建 teloxide `Bot` 实例
2. 包装为 `TeloxideTelegramApi`
3. 通过 `set_telegram_api()` 注入全局单例
4. `TelegramToolsSource::new()` 注册工具到 Agent

## 与工具系统的集成

在 Agent 配置中启用 Telegram 工具：

```rust
use loom::tool_source::TelegramToolsSource;

let telegram_source = TelegramToolsSource::new().await;
// 加入到 AggregateToolSource 或直接用于 ReactBuildConfig
```

工具通过 `AggregateToolSource` 注册，与 Bash、Web 等内置工具并列，Agent 在 ReAct 循环中按需调用。

## 错误处理

| 错误场景 | 错误类型 | 说明 |
|----------|----------|------|
| API 未初始化 | `Transport` | `set_telegram_api()` 未调用 |
| 无 Chat ID | `InvalidInput` | 未提供 `chat_id` 且无当前上下文 |
| 参数无效 | `InvalidInput` | JSON 反序列化失败 |
| API 调用失败 | `Transport` | Telegram 返回错误（权限、限流等） |
| 投票选项不足 | `InvalidInput` | 少于 2 个或超过 10 个选项 |

## 当前覆盖范围

### 已实现

| Bot API 方法 | 工具名 | 状态 | 缺失参数 |
|-------------|--------|------|----------|
| `sendMessage` | `telegram_send_message` | ✅ 基本完整 | `reply_to_message_id`、`disable_notification` |
| `sendPoll` | `telegram_send_poll` | ✅ 基本完整 | `reply_to_message_id`、`disable_notification` |
| `sendDocument` | `telegram_send_document` | ⚠️ 仅本地路径 | `reply_to_message_id`、`disable_notification`、URL 支持 |

### 未实现 — 完整规划

#### 媒体发送类

所有媒体工具参数保持精简：核心文件路径 + caption + parse_mode。teloxide 会自动推断宽高、时长等元数据，无需 Agent 手动传入。

| Bot API 方法 | 计划工具名 | 说明 | 优先级 |
|-------------|-----------|------|--------|
| `sendPhoto` | `telegram_send_photo` | 发送图片 | 高 |
| `sendVideo` | `telegram_send_video` | 发送视频 | 高 |
| `sendAudio` | `telegram_send_audio` | 发送音频 | 中 |
| `sendAnimation` | `telegram_send_animation` | 发送 GIF 动画 | 中 |
| `sendVoice` | `telegram_send_voice` | 发送语音消息（OGG 格式） | 中 |
| `sendMediaGroup` | `telegram_send_media_group` | 批量发送媒体（相册/文档组） | 中 |
| `sendSticker` | `telegram_send_sticker` | 发送贴纸 | 低 |
| `sendVideoNote` | `telegram_send_video_note` | 发送视频笔记（圆形视频） | 低 |

**`telegram_send_photo` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `photo` | string | ✅ | 图片文件路径或 URL |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `caption` | string | ❌ | 图片说明（0-1024 字符） |
| `parse_mode` | string | ❌ | caption 格式：`"MarkdownV2"` 或 `"HTML"` |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |
| `has_spoiler` | boolean | ❌ | 是否标记为剧透内容 |

**`telegram_send_video` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `video` | string | ✅ | 视频文件路径或 URL |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `caption` | string | ❌ | 视频说明 |
| `parse_mode` | string | ❌ | caption 格式 |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |

> 宽高、时长、缩略图、`supports_streaming` 等参数由 teloxide 自动从文件推断，不暴露给 Agent。

**`telegram_send_audio` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `audio` | string | ✅ | 音频文件路径或 URL |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `caption` | string | ❌ | 音频说明 |
| `parse_mode` | string | ❌ | caption 格式 |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |

> `performer`、`title`、`duration` 由 teloxide 从文件元数据自动提取。

**`telegram_send_animation` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `animation` | string | ✅ | GIF 文件路径或 URL |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `caption` | string | ❌ | 动画说明 |
| `parse_mode` | string | ❌ | caption 格式 |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |
| `has_spoiler` | boolean | ❌ | 是否标记为剧透内容 |

**`telegram_send_voice` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `voice` | string | ✅ | 语音文件路径或 URL（OGG 格式） |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `caption` | string | ❌ | 语音说明 |
| `parse_mode` | string | ❌ | caption 格式 |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |

#### 消息操作类

| Bot API 方法 | 计划工具名 | 说明 | 优先级 |
|-------------|-----------|------|--------|
| `editMessageText` | `telegram_edit_message` | 编辑已发送的文本消息 | 高 |
| `deleteMessage` | `telegram_delete_message` | 删除消息 | 高 |
| `forwardMessage` | `telegram_forward_message` | 转发消息到其他聊天 | 中 |
| `copyMessage` | `telegram_copy_message` | 复制消息（不显示原始来源） | 低 |
| `pinChatMessage` | `telegram_pin_message` | 置顶消息 | 低 |
| `unpinChatMessage` | `telegram_unpin_message` | 取消置顶消息 | 低 |

**`telegram_edit_message` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message_id` | integer | ✅ | 要编辑的消息 ID |
| `text` | string | ✅ | 新的文本内容 |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `parse_mode` | string | ❌ | 格式化模式 |

**`telegram_delete_message` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message_id` | integer | ✅ | 要删除的消息 ID |
| `chat_id` | integer | ❌ | 目标 Chat ID |

**`telegram_forward_message` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `from_chat_id` | integer | ✅ | 来源 Chat ID |
| `message_id` | integer | ✅ | 要转发的消息 ID |
| `chat_id` | integer | ❌ | 目标 Chat ID |

#### 位置与联系人

| Bot API 方法 | 计划工具名 | 说明 | 优先级 |
|-------------|-----------|------|--------|
| `sendLocation` | `telegram_send_location` | 发送地理位置 | 中 |
| `sendVenue` | `telegram_send_venue` | 发送场所信息（地点+地址） | 低 |
| `sendContact` | `telegram_send_contact` | 发送联系人名片 | 低 |

**`telegram_send_location` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `latitude` | number | ✅ | 纬度 |
| `longitude` | number | ✅ | 经度 |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |

> `horizontal_accuracy`、`live_period`、`heading` 等参数使用场景极窄，暂不暴露。

**`telegram_send_venue` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `latitude` | number | ✅ | 纬度 |
| `longitude` | number | ✅ | 经度 |
| `title` | string | ✅ | 场所名称 |
| `address` | string | ✅ | 场所地址 |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |

**`telegram_send_contact` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `phone_number` | string | ✅ | 电话号码 |
| `first_name` | string | ✅ | 名 |
| `last_name` | string | ❌ | 姓 |
| `chat_id` | integer | ❌ | 目标 Chat ID |
| `reply_to_message_id` | integer | ❌ | 回复指定消息 |
| `disable_notification` | boolean | ❌ | 静默发送 |

#### Inline Keyboard 交互（中优先级）

> **依赖链**：当前 Bot 侧没有 callback 监听机制。需要先在 `telegram-bot` 中实现 callback router，再添加工具侧支持。整体工作量较大，优先级从中开始。

| Bot API 方法 | 计划工具名 | 说明 | 优先级 |
|-------------|-----------|------|--------|
| `answerCallbackQuery` | `telegram_answer_callback` | 响应内联按钮回调 | 中 |
| `editMessageReplyMarkup` | `telegram_edit_keyboard` | 编辑消息的键盘布局 | 中 |
| `sendGame` | `telegram_send_game` | 发送游戏 | 低 |
| `setGameScore` | `telegram_set_game_score` | 设置游戏分数 | 低 |

**前置依赖**（需先在 `telegram-bot` 中实现）：
1. 在 `send_message` 等工具中增加 `reply_markup` 参数（InlineKeyboardMarkup JSON）
2. 在 `telegram-bot` 的 teloxide dispatcher 中注册 callback handler
3. 将 callback 事件路由给 Agent 处理

**`telegram_answer_callback` 参数规划：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `callback_query_id` | string | ✅ | 回调查询 ID |
| `text` | string | ❌ | 提示文本（最多 200 字符） |
| `show_alert` | boolean | ❌ | 是否弹窗而非 toast |

#### 查询类

| Bot API 方法 | 计划工具名 | 说明 | 优先级 |
|-------------|-----------|------|--------|
| `getChat` | `telegram_get_chat` | 获取聊天信息 | 中 |
| `getChatMemberCount` | `telegram_get_member_count` | 获取群成员数量 | 低 |

### 不在规划内

以下 API 方法不适合由 LLM Agent 自主调用，涉及权限管控或安全风险：

- **聊天管理**：`banChatMember`、`unbanChatMember`、`promoteChatMember`、`restrictChatMember`、`setChatTitle`、`setChatDescription`、`setChatPermissions` — 封禁/提权等操作不应交给 LLM 自主决定
- **用户查询**：`getUserProfilePhotos`、`getChatAdministrators`、`getChatMember` — 隐私敏感
- **Webhook 模式**：当前使用长轮询，无需 Webhook

如需群管功能，建议通过 MCP 接入专门的群管 Bot，而非通过 Agent 工具。

### 优先级汇总

**高优先级**（核心交互必需）：
- `sendPhoto`、`sendVideo` — 媒体是高频需求
- `editMessage`、`deleteMessage` — 消息管理是基本操作

**中优先级**（增强体验）：
- `sendAudio`、`sendAnimation`、`sendVoice` — 补全媒体类型
- `forwardMessage`、`sendLocation` — 常用功能
- Inline Keyboard 全套（callback + keyboard） — 交互式按钮
- `sendMediaGroup` — 批量发送
- `getChat` — 信息查询

**低优先级**（扩展）：
- 联系人/场所/贴纸/游戏
- 消息复制/置顶
- 成员数量查询

## 开发方案

按阶段推进，每个阶段有明确的交付物和验证标准。

### 阶段 0：通用参数补全

为已实现的 3 个工具补全 `reply_to_message_id` 和 `disable_notification`，同时为后续所有新工具建立统一模式。

**涉及文件（4 个）：**

| 文件 | 修改内容 |
|------|----------|
| `loom/src/tools/telegram/mod.rs:20-46` | `TelegramApi` trait 方法签名增加 `reply_to_message_id: Option<i32>` 和 `disable_notification: Option<bool>` 参数 |
| `loom/src/tools/telegram/send_message.rs` | `SendMessageParams` 增加两个可选字段，`call()` 传给 api |
| `loom/src/tools/telegram/send_poll.rs` | `SendPollParams` 增加两个可选字段，`call()` 传给 api |
| `loom/src/tools/telegram/send_document.rs` | `SendDocumentParams` 增加两个可选字段，`call()` 传给 api |
| `telegram-bot/src/telegram_tools/mod.rs` | `TeloxideTelegramApi` 的 3 个实现方法中使用 teloxide 的 `.reply_to()` 和 `.disable_notification()` |

**trait 签名变更：**

```rust
// loom/src/tools/telegram/mod.rs — 修改后
#[async_trait]
pub trait TelegramApi: Send + Sync {
    async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        reply_to_message_id: Option<i32>,
        disable_notification: Option<bool>,
    ) -> Result<i32, String>;

    async fn send_poll(
        &self,
        chat_id: i64,
        question: &str,
        options: Vec<String>,
        is_anonymous: bool,
        allows_multiple_answers: bool,
        reply_to_message_id: Option<i32>,
        disable_notification: Option<bool>,
    ) -> Result<i32, String>;

    async fn send_document(
        &self,
        chat_id: i64,
        file_path: &str,
        caption: Option<&str>,
        reply_to_message_id: Option<i32>,
        disable_notification: Option<bool>,
    ) -> Result<i32, String>;
}
```

**teloxide 实现侧变更示例（send_message）：**

```rust
// telegram-bot/src/telegram_tools/mod.rs
async fn send_message(
    &self,
    chat_id: i64,
    text: &str,
    parse_mode: Option<&str>,
    reply_to_message_id: Option<i32>,
    disable_notification: Option<bool>,
) -> Result<i32, String> {
    let mut request = self.bot.send_message(ChatId(chat_id), text);

    if let Some(mode) = parse_mode {
        request = match mode {
            "MarkdownV2" => request.parse_mode(ParseMode::MarkdownV2),
            "HTML" => request.parse_mode(ParseMode::Html),
            _ => request,
        };
    }
    if let Some(id) = reply_to_message_id {
        request = request.reply_to_message_id(id);
    }
    if let Some(true) = disable_notification {
        request = request.disable_notification(true);
    }

    let message = request.await
        .map_err(|e| format!("Telegram API error: {}", e))?;
    Ok(message.id.0)
}
```

**验证**：编译通过 + 手动测试 `reply_to` 和静默发送。

---

### 阶段 1：媒体发送（高优先级）

实现 `sendPhoto` 和 `sendVideo`。

**新增文件（4 个）：**

| 文件 | 说明 |
|------|------|
| `loom/src/tools/telegram/send_photo.rs` | `TelegramSendPhotoTool` 实现 |
| `loom/src/tools/telegram/send_video.rs` | `TelegramSendVideoTool` 实现 |
| `loom/src/tools/telegram/mod.rs` | 增加 mod 声明 + pub use + trait 新方法 |
| `telegram-bot/src/telegram_tools/mod.rs` | TeloxideTelegramApi 实现新方法 |

**步骤：**

1. 在 `TelegramApi` trait 中新增两个方法：

```rust
// loom/src/tools/telegram/mod.rs — 新增
async fn send_photo(
    &self,
    chat_id: i64,
    photo: &str,
    caption: Option<&str>,
    parse_mode: Option<&str>,
    has_spoiler: Option<bool>,
    reply_to_message_id: Option<i32>,
    disable_notification: Option<bool>,
) -> Result<i32, String>;

async fn send_video(
    &self,
    chat_id: i64,
    video: &str,
    caption: Option<&str>,
    parse_mode: Option<&str>,
    reply_to_message_id: Option<i32>,
    disable_notification: Option<bool>,
) -> Result<i32, String>;
```

2. 创建 `loom/src/tools/telegram/send_photo.rs`：

```rust
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tool_source::{ToolCallContent, ToolCallContext, ToolSourceError, ToolSpec};
use crate::tools::Tool;
use super::{get_current_chat_id, get_telegram_api};

pub const TOOL_TELEGRAM_SEND_PHOTO: &str = "telegram_send_photo";

#[derive(Debug, Deserialize)]
pub struct SendPhotoParams {
    pub chat_id: Option<i64>,
    pub photo: String,
    pub caption: Option<String>,
    pub parse_mode: Option<String>,
    pub has_spoiler: Option<bool>,
    pub reply_to_message_id: Option<i32>,
    pub disable_notification: Option<bool>,
}

pub struct TelegramSendPhotoTool;

#[async_trait]
impl Tool for TelegramSendPhotoTool {
    fn name(&self) -> &str { TOOL_TELEGRAM_SEND_PHOTO }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TOOL_TELEGRAM_SEND_PHOTO.to_string(),
            description: Some("Send a photo to a Telegram chat. Supports local file paths and URLs.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "photo": {
                        "type": "string",
                        "description": "Photo file path or URL"
                    },
                    "chat_id": {
                        "type": "integer",
                        "description": "Target chat ID (optional, defaults to current chat)"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Photo caption (0-1024 characters)"
                    },
                    "parse_mode": {
                        "type": "string",
                        "enum": ["MarkdownV2", "HTML"],
                        "description": "Caption parse mode (optional)"
                    },
                    "has_spoiler": {
                        "type": "boolean",
                        "description": "Mark as spoiler content (optional)"
                    },
                    "reply_to_message_id": {
                        "type": "integer",
                        "description": "Reply to a specific message (optional)"
                    },
                    "disable_notification": {
                        "type": "boolean",
                        "description": "Send silently without notification (optional)"
                    }
                },
                "required": ["photo"]
            }),
            output_hint: None,
        }
    }

    async fn call(&self, args: Value, _ctx: Option<&ToolCallContext>)
        -> Result<ToolCallContent, ToolSourceError>
    {
        let params: SendPhotoParams = serde_json::from_value(args)
            .map_err(|e| ToolSourceError::InvalidInput(format!("Invalid arguments: {}", e)))?;

        let api = get_telegram_api().ok_or_else(|| {
            ToolSourceError::Transport("Telegram API not initialized".to_string())
        })?;

        let chat_id = params.chat_id
            .unwrap_or_else(|| get_current_chat_id().unwrap_or(0));
        if chat_id == 0 {
            return Err(ToolSourceError::InvalidInput(
                "No chat_id provided and no current chat context".to_string(),
            ));
        }

        let message_id = api.send_photo(
            chat_id,
            &params.photo,
            params.caption.as_deref(),
            params.parse_mode.as_deref(),
            params.has_spoiler,
            params.reply_to_message_id,
            params.disable_notification,
        ).await
            .map_err(|e| ToolSourceError::Transport(format!("Failed to send photo: {}", e)))?;

        Ok(ToolCallContent::Text(format!(
            "Photo sent successfully (message_id: {})", message_id
        )))
    }
}
```

3. 创建 `loom/src/tools/telegram/send_video.rs`（结构同上，字段名改为 `video`，去掉 `has_spoiler`）。

4. teloxide 实现侧 — `telegram-bot/src/telegram_tools/mod.rs`：

```rust
async fn send_photo(
    &self,
    chat_id: i64,
    photo: &str,
    caption: Option<&str>,
    parse_mode: Option<&str>,
    has_spoiler: Option<bool>,
    reply_to_message_id: Option<i32>,
    disable_notification: Option<bool>,
) -> Result<i32, String> {
    let input = if photo.starts_with("http://") || photo.starts_with("https://") {
        InputFile::url(photo.parse().unwrap())
    } else {
        InputFile::file(photo)
    };
    let mut request = self.bot.send_photo(ChatId(chat_id), input);

    if let Some(cap) = caption { request = request.caption(cap); }
    if let Some(mode) = parse_mode {
        request = match mode {
            "MarkdownV2" => request.parse_mode(ParseMode::MarkdownV2),
            "HTML" => request.parse_mode(ParseMode::Html),
            _ => request,
        };
    }
    if let Some(true) = has_spoiler { request = request.has_spoiler(true); }
    if let Some(id) = reply_to_message_id { request = request.reply_to_message_id(id); }
    if let Some(true) = disable_notification { request = request.disable_notification(true); }

    let message = request.await
        .map_err(|e| format!("Telegram API error: {}", e))?;
    Ok(message.id.0)
}
```

5. 在 `loom/src/tools/telegram/mod.rs` 注册模块导出：

```rust
mod send_photo;
mod send_video;

pub use send_photo::{TelegramSendPhotoTool, TOOL_TELEGRAM_SEND_PHOTO};
pub use send_video::{TelegramSendVideoTool, TOOL_TELEGRAM_SEND_VIDEO};
```

6. 在 `loom/src/tool_source/telegram_tools_source.rs` 注册：

```rust
source.register_async(Box::new(TelegramSendPhotoTool)).await;
source.register_async(Box::new(TelegramSendVideoTool)).await;
```

**验证**：`cargo build` + 发送本地图片和 URL 图片测试。

---

### 阶段 2：消息操作（高优先级）

实现 `editMessageText` 和 `deleteMessage`。

**新增文件：**

| 文件 | 说明 |
|------|------|
| `loom/src/tools/telegram/edit_message.rs` | `TelegramEditMessageTool` |
| `loom/src/tools/telegram/delete_message.rs` | `TelegramDeleteMessageTool` |

**trait 新增：**

```rust
async fn edit_message_text(
    &self,
    chat_id: i64,
    message_id: i32,
    text: &str,
    parse_mode: Option<&str>,
) -> Result<i32, String>;

async fn delete_message(
    &self,
    chat_id: i64,
    message_id: i32,
) -> Result<(), String>;
```

**teloxide 实现要点：**
- `edit_message_text`：`self.bot.edit_message_text(ChatId(chat_id), message_id, text)`
- `delete_message`：`self.bot.delete_message(ChatId(chat_id), message_id)`
- `delete_message` 返回 `Result<()>` 而非 `Result<i32>`，Tool 侧返回 `"Message deleted successfully"`

**特殊考虑**：当前工具的 `send_message` 返回值中包含 `message_id`，Agent 可通过此 ID 后续调用 `edit_message` 或 `delete_message`。这是已有的设计优势。

**验证**：发送消息 → 编辑内容 → 删除消息，完整链路测试。

---

### 阶段 3：补全媒体 + 消息转发（中优先级）

按相同模式批量实现，每个工具约 80-100 行。

| 工具 | 新建文件 | teloxide 方法 |
|------|----------|--------------|
| `send_audio` | `send_audio.rs` | `bot.send_audio()` |
| `send_animation` | `send_animation.rs` | `bot.send_animation()` |
| `send_voice` | `send_voice.rs` | `bot.send_voice()` |
| `forward_message` | `forward_message.rs` | `bot.forward_message()` |
| `send_location` | `send_location.rs` | `bot.send_location()` |

**输入源统一处理**：所有媒体工具都需要处理本地路径 vs URL 的判断，建议提取公共函数：

```rust
// loom/src/tools/telegram/mod.rs — 新增
pub fn resolve_input_file(path: &str) -> InputFile {
    if path.starts_with("http://") || path.starts_with("https://") {
        InputFile::url(path.parse().expect("Invalid URL"))
    } else {
        InputFile::file(path)
    }
}
```

> 注意：`InputFile` 类型来自 teloxide，不应出现在 loom trait 层。trait 层仍传 `&str`，teloxide 实现侧负责转换。

**验证**：每种媒体类型各发一条消息。

---

### 阶段 4：Inline Keyboard 交互（中优先级）

这是最复杂的阶段，需要跨 crate 改动。

**前置条件**：阶段 2 完成（需要 `edit_message` 来更新键盘）。

**涉及改动：**

| 组件 | 文件 | 改动 |
|------|------|------|
| trait 扩展 | `loom/src/tools/telegram/mod.rs` | `send_message` 签名增加 `reply_markup: Option<&str>` |
| 已有工具 | `send_message.rs` | 参数增加 `reply_markup`，透传给 trait |
| 新工具 | `answer_callback.rs` | `telegram_answer_callback` |
| 新工具 | `edit_keyboard.rs` | `telegram_edit_keyboard` |
| Bot dispatcher | `telegram-bot/src/router.rs` | 注册 callback query handler |
| Bot 新模块 | `telegram-bot/src/callback_handler.rs` | 处理 callback 事件 |

**reply_markup 参数设计：**

```json
{
  "text": "请选择",
  "reply_markup": {
    "inline_keyboard": [
      [
        {"text": "选项A", "callback_data": "choice_a"},
        {"text": "选项B", "callback_data": "choice_b"}
      ]
    ]
  }
}
```

参数类型为 JSON string，teloxide 侧反序列化为 `InlineKeyboardMarkup`。

**callback 路由设计：**

```
用户点击按钮
  → teloxide callback handler
    → 解析 callback_data
      → 作为用户消息注入 Agent 会话
        → Agent 调用 telegram_answer_callback 应答
```

**验证**：发送带按钮消息 → 点击按钮 → Agent 收到回调 → 应答。

---

### 阶段 5：其余工具（低优先级）

按需从以下列表中选择实现，每个工具均遵循阶段 1 建立的模式：

- `send_contact`、`send_venue` — 参数简单，无文件处理
- `pin_message`、`unpin_message` — 仅需 chat_id + message_id
- `copy_message` — 与 `forward_message` 类似
- `send_media_group` — 需要数组参数，复杂度略高
- `get_chat`、`get_member_count` — 只读查询
- `send_sticker`、`send_video_note` — 使用频率低

---

### 开发顺序总览

```
阶段 0: 通用参数补全 ──────── 1-2 天
  ↓
阶段 1: sendPhoto + sendVideo ── 1-2 天
  ↓
阶段 2: editMessage + deleteMessage ── 1 天
  ↓
阶段 3: 补全媒体 + forward + location ── 2-3 天
  ↓
阶段 4: Inline Keyboard ──────── 3-5 天
  ↓
阶段 5: 按需实现 ──────────── 按需
```

每个阶段完成后：
1. `cargo build` 编译通过
2. `cargo test` 所有测试通过
3. 手动端到端测试（通过 Bot 实际发送消息）
4. 更新本文档的"已实现"表格

## 扩展指南

添加新工具遵循以下步骤：

1. **在 `loom/src/tools/telegram/` 下新建文件**，实现 `Tool` trait
2. **在 `TelegramApi` trait 中添加方法签名**（`mod.rs`）
3. **在 `telegram-bot/src/telegram_tools/mod.rs` 中实现** teloxide 调用
4. **在 `TelegramToolsSource::new()` 中注册**新工具

示例骨架：

```rust
// loom/src/tools/telegram/send_photo.rs
pub struct TelegramSendPhotoTool;

#[async_trait]
impl Tool for TelegramSendPhotoTool {
    fn name(&self) -> &str { "telegram_send_photo" }
    fn spec(&self) -> ToolSpec { /* ... */ }
    async fn call(&self, args: Value, _ctx: Option<&ToolCallContext>)
        -> Result<ToolCallContent, ToolSourceError> {
        // 1. 解析参数
        // 2. 获取 api = get_telegram_api()
        // 3. 获取 chat_id（参数 > 上下文）
        // 4. api.send_photo(...)
    }
}
```

---

**相关文档**：[工具系统](./tool-system.md) | [MCP 协议](./mcp.md)
**Bot 侧文档**：[Telegram Bot 概览](../../telegram-bot/docs/overview.md) | [Bot API 参考](../../telegram-bot/docs/reference/bot-api.md)
