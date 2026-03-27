# Telegram Bot 支持 Telegram Markdown 方案

## 背景

当前 `loom-telegram/telegram-bot` 已经具备消息发送抽象层，但**还没有形成完整的 Telegram Markdown/MarkdownV2 支持方案**。

从现有代码看：

- `telegram-bot/src/traits.rs` 的 `MessageSender` 已支持 `send_text_with_parse_mode(..., ParseMode)`
- `telegram-bot/src/sender.rs` 已可通过 teloxide 发送 `ParseMode`
- 当前主流程大多仍以**纯文本**发送/编辑消息
- streaming 场景下大量内容来自：
  - 用户输入
  - LLM 输出
  - tool result
  - 文件路径 / shell 输出

这意味着：

1. **基础能力已具备**（teloxide + parse_mode）
2. **缺少统一格式化层**
3. **缺少对 Telegram MarkdownV2 风险的治理**

因此，需要一个面向 bot 的完整支持方案，而不是在业务代码里零散地直接设置 `ParseMode::MarkdownV2`。

---

## 目标

为 `telegram-bot` 增加一套**可控、安全、渐进式**的 Telegram Markdown 支持能力，满足：

1. 支持发送 Telegram 可解析的富文本消息
2. 避免 MarkdownV2 转义错误导致发送失败
3. 避免把 LLM/tool 原始输出直接当 markdown 发送
4. 兼容 streaming / non-streaming 两类交互模式
5. 后续可以扩展到 HTML 模式与自动降级机制

---

## 非目标

本方案**不追求**：

1. 完整支持“通用 Markdown 语法”到 Telegram MarkdownV2 的无损转换
2. 在第一阶段支持所有 Telegram 富文本能力（如嵌套复杂结构）
3. 在 streaming 中对任意 chunk 实时进行复杂 markdown 语法增量修复

换句话说：**不是做一个通用 Markdown 渲染器**，而是做一个适用于 bot 场景的“Telegram 格式化输出层”。

---

## 现状分析

### 1. 已有能力

#### 1.1 MessageSender 已支持 ParseMode

`telegram-bot/src/traits.rs`

```rust
async fn send_text_with_parse_mode(
    &self,
    chat_id: i64,
    text: &str,
    parse_mode: ParseMode,
) -> Result<(), BotError>;
```

说明发送侧抽象已经预留了格式化消息入口。

#### 1.2 TeloxideSender 已实现 parse_mode 发送

`telegram-bot/src/sender.rs`

```rust
self.bot
    .send_message(ChatId(chat_id), text)
    .parse_mode(parse_mode)
    .await
```

说明基础设施已具备。

#### 1.3 当前 edit_message 仍是纯文本路径

当前 `edit_message(...)` 只走普通编辑接口，没有 parse mode 参数。也就是说：

- 新消息可以支持 parse mode
- 编辑消息暂时没有统一格式模式

这对于 streaming 很关键，因为 streaming 主要依赖 `edit_message_text`。

---

### 2. 当前主要风险

#### 2.1 误把普通 Markdown 当 Telegram MarkdownV2

常见 Markdown：

- `**bold**`
- ````` fenced code ````` 
- `[link](url)`

并不等于 Telegram MarkdownV2 的可安全输入。

#### 2.2 用户/LLM/tool 输出包含保留字符

MarkdownV2 需要严格转义的字符包括：

