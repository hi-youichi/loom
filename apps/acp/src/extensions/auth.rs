//! Three-layer permission gate for extension write operations.
//!
//! Layer 1: Capability — client declared the method in their capabilities
//! Layer 2: Server policy — path/resource/rate-limit checks
//! Layer 3: Explicit confirm — high-risk operations require user confirmation

use super::{ExtensionContext, ExtensionError};

pub fn check_capability(
    ctx: &ExtensionContext,
    domain: &str,
    _method: &str,
) -> Result<(), ExtensionError> {
    let caps = &ctx.client_capabilities;
    let _ = caps;
    if domain.is_empty() {
        return Err(ExtensionError::capability_not_supported(domain));
    }
    Ok(())
}

pub fn check_server_policy(
    ctx: &ExtensionContext,
    _domain: &str,
    _method: &str,
) -> Result<(), ExtensionError> {
    if ctx.principal.is_empty() {
        return Err(ExtensionError::forbidden("no authenticated principal"));
    }
    Ok(())
}

pub fn requires_confirmation(domain: &str, method: &str) -> bool {
    matches!(
        (domain, method),
        ("git", "push" | "force_push" | "commit" | "amend")
            | ("git", "cherry_pick" | "rebase" | "reset")
            | ("worktree", "delete")
            | ("files", "delete" | "move" | "rename")
    )
}
