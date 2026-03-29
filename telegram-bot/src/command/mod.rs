//! Slash-style bot commands (Command pattern).
//!
//! Each command is a small type implementing [`BotCommand`]; [`CommandDispatcher`] runs them in order.

use async_trait::async_trait;

use crate::error::BotError;
use crate::formatting::telegram::markdown_notice;
use crate::handler_deps::HandlerDeps;
use crate::model_selection::ModelSearchResult;

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
            commands: vec![
                Box::new(ResetCommand),
                Box::new(StatusCommand),
                Box::new(ModelCommand),
            ],
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
        for command in &self.commands {
            if command.matches(text) {
                return Some(command.execute(ctx).await);
            }
        }
        None
    }
}

struct ResetCommand;
struct StatusCommand;
struct ModelCommand;

#[async_trait]
impl BotCommand for ResetCommand {
    fn matches(&self, text: &str) -> bool {
        let trimmed_text = text.trim();
        trimmed_text == "/reset" || trimmed_text.starts_with("/reset ")
    }

    async fn execute(&self, ctx: &CommandContext<'_>) -> Result<(), BotError> {
        let thread_id = format!("telegram_{}", ctx.chat_id);
        match ctx.deps.session.reset(&thread_id).await {
            Ok(count) => {
                let message = markdown_notice(
                    "Session Reset",
                    &format!("🔄 Deleted {} checkpoints.", count),
                );
                ctx.deps.sender.send_formatted(ctx.chat_id, &message).await?;
            }
            Err(error) => {
                tracing::error!("Failed to reset session: {}", error);
                let message = markdown_notice("Reset Failed", &format!("❌ {}", error));
                ctx.deps.sender.send_formatted(ctx.chat_id, &message).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl BotCommand for StatusCommand {
    fn matches(&self, text: &str) -> bool {
        text.trim() == "/status"
    }

    async fn execute(&self, ctx: &CommandContext<'_>) -> Result<(), BotError> {
        let message = markdown_notice("Bot Status", "✅ Bot is running!");
        ctx.deps.sender.send_formatted(ctx.chat_id, &message).await?;
        Ok(())
    }
}

#[async_trait]
impl BotCommand for ModelCommand {
    fn matches(&self, text: &str) -> bool {
        text.trim() == "/model"
    }

    async fn execute(&self, ctx: &CommandContext<'_>) -> Result<(), BotError> {
        let current_model = ctx.deps.model_selection.current_model(ctx.chat_id)?;
        let message = format!(
            "Current model: `{}`\n\nSend `/model <query>` to search (e.g. `/model gpt`).\nSend `/model use <model_id>` to switch.\nSend `/model reset` to restore the default.",
            current_model
        );
        ctx.deps.sender.send_text(ctx.chat_id, &message).await?;
        Ok(())
    }
}

pub async fn try_handle_model_command_input(
    ctx: &CommandContext<'_>,
    text: &str,
) -> Result<bool, BotError> {
    let trimmed_text = text.trim();

    if trimmed_text == "/model next" {
        return send_next_page(ctx).await;
    }

    if trimmed_text == "/model prev" {
        return send_previous_page(ctx).await;
    }

    if trimmed_text == "/model reset" {
        ctx.deps.model_selection.clear_selection(ctx.chat_id)?;
        ctx.deps
            .sender
            .send_text(ctx.chat_id, "Default model restored.")
            .await?;
        return Ok(true);
    }

    if let Some(model_id) = trimmed_text.strip_prefix("/model use ") {
        let selected_model = model_id.trim();
        ctx.deps.model_selection.select_model(ctx.chat_id, selected_model)?;
        ctx.deps
            .sender
            .send_text(ctx.chat_id, &format!("Switched to `{}`.", selected_model))
            .await?;
        return Ok(true);
    }

    if let Some(query) = trimmed_text.strip_prefix("/model ") {
        let result = ctx.deps.model_selection.search_models(ctx.chat_id, query.trim(), 1);
        send_model_search_result(ctx, &result).await?;
        return Ok(true);
    }

    Ok(false)
}

async fn send_next_page(ctx: &CommandContext<'_>) -> Result<bool, BotError> {
    if let Some(result) = ctx.deps.model_selection.next_page(ctx.chat_id) {
        send_model_search_result(ctx, &result).await?;
    } else {
        ctx.deps
            .sender
            .send_text(ctx.chat_id, "No active search session.")
            .await?;
    }
    Ok(true)
}

async fn send_previous_page(ctx: &CommandContext<'_>) -> Result<bool, BotError> {
    if let Some(result) = ctx.deps.model_selection.previous_page(ctx.chat_id) {
        send_model_search_result(ctx, &result).await?;
    } else {
        ctx.deps
            .sender
            .send_text(ctx.chat_id, "No active search session.")
            .await?;
    }
    Ok(true)
}

async fn send_model_search_result(
    ctx: &CommandContext<'_>,
    result: &ModelSearchResult,
) -> Result<(), BotError> {
    if result.items.is_empty() {
        let text = format!("No models match `{}`.", result.query);
        ctx.deps.sender.send_text(ctx.chat_id, &text).await?;
        return Ok(());
    }

    let mut lines = vec![format!(
        "Results for `{}` (page {}/{}):",
        result.query, result.page, result.page_count
    )];

    for item in &result.items {
        lines.push(format!("- `{}`", item.model_id));
    }

    if result.page < result.page_count {
        lines.push("Send `/model next` for the next page.".to_string());
    }
    if result.page > 1 {
        lines.push("Send `/model prev` for the previous page.".to_string());
    }
    lines.push("Send `/model use <model_id>` to select a model.".to_string());

    ctx.deps.sender.send_text(ctx.chat_id, &lines.join("\n")).await?;
    Ok(())
}
