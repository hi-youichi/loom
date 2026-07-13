//! HTTP handlers, organised by route group (tasks P0.2, P0.3, P1.x, P2.x).
//!
//! Submodule layout follows the opencode v2 external-kernel spec
//! (`external-kernel-guide.md` §"HTTP/SSE 协议目录"), so it stays
//! obvious which file owns each route:
//!
//! | Module               | Route prefix(es)                          | Tasks  |
//! |----------------------|-------------------------------------------|--------|
//! | [`bootstrap`]        | /config, /provider, /agent, /path, ...    | P0.2/P0.3 |
//! | [`session`]          | /session, /api/session                    | P1.8-16 |
//! | [`messages`]         | /session/:id/messages, /api/.../message   | P1.12, P1.15 |
//! | [`permission`]       | /permission, /api/permission              | P2.18 |
//! | [`question`]         | /question, /api/question                  | P2.18 |
//! | [`mcp_pty_file`]     | /mcp, /pty, /file, /find                  | P2.20 |
//! | [`instance`]         | /instance, /api/instance                  | P2.20 |
//! | [`experimental`]     | /experimental/*                           | P2.21 |
//! | [`provider_auth`]    | /provider/auth                            | P2.22 |
//! | [`global_bus`]       | /global/*                                 | P2.23 |
//! | [`control`]          | /tui/*                                    | P1.16 |
//! | [`revert`]           | /session/:id/revert/*                     | P2.19 |
//! | [`lsp_formatter`]    | /lsp/status, /formatter/status            | P0.3   |
//! | [`health`]           | /api/health, /api/location                | P0.2   |
//!
//! All handlers return JSON in the opencode envelope — `{ ... }` for
//! unstructured shapes, `{ data: ... }` for v2 SDK operations, and
//! empty `{}` for irrelevant stub endpoints (per the spec's "data may
//! be `null`" allowance for unused capabilities).

pub mod bootstrap;
pub mod control;
pub mod experimental;
pub mod global_bus;
pub mod health;
pub mod instance;
pub mod lsp_formatter;
pub mod mcp_pty_file;
pub mod messages;
pub mod permission;
pub mod provider_auth;
pub mod question;
pub mod revert;
pub mod session;
pub mod v2_compat;
pub mod vcs_extra;
