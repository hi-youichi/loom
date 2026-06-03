//! Approval policy for destructive or high-risk file operations.

/// Approval policy for destructive or high-risk file operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// No approval; agent may execute all file operations.
    None,
    /// Require approval only for destructive operations (e.g. delete_file).
    DestructiveOnly,
    /// Require approval for destructive and bulk write operations before executing.
    Always,
}

/// Event type for Custom stream events and Interrupt value when approval is required.
pub const APPROVAL_REQUIRED_EVENT_TYPE: &str = "approval_required";

/// Returns the list of tool names that require user approval for the given policy.
pub fn tools_requiring_approval(policy: ApprovalPolicy) -> &'static [&'static str] {
    match policy {
        ApprovalPolicy::None => &[],
        ApprovalPolicy::DestructiveOnly => &["delete_file"],
        ApprovalPolicy::Always => &["delete_file", "write_file"],
    }
}