```text
_ * [ ] ( ) ~ ` > # + - = | { } . !
```

例如：

- 文件路径：`foo/bar_baz.rs`
- shell 输出：`Error: a_b_c`
- JSON：`{"a": 1}`
- tool result 中的 `[]()-.!`

都可能导致 Telegram 解析失败。

#### 2.3 streaming 不适合直接做复杂 MarkdownV2

流式文本是 chunk-by-chunk 到达的：

- 前一个 chunk 可能有 `*`
- 下一个 chunk 才补全闭合
- 中途编辑时 Telegram 解析可能失败

所以 streaming 过程直接使用 MarkdownV2 风险非常高。

#### 2.4 缺少失败降级策略

即使 parse mode 发送失败，当前没有统一策略：

- 记录日志
- 自动回退为纯文本
- 或切换 HTML

---

## 设计原则

1. **默认安全优先**：默认纯文本，而不是默认 MarkdownV2
2. **模板与动态内容分离**：固定模板可格式化，动态内容默认转义/包裹 code
3. **streaming 保守，final 输出增强**
4. **统一格式化入口**：业务层不直接拼 MarkdownV2
5. **失败可降级**：格式发送失败后自动回退纯文本

---

## 推荐总体方案

采用三层设计：

```text
业务层
  ↓
Telegram 格式化层（新增）
  ↓
MessageSender / TeloxideSender
  ↓
Telegram API
```

新增模块建议：

```text
telegram-bot/src/formatting/
  mod.rs
  telegram.rs
  markdown.rs
  html.rs
```

其中第一阶段最少可以只做：

```text
telegram-bot/src/formatting/telegram.rs
```

---

## 核心设计

## 1. 新增格式模型

建议定义统一的数据结构，而不是让业务层直接传 `String + ParseMode`。

```rust
use teloxide::types::ParseMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramFormatMode {
    PlainText,
    MarkdownV2,
    Html,
}

#[derive(Debug, Clone)]
pub struct FormattedMessage {
    pub text: String,
    pub parse_mode: Option<ParseMode>,
}
```

进一步可定义一个高层消息片段模型：

```rust
pub enum MessageFragment {
    Text(String),
    Bold(String),
    Italic(String),
    InlineCode(String),
    CodeBlock(String),
    Link { text: String, url: String },
    LineBreak,
}
```

这样业务层只表达“意图”，由格式化层决定输出 MarkdownV2 / HTML / 纯文本。

---

## 2. 第一阶段只支持“受控子集”

### 支持的格式能力

第一阶段建议仅支持：

- 普通文本
- 粗体
- 行内代码
- 代码块
- 链接
- 换行

### 不建议第一阶段支持

- 任意通用 Markdown 输入
- 嵌套复杂列表
- streaming 中的增量 markdown 自动修复
- 表格
- 引用块完整兼容

原因：可控范围小，风险低，容易测试。

---

## 3. streaming 与 final 输出分离策略

这是本方案最重要的点。

### 3.1 streaming 阶段

**建议仍使用纯文本**。

原因：

- chunk 边界不稳定
- MarkdownV2 需要整体结构完整
- 编辑消息失败会严重影响体验

适用范围：

- Think phase
- Act phase
- Tool output streaming

### 3.2 final 输出 / 非 streaming 输出

支持使用 Telegram MarkdownV2 或 HTML。

适用范围：

- `/status`、`/help`、`/reset` 这类固定命令回复
- 启动通知、部署通知、错误摘要
- 最终总结消息
- 非 streaming 的 agent reply

### 3.3 推荐默认策略

| 场景 | 推荐格式 |
|---|---|
| streaming edit | 纯文本 |
| 命令类固定回复 | MarkdownV2 或 HTML |
| LLM 最终总结 | 优先 HTML / 保守 MarkdownV2 |
| tool result | 纯文本或代码块 |
| 用户输入回显 | 纯文本或 escape 后 inline code |

---

## 4. 新增统一格式化函数

建议提供以下 API：

### 4.1 纯文本到 MarkdownV2 安全转义

```rust
pub fn escape_markdown_v2(text: &str) -> String
```

职责：

- 仅做 Telegram MarkdownV2 保留字符转义
- 不做结构推断

### 4.2 纯文本到 HTML 转义

```rust
pub fn escape_html(text: &str) -> String
```

职责：转义 `& < >`。

### 4.3 片段渲染

```rust
pub fn render_markdown_v2(fragments: &[MessageFragment]) -> FormattedMessage
pub fn render_html(fragments: &[MessageFragment]) -> FormattedMessage
pub fn render_plain_text(fragments: &[MessageFragment]) -> FormattedMessage
```

### 4.4 面向业务的辅助函数

```rust
pub fn success_message(title: &str, body: &str) -> FormattedMessage
pub fn error_message(title: &str, body: &str) -> FormattedMessage
pub fn code_message(title: &str, code: &str) -> FormattedMessage
```

这样业务代码不会自己拼 markdown。

---

## 5. 配置设计

建议在 `telegram-bot/src/config/telegram.rs` 中增加格式相关配置：

```toml
[settings.formatting]
default_mode = "plain_text"   # plain_text | markdown_v2 | html
streaming_mode = "plain_text"
final_response_mode = "html"
command_response_mode = "markdown_v2"
auto_fallback_to_plain_text = true
```

对应 Rust 结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramMessageFormat {
    PlainText,
    MarkdownV2,
    Html,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormattingConfig {
    pub default_mode: TelegramMessageFormat,
    pub streaming_mode: TelegramMessageFormat,
    pub final_response_mode: TelegramMessageFormat,
    pub command_response_mode: TelegramMessageFormat,
    pub auto_fallback_to_plain_text: bool,
}
```

