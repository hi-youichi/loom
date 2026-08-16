use std::sync::Arc;

use crate::global_events::GlobalEventBus;

use super::ExtensionRegistry;

/// Register every extension domain implemented in this crate.
pub fn register_default_extensions(registry: &mut ExtensionRegistry, global_bus: Arc<GlobalEventBus>) {
    registry.register("files", Arc::new(super::files::FilesHandler));
    registry.register(
        "git",
        Arc::new(super::git::GitHandler::new().with_global_bus(global_bus.clone())),
    );
    registry.register("worktree", Arc::new(super::worktree::WorktreeHandler));
    registry.register("mcp", Arc::new(super::mcp::McpHandler));
 registry.register("model", Arc::new(super::model::ModelHandler::new()));
    registry.register("goal", Arc::new(super::goal::GoalHandler));
    registry.register(
        "scheduled-task",
        Arc::new(super::scheduled_task::ScheduledTaskHandler),
    );
    registry.register("connection", Arc::new(super::connection::ConnectionHandler));
    registry.register("relay", Arc::new(super::relay::RelayHandler));
    registry.register("pairing", Arc::new(super::pairing::PairingHandler::default()));
    registry.register("client-auth", Arc::new(super::client_auth::ClientAuthHandler::default()));
    registry.register("question", Arc::new(super::question::QuestionHandler::default()));
    registry.register("github", Arc::new(super::github::GithubHandler::default()));
    registry.register("notification", Arc::new(super::notification::NotificationHandler::default()));
    registry.register("skills", Arc::new(super::skills::SkillsHandler::default()));
    registry.register("session-folder", Arc::new(super::session_folder::SessionFolderHandler::default()));
    registry.register("snippet", Arc::new(super::snippet::SnippetHandler::default()));
    registry.register("command", Arc::new(super::command::CommandHandler::default()));
    registry.register("plugin", Arc::new(super::plugin::PluginHandler::default()));
    registry.register("agent", Arc::new(super::agent_profile::AgentProfileHandler::default()));
    registry.register("diagnostics", Arc::new(super::diagnostics::DiagnosticsHandler::default()));
    registry.register("project", Arc::new(super::project::ProjectHandler::persistent()));
    registry.register("tunnel", Arc::new(super::tunnel::TunnelHandler::default()));
    registry.register("multi-run", Arc::new(super::multi_run::MultiRunHandler::default()));
    registry.register(
        "settings",
        Arc::new(super::settings::SettingsHandler::default().with_global_bus(global_bus.clone())),
    );
    registry.register("session-assist", Arc::new(super::session_assist::SessionAssistHandler));
    registry.register("small-model", Arc::new(super::small_model::SmallModelHandler::default()));
    registry.register("auto-review", Arc::new(super::auto_review::AutoReviewHandler::default()));
    registry.register("preview", Arc::new(super::preview::PreviewHandler::default()));
    registry.register(
        "terminal-ext",
        Arc::new(super::terminal_ext::TerminalExtHandler::new(Arc::new(
            crate::terminal::TerminalManager::default(),
        ))),
    );
    registry.register("tts", Arc::new(super::tts::TtsHandler::default()));
    registry.register("dictation", Arc::new(super::dictation::DictationHandler::default()));
    registry.register("global", Arc::new(super::global::GlobalHandler::new(global_bus)));
}
