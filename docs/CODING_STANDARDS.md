# Code Standards

## Project Conventions

### Module Organization

#### 1. File Naming
- Modules: `snake_case.rs`
- Submodules: `snake_case/` directory with `mod.rs`
- Tests: inline `#[cfg(test)]` or `mod tests`

#### 2. Module Visibility
- Default to private (`mod`)
- Only use `pub` when explicitly part of public API
- Document why items are `pub(crate)` or `pub`

**Current Issue**: `cli/src/run/mod.rs` has 15/20 modules as `pub` with no clear pattern.

```rust
// Good: explicit visibility
mod agent;          // private: internal implementation
mod display;        // private: display formatting internals
pub mod curator;    // public: part of CLI interface
pub mod panel_format; // public: part of CLI interface
```

#### 3. Re-export Organization
- Avoid flat mega-re-exports in `lib.rs`
- Group related exports into `prelude` modules
- Use feature flags for optional components

**Current Issue**: `loom/src/lib.rs` has 72 lines of direct re-exports.

**Recommended Structure**:
```rust
// lib.rs
pub mod agent;
pub mod graph;
pub mod llm;

pub mod prelude;        // Common types for library users
pub mod prelude::cli;   // CLI-specific types
pub mod prelude::graph; // Graph execution types
```

### Naming Conventions

| Type | Convention | Example |
|------|------------|---------|
| Struct | PascalCase | `ReActState`, `RunOptions` |
| Enum | PascalCase | `Next`, `RunError` |
| Enum Variant | PascalCase | `Next::Continue` |
| Function | snake_case | `build_helve_config` |
| Method | snake_case | `invoke_with_context` |
| Constant | SCREAMING_SNAKE | `MAX_CONSECUTIVE_FAILURES` |
| Module | snake_case | `cli_run`, `agent_tool` |
| Field | snake_case | `runnable_config`, `stream_tx` |
| Trait | PascalCase | `LlmProvider`, `Checkpointer` |
| Generic | PascalCase | `S`, `T`, `Ctx` |

### Function Design

#### 1. Parameter Limits
- Maximum 5 parameters for a function
- Beyond 5, use a config struct or builder

**Current Issue**: `ReactRunner::new()` has 13 positional parameters (compiled.rs:37).

**Good Pattern**:
```rust
// Option 1: Config struct
pub struct ReactRunnerConfig {
    pub provider: Arc<dyn LlmProvider>,
    pub tool_source: Box<dyn ToolSource>,
    pub system_prompt: String,
    pub checkpointer: Option<Arc<dyn Checkpointer<ReActState>>>,
    pub store: Option<Arc<dyn Store>>,
    // Optional fields with defaults
    pub compaction_config: Option<CompactionConfig>,
    pub cancellation: Option<CancellationToken>,
    pub verbose: bool,
}

impl Default for ReactRunnerConfig {
    fn default() -> Self {
        Self {
            compaction_config: None,
            cancellation: None,
            verbose: false,
            // ... other defaults
        }
    }
}

// Option 2: Builder pattern
pub struct RunOptionsBuilder {
    model: Option<ModelConfig>,
    max_iterations: Option<usize>,
    // ...
}
```

#### 2. Function Length
- Target: < 50 lines
- Maximum: < 100 lines
- Split larger functions into named helper functions

#### 3. Early Returns
- Use early returns to avoid deep nesting
- Guard clauses preferred over single-exit-point

```rust
// Good: early return for validation
pub fn process_input(input: &str) -> Result<Output, Error> {
    if input.is_empty() {
        return Err(Error::EmptyInput);
    }
    // main logic
}

// Bad: deep nesting
pub fn process_input(input: &str) -> Result<Output, Error> {
    if !input.is_empty() {
        if let Some(data) = parse(input) {
            if data.is_valid() {
                // 100 lines of logic
            }
        }
    }
}
```

### Error Handling

#### 1. Error Types
- Use `thiserror` for application errors
- Use `anyhow` for library-internal errors requiring context
- Distinguish between recoverable and unrecoverable errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("compilation failed: {0}")]
    Compilation(#[from] CompilationError),
    
    #[error("checkpoint error: {0}")]
    Checkpoint(String),
    
    #[error("execution failed: {0}")]
    Execution(String),
}
```

#### 2. Error Propagation
- Use `?` operator for error propagation
- Avoid `.unwrap()` and `.expect()` in production code
- Use `Option::ok()` or `Result::ok()` for explicit None/Error discarding

**Forbidden**:
```rust
// Never use in production
let value = mutex.lock().unwrap();
let config = some_fn().unwrap();
```

**Allowed (with clear intent)**:
```rust
// Explicit panic for unrecoverable errors
let config = std::env::var("REQUIRED_VAR").expect("REQUIRED_VAR must be set");

// Or handle gracefully
let value = mutex.lock().map_err(|_| Error::LockPoisoned)?;
```

#### 3. Error Conversion
- Implement `From` for error conversions
- Use `.into()` for automatic conversion

### Async Code

#### 1. Async Traits
- Use `async_trait` crate for async methods in traits
- Mark trait bounds as `Send + Sync` when thread-safety is required

```rust
use async_trait::async_trait;

#[async_trait]
pub trait GraphRunner<S> {
    async fn invoke(&self, state: S) -> Result<S, RunError>;
    async fn stream_with_config(
        &self,
        state: S,
        config: StreamingConfig,
    ) -> Result<S, RunError>;
}
```

#### 2. Cancellation
- Use `CancellationToken` for graceful cancellation
- Check cancellation at loop boundaries
- Propagate cancellation errors clearly

#### 3. Spawning Tasks
- Always `.await` or `.detach()` spawned tasks
- Consider structured concurrency with `tokio::task::JoinSet`

### Comments

#### 1. When to Comment
- **WHY**: Explain reasoning, constraints, trade-offs
- **FIXME/TODO**: Mark known issues
- **SAFETY**: Document unsafe blocks

#### 2. When NOT to Comment
- **WHAT**: Don't restate the code
- Obvious code that's self-documenting

**Bad Examples**:
```rust
// Get the user from the database
let user = db.get_user(id)?;

// Call the function
execute_node(node)?;

// Check if value exists
if let Some(val) = value { ... }
```

**Good Examples**:
```rust
// Retain the most recent checkpoint to allow recovery from partial failures.
// Older checkpoints are pruned to bound memory usage.
let old_checkpoint = checkpoints.pop_front();

// Use a barrier to ensure all parallel branches complete before merging.
// Without this, we'd risk deadlocks from partial state.
join!(branch_a, branch_b);
```

### Testing

#### 1. Test Organization
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_case() {
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    fn test_edge_case_empty() {
        assert_eq!(process(""), Ok(vec![]));
    }

    #[tokio::test]
    async fn test_async_behavior() {
        let result = async_operation().await;
        assert!(result.is_ok());
    }
}
```

#### 2. Test Naming
- Format: `test_<subject>_<scenario>_<expected>`
- Examples: `test_react_runner_empty_messages`, `test_checkpoint_save_overwrites`

#### 3. Test Coverage
- Aim for > 80% coverage on critical paths
- Test error paths explicitly
- Include property-based tests for complex transformations

### Code Review Checklist

- [ ] No `unwrap()` without clear justification
- [ ] No `expect()` for recoverable errors
- [ ] Function parameters < 5
- [ ] Function length < 100 lines
- [ ] Comments explain WHY, not WHAT
- [ ] Error types use `thiserror`
- [ ] Async functions properly await all spawned tasks
- [ ] Public API has doc comments
- [ ] No direct imports from internal modules
- [ ] Tests cover happy path AND error paths