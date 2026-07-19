//! HTTP route registry — single source of truth for which URLs loom-server
//! exposes (tasks P0.2, P0.3, P0.5, P0.6, P0.7, P1.x, P2.x, P3.x).
//!
//! W4 cleanup: removed worktree, revert, mcp-connect, and oauth route groups
//! (opencode has none of these). Wired the new contract routes: `/api/provider`
//! (W2), `/api/credential` (W3), and the contract-shaped `/api/pty*` set.

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{log_authorization_header, require_valid_token};
use crate::handlers;
use crate::sse;
use crate::state::SharedState;

/// Build the application router with all current v1+v2 routes.
pub fn build_router(state: SharedState) -> Router {
    Router::new()
        // ─── Bootstrap (P0.2 + P0.3) ────────────────────────────
        .route(
            "/config",
            get(handlers::bootstrap::get_api_config).patch(handlers::bootstrap::patch_api_config),
        )
        .route("/acp", get(handlers::acp::connect))
        .route(
            "/config/providers",
            get(handlers::bootstrap::get_config_providers),
        )
        .route(
            "/config/settings",
            get(handlers::settings::get_settings)
                .put(handlers::settings::put_settings),
        )
        .route("/config/reload", post(handlers::settings::reload_config))
        .route(
            "/api/config/providers",
            get(handlers::bootstrap::get_config_providers),
        )
        .route(
            "/api/config/settings",
            get(handlers::settings::get_settings)
                .put(handlers::settings::put_settings),
        )
        .route("/api/config/reload", post(handlers::settings::reload_config))
        .route("/provider", get(handlers::bootstrap::get_provider_list))
        // ─── Provider auth CRUD (OC-compat T4) ───────────────────
        .route(
            "/provider/auth",
            get(handlers::provider_auth::get_provider_auth),
        )
        .route(
            "/provider/:providerId/auth",
            post(handlers::provider_auth::post_provider_auth)
                .delete(handlers::provider_auth::delete_provider_auth),
        )
        .route(
            "/provider/:providerId/source",
            get(handlers::provider_auth::get_provider_source),
        )
        .route(
            "/api/provider/auth",
            get(handlers::provider_auth::get_provider_auth),
        )
        .route(
            "/api/provider/:providerId/auth",
            post(handlers::provider_auth::post_provider_auth)
                .delete(handlers::provider_auth::delete_provider_auth),
        )
        .route(
            "/api/provider/:providerId/source",
            get(handlers::provider_auth::get_provider_source),
        )
        .route("/agent", get(handlers::bootstrap::get_agent_list))
        .route("/path", get(handlers::bootstrap::get_api_path))
        .route("/project", get(handlers::bootstrap::get_project_list))
        .route(
            "/project/current",
            get(handlers::bootstrap::get_project_current),
        )
        .route(
            "/project/:id/directories",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/api/project/:id/directories",
            get(handlers::v2_compat::empty_list),
        )
        .route("/command", get(handlers::bootstrap::get_command_list))
        // Note: `/api/command` (list) is registered below in the v2 SDK
        // aliases block since `get_api_commands` returns the contract-shaped
        // `Location.response` envelope and uses `bootstrap::get_api_commands`.
        .route(
            "/mcp",
            get(handlers::mcp_pty_file::get_api_mcp_status)
                .patch(handlers::mcp_pty_file::patch_mcp),
        )
        .route(
            "/mcp/status",
            get(handlers::mcp_pty_file::get_mcp_status_legacy),
        )
        .route(
            "/api/mcp/status",
            get(handlers::mcp_pty_file::get_mcp_status_legacy),
        )
        .route(
            "/api/mcp",
            get(handlers::mcp_pty_file::get_api_mcp_status)
                .post(handlers::v2_compat::true_value)
                .patch(handlers::v2_compat::true_value),
        )
        .route("/lsp", get(handlers::lsp_formatter::get_lsp_status))
        .route("/lsp/status", get(handlers::lsp_formatter::get_lsp_status))
        .route(
            "/formatter",
            get(handlers::lsp_formatter::get_formatter_status),
        )
        .route(
            "/formatter/status",
            get(handlers::lsp_formatter::get_formatter_status),
        )
        .route(
            "/session/status",
            get(handlers::messages::get_session_status),
        )
        .route(
            "/api/session/status",
            get(handlers::messages::get_session_status),
        )
        .route(
            "/experimental/capabilities",
            get(handlers::experimental::get_capabilities),
        )
        .route(
            "/api/experimental/capabilities",
            get(handlers::experimental::get_capabilities),
        )
        .route(
            "/experimental/console",
            get(handlers::experimental::get_console),
        )
        .route(
            // v2 SDK client prefix: the TUI's `sync.tsx:458` calls
            // `sdk.client.experimental.console.get(...)` which resolves
            // to `/api/experimental/console` (not the v1 `/experimental/console`).
            // Without this alias, the request 404s, `x.data` is undefined,
            // `reconcile(undefined)` writes `undefined` into
            // `sync.data.console_state`, and `dialog-provider.tsx:135` crashes
            // with `undefined is not an object (evaluating 'consoleManagedProviders.has')`.
            "/api/experimental/console",
            get(handlers::experimental::get_console),
        )
        .route(
            "/experimental/console/orgs",
            get(handlers::experimental::get_console_orgs),
        )
        .route(
            "/api/experimental/console/orgs",
            get(handlers::experimental::get_console_orgs),
        )
        .route(
            "/experimental/console/org",
            post(handlers::experimental::post_console_org),
        )
        .route(
            "/api/experimental/console/org",
            post(handlers::experimental::post_console_org),
        )
        .route(
            "/experimental/resource",
            get(handlers::experimental::get_resource).post(handlers::experimental::post_resource),
        )
        .route(
            "/experimental/resource/:id",
            get(handlers::experimental::get_resource_one)
                .delete(handlers::experimental::delete_resource_one),
        )
        .route(
            "/experimental/resource/list",
            get(handlers::experimental::get_resource_list),
        )
        .route(
            "/api/experimental/resource/list",
            get(handlers::experimental::get_resource_list),
        )
        .route(
            "/experimental/eval",
            post(handlers::experimental::post_eval),
        )
        .route(
            "/api/experimental/eval",
            post(handlers::experimental::post_eval),
        )
        // ─── v2 bootstrap (P0.2) ────────────────────────────────
        .route(
            "/api/config",
            get(handlers::bootstrap::get_api_config).patch(handlers::bootstrap::patch_api_config),
        )
        // Provider group (group-provider.ts, W2) — list + get from config.
        .route("/api/provider", get(handlers::bootstrap::get_api_providers))
        .route(
            "/api/provider/:providerID",
            get(handlers::bootstrap::get_api_provider),
        )
        // Credential group (group-credential.ts, W3) — update label + remove.
        .route(
            "/api/credential/:credentialID",
            patch(handlers::credential::update_credential)
                .delete(handlers::credential::remove_credential),
        )
        .route(
            "/api/app/agent",
            get(handlers::bootstrap::get_v2_agent_list),
        )
        .route(
            "/api/app/model",
            get(handlers::bootstrap::get_v2_model_list),
        )
        .route(
            "/api/app/provider",
            get(handlers::bootstrap::get_v2_provider_list),
        )
        .route("/api/agent", get(handlers::bootstrap::get_api_agents))
        .route("/api/model", get(handlers::bootstrap::get_api_models))
        .route("/model", get(handlers::bootstrap::get_api_models))
        .route("/api/command", get(handlers::bootstrap::get_api_commands))
        .route("/api/skill", get(handlers::bootstrap::get_api_skills))
        .route(
            "/api/reference",
            get(handlers::bootstrap::get_api_references),
        )
        .route(
            "/api/integration",
            get(handlers::bootstrap::get_api_integrations).post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/location",
            get(handlers::bootstrap::get_api_location)
                .patch(handlers::bootstrap::patch_api_location),
        )
        .route(
            "/api/location/workspace",
            axum::routing::put(handlers::v2_compat::set_workspace),
        )
        .route("/api/project", get(handlers::v2_compat::project_list))
        .route(
            "/api/project/current",
            get(handlers::v2_compat::project_current),
        )
        .route(
            "/api/project/:id",
            patch(handlers::v2_compat::project_update),
        )
        .route("/api/path", get(handlers::bootstrap::get_api_path))
        .route("/api/fs/list", get(handlers::fs::list))
        .route("/api/fs/read/*path", get(handlers::fs::read))
        .route("/api/fs/find", get(handlers::fs::find))
        .route("/api/fs/write", post(handlers::fs::write))
        .route("/api/fs/delete", post(handlers::fs::delete))
        .route("/api/fs/rename", post(handlers::fs::rename))
        .route("/api/fs/mkdir", post(handlers::fs::mkdir))
        .route("/api/fs/stat", get(handlers::fs::stat))

        // ─── PTY group (group-pty.ts, W3/W4) ─────────────────────
        // Contract-shaped /api/pty* routes. Handlers live in mcp_pty_file.rs
        // as honest 501s until handlers/pty.rs ships the full lifecycle.
        .route(
            "/api/pty",
            get(handlers::mcp_pty_file::get_api_pty_list)
                .post(handlers::mcp_pty_file::post_api_pty),
        )
        .route(
            "/api/pty/:ptyID",
            get(handlers::mcp_pty_file::get_pty_one)
                .put(handlers::mcp_pty_file::put_pty_one)
                .delete(handlers::mcp_pty_file::delete_pty_one),
        )
        .route(
            "/api/pty/:ptyID/connect-token",
            post(handlers::mcp_pty_file::post_api_pty_connect_token),
        )
        .route(
            "/api/pty/:ptyID/connect",
            get(handlers::mcp_pty_file::get_api_pty_connect),
        )
        // ─── VCS (P0.2) ─────────────────────────────────────────
        .route("/vcs", get(handlers::vcs_extra::get_vcs))
        .route("/vcs/status", get(handlers::vcs_extra::get_vcs_status))
        .route("/vcs/diff", get(handlers::vcs_extra::get_vcs_diff))
        .route("/vcs/diff/raw", get(handlers::vcs_extra::get_vcs_diff_raw))
        .route("/api/vcs", get(handlers::vcs_extra::get_vcs))
        .route("/api/vcs/status", get(handlers::vcs_extra::get_vcs_status))
        .route("/api/vcs/diff", get(handlers::vcs_extra::get_vcs_diff))
        .route(
            "/api/vcs/diff/raw",
            get(handlers::vcs_extra::get_vcs_diff_raw),
        )
        // ─── Git operations (OC-G) ───────────────────────────────
        .route("/git/check", get(handlers::git::check))
        .route("/git/stage", post(handlers::git::stage))
        .route("/git/unstage", post(handlers::git::unstage))
        .route("/git/commit", post(handlers::git::commit))
        .route("/git/log", get(handlers::git::log))
        .route("/git/branches", get(handlers::git::branches))
        .route("/api/git/check", get(handlers::git::check))
        .route("/api/git/stage", post(handlers::git::stage))
        .route("/api/git/unstage", post(handlers::git::unstage))
        .route("/api/git/commit", post(handlers::git::commit))
        .route("/api/git/log", get(handlers::git::log))
        .route("/api/git/branches", get(handlers::git::branches))
        // ─── Health (P0.2, P0.3) ────────────────────────────────
        .route("/api/health", get(handlers::health::get_api_health))
        .route("/global/health", get(handlers::health::get_global_health))
        .route(
            "/api/permission/saved",
            get(handlers::health::get_permission_saved),
        )
        // ─── Instance + auth metadata (P2.20) ──────────────────
        .route("/instance", get(handlers::instance::get_instance))
        .route(
            "/api/instance",
            get(handlers::instance::get_api_instance)
                .post(handlers::v2_compat::instance_start)
                .delete(handlers::v2_compat::instance_dispose),
        )
        .route("/auth", get(handlers::instance::get_auth))
        .route("/api/auth", get(handlers::instance::get_auth))
        // ─── Session CRUD (P1.13, P1.14) ───────────────────────
        .route(
            "/session",
            get(handlers::session::list_sessions).post(handlers::session::create_session),
        )
        .route(
            "/api/session",
            get(handlers::global_bus::get_global_session).post(handlers::session::create_session),
        )
        .route(
            "/session/:id",
            get(handlers::session::get_session)
                .patch(handlers::session::patch_session)
                .delete(handlers::session::delete_session),
        )
        .route(
            "/api/session/:sessionID",
            get(handlers::session::get_session)
                .patch(handlers::session::patch_session)
                .delete(handlers::session::delete_session),
        )
        .route(
            "/global/session/:id",
            delete(handlers::session::delete_session),
        )
        .route(
            "/session/:id/children",
            get(handlers::session::get_session_children),
        )
        .route(
            "/api/session/:sessionID/children",
            get(handlers::session::get_session_children),
        )
        .route(
            "/session/:id/fork",
            post(handlers::session::post_api_session_fork),
        )
        .route(
            "/session/:id/init",
            post(handlers::session::post_api_session_init),
        )
        .route(
            "/session/:id/summarize",
            post(handlers::session::post_api_session_summarize),
        )
        .route(
            "/session/:id/share",
            post(handlers::session::post_session_share)
                .delete(handlers::session::delete_session_share),
        )
        .route(
            "/api/session/:sessionID/share",
            post(handlers::session::post_session_share)
                .delete(handlers::session::delete_session_share),
        )
        // ─── Session main paths (P1.8, P1.9, P1.10, P1.11, P1.13) ──
        .route("/session/:id/prompt", post(handlers::session::prompt))
        .route(
            "/session/:id/prompt_async",
            post(handlers::session::prompt_async),
        )
        .route(
            "/api/session/:sessionID/prompt_async",
            post(handlers::session::prompt_async),
        )
        .route(
            "/session/:id/command",
            post(handlers::session::session_command),
        )
        .route("/session/:id/shell", post(handlers::session::session_shell))
        .route("/session/:id/abort", post(handlers::session::session_abort))
        .route(
            "/api/session/:sessionID/agent",
            post(handlers::session::api_session_prompt),
        )
        // Contract (group-session.ts): session.prompt — durable admit one input.
        .route(
            "/api/session/:sessionID/prompt",
            post(handlers::session::prompt),
        )
        .route(
            "/api/session/:sessionID/command",
            post(handlers::session::api_session_command),
        )
        .route(
            "/api/session/:sessionID/shell",
            post(handlers::session::api_session_shell),
        )
        .route(
            "/api/session/:sessionID/interrupt",
            post(handlers::session::api_session_interrupt),
        )
        .route(
            "/api/session/:sessionID/status",
            get(handlers::session::session_status),
        )
        .route(
            "/api/session/:sessionID/fork",
            post(handlers::session::post_api_session_fork),
        )
        .route(
            "/api/session/:sessionID/summarize",
            post(handlers::session::post_api_session_summarize),
        )
        .route(
            "/api/session/:sessionID/init",
            post(handlers::session::post_api_session_init),
        )
        .route(
            "/api/location/snapshot",
            post(handlers::vcs_extra::post_api_location_snapshot),
        )
        .route(
            "/api/session/:sessionID/event",
            get(sse::api_session_event_stream),
        )
        .route(
            "/api/session/active",
            get(handlers::v2_compat::active_sessions),
        )
        .route(
            "/api/session/create",
            post(handlers::v2_compat::create_workspace_session),
        )
        .route("/api/lsp", get(handlers::lsp_formatter::get_lsp_status))
        .route(
            "/api/lsp/status",
            get(handlers::lsp_formatter::get_lsp_status),
        )
        .route(
            "/api/formatter",
            get(handlers::lsp_formatter::get_formatter_status),
        )
        .route(
            "/api/formatter/status",
            get(handlers::lsp_formatter::get_formatter_status),
        )
        // ─── Permission (P2.18) ─────────────────────────────────
        .route(
            "/permission",
            get(handlers::permission::get_permission_pending),
        )
        .route(
            "/permission/:requestID/reply",
            post(handlers::permission::post_permission_reply),
        )
        .route(
            "/api/permission",
            post(handlers::permission::post_api_permission),
        )
        // Contract (group-permission.ts): GET /api/permission/request lists
        // pending location-scoped permission requests.
        .route(
            "/api/permission/request",
            get(handlers::permission::get_api_permission_pending),
        )
        .route(
            "/api/permission/saved/:id",
            get(handlers::v2_compat::empty_object).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/permission/pending",
            get(handlers::permission::get_api_permission_pending),
        )
        .route(
            "/api/permission/:requestID/reply",
            post(handlers::permission::post_api_permission_reply),
        )
        .route(
            "/session/:id/permissions/:permissionID",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/session/:sessionID/permissions/:permissionID",
            post(handlers::v2_compat::true_value),
        )
        // ─── Question (P2.18) ───────────────────────────────────
        .route(
            "/question",
            get(handlers::question::get_question_pending).post(handlers::question::post_question),
        )
        .route(
            "/question/:requestID/reply",
            post(handlers::question::post_question_reply),
        )
        .route(
            "/question/:requestID/reject",
            post(handlers::question::post_question_reject),
        )
        .route("/api/question", post(handlers::question::post_api_question))
        // Contract (group-question.ts): GET /api/question/request lists
        // pending location-scoped question requests.
        .route(
            "/api/question/request",
            get(handlers::question::get_api_question_pending),
        )
        .route(
            "/api/question/pending",
            get(handlers::question::get_api_question_pending),
        )
        .route(
            "/api/question/:requestID/reply",
            post(handlers::question::post_api_question_reply),
        )
        // ─── TUI control (P1.16) ───────────────────────────────
        .route("/tui/command", post(handlers::control::post_tui_command))
        .route(
            "/tui/control/next",
            get(handlers::v2_compat::empty_object).post(handlers::control::post_tui_control_next),
        )
        .route(
            "/tui/control/exit",
            post(handlers::control::post_tui_control_exit),
        )
        .route(
            "/tui/control/cancel/:request_id",
            post(handlers::control::post_tui_control_cancel),
        )
        .route("/control/next", post(handlers::control::post_control_next))
        // ─── MCP auth + File/Find (P2.20) ──────────────────────
        // NOTE: MCP connect/disconnect removed (W4 — opencode has no
        // mcp-connect group). MCP auth stubs retained for compat.
        .route(
            "/mcp/:name/auth",
            post(handlers::mcp_pty_file::post_mcp_auth).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/mcp/:name/auth/callback",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/mcp/:name/auth",
            post(handlers::mcp_pty_file::post_api_mcp_auth).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/file",
            get(handlers::mcp_pty_file::get_file).put(handlers::mcp_pty_file::put_file),
        )
        .route(
            "/api/file",
            get(handlers::mcp_pty_file::get_api_file).put(handlers::mcp_pty_file::put_file),
        )
        .route(
            "/api/file/content",
            get(handlers::mcp_pty_file::get_file_content),
        )
        .route(
            "/api/file/status",
            get(handlers::mcp_pty_file::get_file_status),
        )
        .route(
            "/file/content",
            get(handlers::mcp_pty_file::get_file_content),
        )
        .route("/file/status", get(handlers::mcp_pty_file::get_file_status))
        .route(
            "/find",
            get(handlers::mcp_pty_file::get_find).post(handlers::mcp_pty_file::post_find),
        )
        .route("/find/symbol", get(handlers::mcp_pty_file::get_find_symbol))
        .route("/find/file", get(handlers::mcp_pty_file::get_find_file))
        .route(
            "/api/find",
            get(handlers::mcp_pty_file::get_find).post(handlers::mcp_pty_file::post_api_find),
        )
        .route(
            "/api/find/symbol",
            get(handlers::mcp_pty_file::get_api_find_symbol),
        )
        .route(
            "/api/find/file",
            get(handlers::mcp_pty_file::get_api_find_file),
        )
        // ─── Global (P2.23) — keep honest 501 for upgrade/instance ──
        .route(
            "/global/upgrade",
            post(handlers::global_bus::post_global_upgrade),
        )
        .route(
            "/api/instance/dispose",
            post(handlers::v2_compat::instance_dispose),
        )
        .route(
            "/global/event/replay",
            get(handlers::global_bus::get_global_event_replay),
        )
        .route(
            "/global/dispose",
            post(handlers::global_bus::post_global_dispose),
        )
        .route(
            "/global/version",
            get(handlers::global_bus::get_global_version),
        )
        .route(
            "/global/config",
            get(handlers::global_bus::get_global_config)
                .patch(handlers::global_bus::patch_global_config),
        )
        .route(
            "/global/instance/update",
            post(handlers::global_bus::post_global_instance_update),
        )
        .route(
            "/api/global/event",
            get(handlers::global_bus::get_api_global_event),
        )
        .route(
            "/api/global/event/:id",
            patch(handlers::global_bus::patch_api_global_event_ack),
        )
        // ─── Messages (P1.12, P1.15) ───────────────────────────
        .route(
            "/session/:id/messages",
            get(handlers::messages::get_messages),
        )
        .route(
            "/session/:id/message",
            get(handlers::messages::get_messages).post(handlers::session::prompt),
        )
        .route(
            "/session/:id/message/:messageID",
            get(handlers::messages::get_session_message)
                .delete(handlers::messages::delete_api_session_message),
        )
        .route(
            "/session/:id/message/:messageID/part/:partID",
            patch(handlers::messages::patch_api_session_message_part)
                .delete(handlers::messages::delete_api_session_message_part),
        )
        .route(
            "/session/:id/todo",
            get(handlers::session::get_session_todo),
        )
        .route(
            "/api/session/:sessionID/todo",
            get(handlers::session::get_session_todo),
        )
        .route(
            "/session/:id/diff",
            get(handlers::session::get_session_diff),
        )
        .route(
            "/api/session/:sessionID/diff",
            get(handlers::session::get_session_diff),
        )
        .route(
            "/api/session/:sessionID/messages",
            get(handlers::messages::get_messages),
        )
        .route(
            "/api/session/:sessionID/message",
            get(handlers::messages::get_api_session_message)
                .post(handlers::messages::post_api_session_message),
        )
        .route(
            "/api/session/:sessionID/message/:messageID",
            get(handlers::messages::get_session_message)
                .delete(handlers::messages::delete_api_session_message),
        )
        .route(
            "/api/session/:sessionID/message/:messageID/part",
            get(handlers::messages::get_api_session_message_parts),
        )
        .route(
            "/api/session/:sessionID/message/:messageID/part/:partID",
            get(handlers::messages::get_api_session_message_part)
                .patch(handlers::messages::patch_api_session_message_part)
                .delete(handlers::messages::delete_api_session_message_part),
        )
        // ─── Current v2 SDK aliases (protocol drift compatibility) ──
        .route("/experimental/agent", get(handlers::v2_compat::empty_list))
        .route(
            "/api/experimental/agent",
            get(handlers::v2_compat::empty_list),
        )
        .route("/experimental/tool", get(handlers::v2_compat::empty_list))
        .route(
            "/api/experimental/tool",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/experimental/workspace",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/experimental/console/switch",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/console/switch",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/experimental/tool/ids",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/api/experimental/tool/ids",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/experimental/session",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/experimental/session/:id/background",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/session",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/api/experimental/session/:id/background",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/workspace",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/api/experimental/resource",
            get(handlers::v2_compat::empty_list).post(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/experimental/resource/:id",
            get(handlers::v2_compat::empty_object).delete(handlers::v2_compat::true_value),
        )
        .route("/api/workspace", get(handlers::v2_compat::empty_list))
        .route("/api/tool/ids", get(handlers::v2_compat::empty_list))
        .route("/api/tool", get(handlers::v2_compat::empty_list))
        .route(
            "/api/permission/policy",
            get(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/permission/ruleset",
            get(handlers::v2_compat::empty_list),
        )
        .route(
            "/api/question/:requestID/reject",
            post(handlers::v2_compat::true_value),
        )
        .route("/api/command/:name", get(handlers::v2_compat::empty_object))
        .route("/api/model/:id", get(handlers::v2_compat::empty_object))
        .route(
            "/api/reference/workspace",
            get(handlers::v2_compat::empty_list),
        )
        .route("/api/skill/:name", delete(handlers::v2_compat::true_value))
        .route("/api/mcp/:name", delete(handlers::v2_compat::true_value))
        .route(
            "/api/mcp/:name/auth/callback",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/app",
            get(handlers::v2_compat::empty_list).post(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/experimental/app/:id",
            get(handlers::v2_compat::empty_object)
                .patch(handlers::v2_compat::empty_object)
                .delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/app/:id/auth",
            post(handlers::v2_compat::true_value).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/app/:id/auth/authorize",
            post(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/experimental/app/:id/auth/callback",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/integration/:integrationID",
            axum::routing::put(handlers::v2_compat::true_value)
                .delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/config/agents",
            get(handlers::v2_compat::empty_object).patch(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/config/agents/:name",
            get(handlers::v2_compat::empty_object)
                .patch(handlers::v2_compat::empty_object)
                .delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/config/auth",
            get(handlers::v2_compat::empty_object).put(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/config/auth/:providerID",
            delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/event/:eventID/ack",
            post(handlers::v2_compat::true_value),
        )
        .route("/tui/submit-prompt", post(handlers::v2_compat::true_value))
        .route("/tui/clear-prompt", post(handlers::v2_compat::true_value))
        .route("/tui/publish", post(handlers::v2_compat::true_value))
        .route("/tui/select-session", post(handlers::v2_compat::true_value))
        .route("/tui/select", post(handlers::v2_compat::true_value))
        .route("/tui/append-prompt", post(handlers::v2_compat::true_value))
        .route("/tui/open-help", post(handlers::v2_compat::true_value))
        .route("/tui/open-sessions", post(handlers::v2_compat::true_value))
        .route("/tui/open-themes", post(handlers::v2_compat::true_value))
        .route("/tui/open-models", post(handlers::v2_compat::true_value))
        .route(
            "/tui/execute-command",
            post(handlers::v2_compat::true_value),
        )
        .route("/tui/show-toast", post(handlers::v2_compat::true_value))
        .route(
            "/tui/control/response",
            post(handlers::v2_compat::true_value),
        )
        // ─── MISSING CONTRACT ROUTES (awaiting W2 handlers) ───
        // The following opencode v2 endpoints (groups/*.ts) have no handler
        // yet. Listed as comments so W2 can wire them; kept out of the
        // Router so the crate stays compiling and no stubs ship.
        //
        // ── integration (group-integration.ts) ──
        // TODO(W2): GET    /api/integration/:integrationID            → handlers::integration::get
        // TODO(W2): POST   /api/integration/:integrationID/connect/key  → handlers::integration::connect_key
        // TODO(W2): POST   /api/integration/:integrationID/connect/oauth→ handlers::integration::connect_oauth
        // TODO(W2): GET    /api/integration/attempt/:attemptID          → handlers::integration::attempt_status
        // TODO(W2): POST   /api/integration/attempt/:attemptID/complete → handlers::integration::attempt_complete
        // TODO(W2): DELETE /api/integration/attempt/:attemptID          → handlers::integration::attempt_cancel
        //
        // ── fs (group-fs.ts) ──
        // fs.read/find now registered above with handlers::fs::read/find.
        // (fs.find GET is registered above as a TODO; see handlers::fs::find)
        //
        // ── session (group-session.ts) — switchModel/compact/wait/context/history ──
        // TODO(W2): POST /api/session/:sessionID/model   → handlers::session::switch_model (switchAgent→/agent wired above)
        // TODO(W2): POST /api/session/:sessionID/compact → handlers::session::compact
        // TODO(W2): POST /api/session/:sessionID/wait    → handlers::session::wait
        // TODO(W2): GET  /api/session/:sessionID/context → handlers::session::get_context
        // TODO(W2): GET  /api/session/:sessionID/history → handlers::session::get_history
        //
        // ── session revert (group-session.ts: session.revert.*) ──
        // TODO(W2): POST /api/session/:sessionID/revert/stage   → handlers::session::revert_stage
        // TODO(W2): POST /api/session/:sessionID/revert/clear   → handlers::session::revert_clear
        // TODO(W2): POST /api/session/:sessionID/revert/commit  → handlers::session::revert_commit
        //
        // ── session-scoped permission (group-permission.ts: session.permission.*) ──
        // TODO(W2): POST /api/session/:sessionID/permission             → handlers::permission::create
        // TODO(W2): GET  /api/session/:sessionID/permission             → handlers::permission::list_session
        // TODO(W2): GET  /api/session/:sessionID/permission/:requestID  → handlers::permission::get_session
        // TODO(W2): POST /api/session/:sessionID/permission/:requestID/reply → handlers::permission::reply_session
        //
        // ── session-scoped question (group-question.ts: session.question.*) ──
        // TODO(W2): GET  /api/session/:sessionID/question                  → handlers::question::list_session
        // TODO(W2): POST /api/session/:sessionID/question/:requestID/reply → handlers::question::reply_session
        // TODO(W2): POST /api/session/:sessionID/question/:requestID/reject→ handlers::question::reject_session
        //
        // ── project copy (group-project-copy.ts) ──
        // TODO(W2): POST   /experimental/project/:projectID/copy         → handlers::project_copy::create
        // TODO(W2): DELETE /experimental/project/:projectID/copy         → handlers::project_copy::remove
        // TODO(W2): POST   /experimental/project/:projectID/copy/refresh → handlers::project_copy::refresh

        // ─── SSE channels (P0.4) ───────────────────────────────
        .route("/event", get(sse::event_stream))
        .route("/global/event", get(sse::event_stream))
        .route("/api/event", get(sse::api_event_stream))
        // ─── Auth middleware (P0.7 + LS-017) ─────────────────────
        // `log_authorization_header` records credential presence; `require_valid_token`
        // enforces an optional bearer token (LOOM_AUTH_TOKEN). When no token is
        // configured, all requests are allowed (development mode).
        .layer(middleware::from_fn(require_valid_token))
        .layer(middleware::from_fn(log_authorization_header))
        // ─── CORS (LS-017) — permissive for local development ──
        .layer(CorsLayer::very_permissive())
        // ─── Tracing layer (P0.1) ──────────────────────────────
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
