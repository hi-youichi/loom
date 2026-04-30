# 斜杠命令系统

通过 `CommandDispatcher` 注册和执行斜杠命令，如 `/model`、`/reset`、`/help`。

## 什么时候用

- 你想让用户在 Telegram 中通过命令切换模型、重置会话
- 你想添加自定义命令来扩展 Bot 行为

## 核心概念

### 命令分发

`CommandDispatcher` 使用 Command 模式。每个命令实现 `BotCommand` trait：

```rust
#[async_trait]
pub trait BotCommand {
    fn names(&self) -> &[&str];
    async fn execute(&ctx: CommandContext<'_>) -> Result<(), BotError>;
}
```

`CommandDispatcher::dispatch()` 按注册顺序遍历命令，第一个匹配 `names()` 的命令被执行。

### 内置命令

| 命令 | 功能 | 示例 |
|------|------|------|
| `/help` | 显示帮助信息 | `/help` |
| `/model` | 切换或搜索模型 | `/model gpt-4`、`/model list` |
| `/reset` | 重置当前聊天会话 | `/reset` |
| `/status` | 显示 Bot 状态 | `/status` |

### 自定义命令

实现 `BotCommand` trait 并注册到 `CommandDispatcher`：

```rust
use crate::command::{BotCommand, CommandContext, CommandDispatcher};
use crate::error::BotError;

pub struct GreetCommand;

#[async_trait]
impl BotCommand for GreetCommand {
    fn names(&self) -> &[&str] {
        &["greet", "hello"]
    }

    async fn execute(ctx: &CommandContext<'_>) -> Result<(), BotError> {
        let chat_id = ctx.chat_id();
        ctx.sender.send_text(chat_id, "Hello! 👋").await?;
        Ok(())
    }
}

// 注册
dispatcher.register(GreetCommand);
```

### 模型搜索

`/model` 命令支持模糊搜索。`InMemorySearchSession` 从 SQLite 加载模型列表，支持分页浏览：

- `/model list` — 显示第一页模型
- `/model gpt` — 搜索包含 "gpt" 的模型
- `/model <model_id>` — 直接切换到指定模型

## 最佳实践

✅ 命令名用小写，用下划线分隔（`/my_command`）
✅ `names()` 返回多个别名，方便用户记忆（如 `["model", "m"]`）
⚠️ 命令执行是同步阻塞的——长时间操作用 `tokio::spawn` 异步处理

## 本页覆盖范围

- 覆盖：命令注册、分发、内置命令、自定义命令、模型搜索
- 不覆盖：消息如何到达命令系统（见 [消息处理管线](message-pipeline.md)）

## 下一步

- [消息处理管线](message-pipeline.md) — 命令在管线中的位置
- [配置文件参考](../reference/config-reference.md) — 模型相关配置