默认值建议：

- `default_mode = plain_text`
- `streaming_mode = plain_text`
- `final_response_mode = html`
- `command_response_mode = markdown_v2`
- `auto_fallback_to_plain_text = true`

说明：虽然用户提的是“支持 telegram markdown”，但从工程角度看，**HTML 更适合承载最终富文本**。

---

## 6. Sender 接口改造建议

当前只有：

- `send_text(...)`
- `send_text_with_parse_mode(...)`
- `edit_message(...)`

建议补齐：

```rust
async fn send_formatted(
    &self,
    chat_id: i64,
    msg: &FormattedMessage,
) -> Result<(), BotError>;

async fn edit_message_with_parse_mode(
    &self,
    chat_id: i64,
    message_id: i32,
    text: &str,
    parse_mode: ParseMode,
) -> Result<(), BotError>;

async fn edit_formatted(
    &self,
    chat_id: i64,
    message_id: i32,
    msg: &FormattedMessage,
) -> Result<(), BotError>;
```

这样才能为未来 final-edit 或非 streaming 富文本编辑打基础。

---

## 7. 失败回退策略

新增统一策略：

### 7.1 send/edit with parse mode 失败时

如果开启：

```toml
auto_fallback_to_plain_text = true
```

则行为：

1. 记录 warning 日志
2. 去掉 parse mode
3. 发送纯文本版本

例如：

```rust
tracing::warn!(error = %e, "formatted telegram message failed, fallback to plain text");
```

### 7.2 回退方式

- MarkdownV2 → 纯文本：使用未格式化版本，或剥离 fragments
- HTML → 纯文本：去标签 / 使用源文本

这要求 `FormattedMessage` 最好保留一个 `plain_text_fallback` 字段，或在渲染前保留 fragments。

---

## 8. 与当前模块的集成建议

## 8.1 `pipeline/mod.rs`

命令回复（如 `/status`, `/reset`）优先接入格式化层。

例如：

- `/status` 可以返回：
  - MarkdownV2：`*Bot Status*\n\n✅ Running`
  - 或 HTML：`<b>Bot Status</b>\n\n✅ Running`

## 8.2 `streaming/message_handler.rs`

第一阶段保持纯文本，不建议改成 MarkdownV2。

如果要增强，可只对：

- phase header（Think / Act）
- 最终 flush 的 summary

做可控格式化。

## 8.3 `sender.rs`

新增：

- `send_formatted`
- `edit_formatted`
- fallback logic

---

## 9. 建议实施阶段

## Phase 1：基础设施

目标：具备“支持 Telegram Markdown 的正确入口”。

工作项：

