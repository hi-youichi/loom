//! Slash-style bot commands (Command pattern).
//!
//! Each command is a small type implementing [`BotCommand`]; [`CommandDispatcher`] runs them in order.

use async_trait::async_trait;

use crate::error::BotError;
use crate::formatting::telegram::markdown_notice;
use crate::handler_deps::HandlerDeps;


/// Context available when executing a command.
pub struct CommandContext<'a> {
    pub chat_id: i64,
    pub deps: &'a HandlerDeps,
}

#[async_trait]
pub trait BotCommand: Send + Sync {
    fn matches(&self, text: &str) -> bool;
    async fn execute(&self, ctx: &CommandContext<'_>) -> Result<(), BotError>;
}

/// Ordered list of commands; first match wins.
pub struct CommandDispatcher {
    commands: Vec<Box<dyn BotCommand>>,
}

impl CommandDispatcher {
    pub fn new() -> Self {
        Self {
            commands: vec![Box::new(ResetCommand), Box::new(StatusCommand)],
        }
    }
}

impl Default for CommandDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandDispatcher {
    /// If any command matches `text`, run it and return `Some(result)`.
    pub async fn try_dispatch(
        &self,
        ctx: &CommandContext<'_>,
        text: &str,
    ) -> Option<Result<(), BotError>> {
        for cmd in &self.commands {
            if cmd.matches(text) {
                return Some(cmd.execute(ctx).await);
            }
        }
        None
    }
}

struct ResetCommand;

#[async_trait]
impl BotCommand for ResetCommand {
    fn matches(&self, text: &str) -> bool {
        let t = text.trim();
        t == "/reset" || t.starts_with("/reset ")
    }

    async fn execute(&self, ctx: &CommandContext<'_>) -> Result<(), BotError> {
        let thread_id = format!("telegram_{}", ctx.chat_id);
        match ctx.deps.session.reset(&thread_id).await {
            Ok(count) => {
                let msg = markdown_notice(
                    "Session Reset",
                    &format!("🔄 Deleted {} checkpoints.", count),
                );
                ctx.deps.sender.send_formatted(ctx.chat_id, &msg).await?;

            }
            Err(e) => {
                tracing::error!("Failed to reset session: {}", e);
                let msg = markdown_notice("Reset Failed", &format!("❌ {}", e));
                ctx.deps.sender.send_formatted(ctx.chat_id, &msg).await?;

            }
        }
        Ok(())
    }
}

struct StatusCommand;

#[async_trait]
impl BotCommand for StatusCommand {
    fn matches(&self, text: &str) -> bool {
        text.trim() == "/status"
    }

    async fn execute(&self, ctx: &CommandContext<'_>) -> Result<(), BotError> {
        let msg = markdown_notice("Bot Status", "✅ Bot is running!");
        ctx.deps.sender.send_formatted(ctx.chat_id, &msg).await?;

        Ok(())
    }
}
