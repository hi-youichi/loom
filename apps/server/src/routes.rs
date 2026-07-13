//! HTTP route registry — single source of truth for which URLs loom-server
//! exposes (tasks P0.2, P0.3, P0.5, P0.6, P0.7, P1.x, P2.x, P3.x).
//!
//! The set of registered routes is now wide — every URL the opencode
//! v1 + v2 TUI bootstrap may hit. Most are stubs that return empty
//! JSON envelopes, but they share the same envelopes the opencode code
//! expects so its `Promise.all`s and `.fetch().then()` resolvers all
//! complete cleanly.

use axum::{
    middleware,
    routing::{delete, get, patch, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::auth::log_authorization_header;
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
        .route(
            "/config/providers",
            get(handlers::bootstrap::get_config_providers),
        )
        .route("/provider", get(handlers::bootstrap::get_provider_list))
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
            "/experimental/capabilities",
            get(handlers::experimental::get_capabilities),
        )
        .route(
            "/experimental/console",
            get(handlers::experimental::get_console),
        )
        .route(
            "/experimental/console/orgs",
            get(handlers::experimental::get_console_orgs),
        )
        .route(
            "/experimental/console/org",
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
            "/experimental/eval",
            post(handlers::experimental::post_eval),
        )
        // ─── Provider OAuth (P2.22) ────────────────────────────
        .route(
            "/provider/auth",
            get(handlers::v2_compat::empty_object)
                .post(handlers::provider_auth::post_provider_auth),
        )
        .route(
            "/provider/auth/:id",
            get(handlers::provider_auth::get_provider_auth),
        )
        .route(
            "/api/provider/auth",
            post(handlers::provider_auth::post_api_provider_auth),
        )
        .route(
            "/api/provider/auth/:id",
            get(handlers::provider_auth::get_api_provider_auth)
                .delete(handlers::provider_auth::delete_api_provider_auth),
        )
        // ─── v2 bootstrap (P0.2) ────────────────────────────────
        .route(
            "/api/config",
            get(handlers::bootstrap::get_api_config).patch(handlers::bootstrap::patch_api_config),
        )
        .route("/api/provider", get(handlers::bootstrap::get_api_providers))
        .route(
            "/api/provider/:id",
            get(handlers::bootstrap::get_api_provider),
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
        .route("/api/fs/list", get(handlers::bootstrap::get_api_fs_list))
        .route("/api/fs/find", post(handlers::bootstrap::post_api_fs_find))
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
            "/api/session/:id",
            get(handlers::session::get_session)
                .patch(handlers::session::patch_session)
                .delete(handlers::session::delete_session),
        )
        .route(
            "/global/session/:id",
            delete(handlers::global_bus::delete_global_session),
        )
        .route(
            "/session/:id/children",
            get(handlers::session::get_session_children),
        )
        .route(
            "/api/session/:id/children",
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
            post(handlers::session::post_session_share).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/session/:id/unrevert",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/session/:id/share",
            post(handlers::session::post_session_share),
        )
        // ─── Session main paths (P1.8, P1.9, P1.10, P1.11, P1.13) ──
        .route("/session/:id/prompt", post(handlers::session::prompt))
        .route(
            "/session/:id/prompt_async",
            post(handlers::session::prompt_async),
        )
        .route(
            "/session/:id/command",
            post(handlers::session::session_command),
        )
        .route("/session/:id/shell", post(handlers::session::session_shell))
        .route("/session/:id/abort", post(handlers::session::session_abort))
        .route(
            "/api/session/:id/agent",
            post(handlers::session::api_session_prompt),
        )
        .route(
            "/api/session/:id/command",
            post(handlers::session::api_session_command),
        )
        .route(
            "/api/session/:id/shell",
            post(handlers::session::api_session_shell),
        )
        .route(
            "/api/session/:id/interrupt",
            post(handlers::session::api_session_interrupt),
        )
        .route(
            "/api/session/:id/status",
            get(handlers::v2_compat::session_status),
        )
        .route(
            "/api/session/:id/fork",
            post(handlers::session::post_api_session_fork),
        )
        .route(
            "/api/session/:id/summarize",
            post(handlers::session::post_api_session_summarize),
        )
        .route(
            "/api/session/:id/init",
            post(handlers::session::post_api_session_init),
        )
        .route(
            "/api/session/:id/revert",
            post(handlers::session::post_api_session_revert),
        )
        .route(
            "/api/location/snapshot",
            post(handlers::vcs_extra::post_api_location_snapshot),
        )
        .route(
            "/api/session/:id/event",
            get(handlers::session::get_api_session_event),
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
        // ─── Reverts (P2.19) ────────────────────────────────────
        .route(
            "/session/:id/revert",
            post(handlers::revert::post_session_revert),
        )
        .route(
            "/session/:id/revert/stage",
            get(handlers::revert::get_session_revert_stage),
        )
        .route(
            "/session/:id/revert/clear",
            post(handlers::revert::post_session_revert_clear),
        )
        .route(
            "/session/:id/revert/commit",
            post(handlers::revert::post_session_revert_commit),
        )
        .route(
            "/api/session/:id/revert/stage",
            get(handlers::revert::get_api_session_revert_stage)
                .post(handlers::revert::post_api_session_revert_stage),
        )
        .route(
            "/api/session/:id/revert/clear",
            post(handlers::revert::post_api_session_revert_clear),
        )
        .route(
            "/api/session/:id/revert/commit",
            post(handlers::revert::post_api_session_revert_commit),
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
        .route(
            "/api/permission/request",
            post(handlers::permission::post_api_permission),
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
            "/api/session/:id/permissions/:permissionID",
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
        .route(
            "/api/question/request",
            post(handlers::question::post_api_question),
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
        // ─── MCP/PTY/File/Find (P2.20) ─────────────────────────
        .route(
            "/mcp/:name/auth",
            post(handlers::mcp_pty_file::post_mcp_auth).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/mcp/:name/auth/callback",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/mcp/:name/connect",
            post(handlers::mcp_pty_file::post_mcp_connect),
        )
        .route(
            "/mcp/:name/disconnect",
            post(handlers::mcp_pty_file::post_mcp_disconnect),
        )
        .route(
            "/api/mcp/:name/auth",
            post(handlers::mcp_pty_file::post_api_mcp_auth).delete(handlers::v2_compat::true_value),
        )
        .route(
            "/api/mcp/:name/connect",
            post(handlers::mcp_pty_file::post_mcp_connect),
        )
        .route(
            "/api/mcp/:name/disconnect",
            post(handlers::mcp_pty_file::post_mcp_disconnect),
        )
        .route(
            "/pty",
            get(handlers::mcp_pty_file::get_pty_list).post(handlers::mcp_pty_file::post_pty),
        )
        .route(
            "/pty/:id",
            get(handlers::mcp_pty_file::get_pty_one)
                .patch(handlers::mcp_pty_file::patch_pty_one)
                .delete(handlers::mcp_pty_file::delete_pty_one),
        )
        .route(
            "/api/pty",
            get(handlers::mcp_pty_file::get_api_pty_list)
                .post(handlers::mcp_pty_file::post_api_pty),
        )
        .route(
            "/api/pty/:id",
            get(handlers::mcp_pty_file::get_pty_one)
                .patch(handlers::mcp_pty_file::patch_pty_one)
                .delete(handlers::mcp_pty_file::delete_pty_one),
        )
        .route("/pty/:id/connect", get(handlers::v2_compat::empty_object))
        .route("/pty/:id/input", post(handlers::v2_compat::true_value))
        .route("/pty/:id/resize", patch(handlers::v2_compat::true_value))
        .route("/api/pty/:id/input", post(handlers::v2_compat::true_value))
        .route(
            "/api/pty/:id/resize",
            patch(handlers::v2_compat::true_value),
        )
        .route(
            "/api/pty/:id/connect",
            get(handlers::v2_compat::empty_object),
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
        .route(
            "/worktree",
            get(handlers::v2_compat::empty_list).post(handlers::v2_compat::empty_object),
        )
        .route("/global/upgrade", post(handlers::v2_compat::true_value))
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
            get(handlers::messages::get_session_todo),
        )
        .route(
            "/session/:id/diff",
            get(handlers::messages::get_session_diff),
        )
        .route(
            "/api/session/:id/messages",
            get(handlers::messages::get_messages),
        )
        .route(
            "/api/session/:id/message",
            get(handlers::messages::get_api_session_message)
                .post(handlers::messages::post_api_session_message),
        )
        .route(
            "/api/session/:id/message/:messageID",
            delete(handlers::messages::delete_api_session_message),
        )
        .route(
            "/api/session/:id/message/:messageID/part",
            get(handlers::messages::get_api_session_message_parts),
        )
        .route(
            "/api/session/:id/message/:messageID/part/:partID",
            get(handlers::messages::get_api_session_message_part)
                .patch(handlers::messages::patch_api_session_message_part)
                .delete(handlers::messages::delete_api_session_message_part),
        )
        // ─── Current v2 SDK aliases (protocol drift compatibility) ──
        .route(
            "/experimental/provider/auth",
            get(handlers::v2_compat::empty_object).post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/provider/auth",
            get(handlers::v2_compat::empty_object).post(handlers::v2_compat::true_value),
        )
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
            "/experimental/worktree",
            get(handlers::v2_compat::empty_list)
                .post(handlers::v2_compat::empty_object)
                .delete(handlers::v2_compat::true_value),
        )
        .route(
            "/experimental/worktree/reset",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/experimental/worktree/:id",
            delete(handlers::v2_compat::true_value),
        )
        .route(
            "/experimental/worktree/:id/reset",
            post(handlers::v2_compat::true_value),
        )
        .route(
            "/api/experimental/worktree",
            get(handlers::v2_compat::empty_list).post(handlers::v2_compat::empty_object),
        )
        .route(
            "/api/experimental/worktree/:id",
            delete(handlers::v2_compat::true_value),
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
        .route(
            "/provider/:id/oauth/authorize",
            post(handlers::v2_compat::not_implemented),
        )
        .route(
            "/provider/:id/oauth/callback",
            post(handlers::v2_compat::not_implemented),
        )
        .route(
            "/api/provider/:id/oauth/authorize",
            post(handlers::v2_compat::not_implemented),
        )
        .route(
            "/api/provider/:id/oauth/callback",
            post(handlers::v2_compat::not_implemented),
        )
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
            "/api/integration/:name",
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
        // ─── SSE channels (P0.4) ───────────────────────────────
        .route("/event", get(sse::event_stream))
        .route("/global/event", get(sse::event_stream))
        .route("/api/event", get(sse::api_event_stream))
        // ─── Auth middleware (P0.7) ────────────────────────────
        .layer(middleware::from_fn(log_authorization_header))
        // ─── Tracing layer (P0.1) ──────────────────────────────
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