1. 新增 `formatting/telegram.rs`
2. 实现 `escape_markdown_v2`
3. 实现 `escape_html`
4. 定义 `FormattedMessage`
5. `MessageSender` 新增 `send_formatted`
6. `TeloxideSender` 实现 send fallback

交付结果：

- bot 可以安全发送固定 MarkdownV2 / HTML 模板消息

---

## Phase 2：命令类接入

工作项：

1. `/status` 使用格式化输出
2. `/reset` 成功/失败消息使用格式化输出
3. 媒体下载成功消息可用 code / bold 模板优化

交付结果：

- 低风险路径先验证格式系统可用性

---

## Phase 3：最终回复增强

工作项：

1. 为非 streaming 最终响应增加富文本模板
2. 支持将安全片段组合成 HTML/MarkdownV2
3. 增加失败自动回退纯文本

交付结果：

- LLM 最终回复可具备一定结构化展示能力

---

## Phase 4：可选的 Markdown 输入转换

如果业务确实需要“标准 markdown → telegram markdown/html”：

1. 引入 markdown parser（例如 pulldown-cmark）
2. 仅支持常用子集：
   - heading → bold
   - paragraph
   - inline code
   - code block
   - emphasis
   - links
3. 输出优先 HTML，而不是 MarkdownV2

注意：这不是第一阶段必须做的。

---

## 风险评估

### 1. MarkdownV2 复杂度高

风险：格式稍复杂就容易解析失败。

应对：

- 第一阶段只支持受控子集
- 动态内容统一 escape
- streaming 不用 MarkdownV2

### 2. HTML 与 MarkdownV2 双栈维护成本

风险：两套 renderer 增加复杂度。

应对：

- 统一片段模型
- renderer 分层
- 初期只重点落地一种（推荐 HTML 为主，MarkdownV2 为显式支持）

### 3. 编辑消息 parse mode 差异

风险：send 与 edit 行为可能不一致。

应对：

- 统一封装 sender
- 增加集成测试

---

## 测试建议

### 单元测试

覆盖：

1. `escape_markdown_v2`
2. `escape_html`
3. `render_markdown_v2`
4. `render_html`
5. fallback 逻辑

测试样例要覆盖：

- `_ * [ ] ( ) ~ ` > # + - = | { } . !`
- 中文 + emoji
- 文件路径
- JSON / shell 输出
- URL

### 集成测试

覆盖：

1. `/status` formatted response
2. `/reset` formatted response
3. send 失败后 fallback pure text
4. streaming 路径仍保持纯文本

---

## 最终建议

### 结论

**loom telegram-bot 应该支持 Telegram Markdown，但不应让业务层直接操作 MarkdownV2 字符串。**

正确做法是：

1. 增加统一的 Telegram formatting 层
2. 以“受控子集 + 安全转义 + fallback”为核心
3. streaming 保持纯文本
4. 最终/命令回复再启用 MarkdownV2 或 HTML

### 推荐落地顺序

1. 先做 `FormattedMessage + escape_markdown_v2`
2. 再接命令消息
3. 再考虑最终回复富文本
4. 最后才考虑通用 markdown 转换

---

## 建议新增文件

```text
telegram-bot/src/formatting/mod.rs
telegram-bot/src/formatting/telegram.rs
telegram-bot/src/tests/formatting_tests.rs
```

---

## 附：第一阶段最小 API 草案

```rust
pub enum TelegramMessageFormat {
    PlainText,
    MarkdownV2,
    Html,
}

pub struct FormattedMessage {
    pub text: String,
    pub parse_mode: Option<teloxide::types::ParseMode>,
    pub plain_text_fallback: String,
}

pub fn escape_markdown_v2(text: &str) -> String;
pub fn escape_html(text: &str) -> String;

pub fn as_markdown_code_block(title: &str, body: &str) -> FormattedMessage;
pub fn as_markdown_notice(title: &str, body: &str) -> FormattedMessage;
pub fn as_html_notice(title: &str, body: &str) -> FormattedMessage;
```

这套 API 已足够支撑第一阶段上线。
