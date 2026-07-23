//! Regression test: prove that `parallel(items, mapper)` with a mapper that
//! returns tables without a `prompt` key surfaces as a `ToolError`.
//!
//! NOTE: `WorkflowTool` was removed in a prior refactor (split into
//! WorkflowStartTool / WorkflowRuntime). These tests need to be rewritten
//! against the new API.
