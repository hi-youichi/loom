//! Tool name constants for all built-in Loom tools.
//!
//! These constants are the single source of truth for tool names used across
//! the workspace. All tool implementations should import from this module
//! rather than defining their own local constants.

// ============================================================================
    // File tools
    // ============================================================================

    /// read — read text content of a file under the working folder.
    pub const TOOL_READ_FILE: &str = "read";

    /// write_file — write text content to a file under the working folder.
    pub const TOOL_WRITE_FILE: &str = "write_file";

    /// ls — list files and directories in a given path.
    pub const TOOL_LS: &str = "ls";

    /// grep — search file contents using regular expressions.
    pub const TOOL_GREP: &str = "grep";

    /// glob — fast file pattern matching.
    pub const TOOL_GLOB: &str = "glob";

    /// edit — apply a single find-and-replace to a file.
    pub const TOOL_EDIT_FILE: &str = "edit";

    /// delete_file — delete a file or empty directory.
    pub const TOOL_DELETE_FILE: &str = "delete_file";

    /// multiedit — apply multiple find-and-replace edits to a single file.
    pub const TOOL_MULTIEDIT: &str = "multiedit";

    /// move_file — move or rename a file or directory.
    pub const TOOL_MOVE_FILE: &str = "move_file";

    /// create_dir — create a directory; parent directories are created if needed.
    pub const TOOL_CREATE_DIR: &str = "create_dir";

    /// apply_patch — apply a multi-file patch in opencode format.
    pub const TOOL_APPLY_PATCH: &str = "apply_patch";

    // ============================================================================
    // Task tools
    // ============================================================================

    /// task_create — create a new task in the task management system.
    pub const TOOL_TASK_CREATE: &str = "task_create";

    /// task_show — show a task by its ID or prefix.
    pub const TOOL_TASK_SHOW: &str = "task_show";

    /// task_list — list tasks with optional filters, sorting, and pagination.
    pub const TOOL_TASK_LIST: &str = "task_list";

    /// task_update — update an existing task (status, name, description, etc.).
    pub const TOOL_TASK_UPDATE: &str = "task_update";

    /// task_delete — delete a task by ID.
    pub const TOOL_TASK_DELETE: &str = "task_delete";

    // ============================================================================
    // Memory tools
    // ============================================================================

    /// remember — store information in long-term memory.
    pub const TOOL_REMEMBER: &str = "remember";

    /// recall — retrieve a specific memory by ID.
    pub const TOOL_RECALL: &str = "recall";

    /// search_memories — search memory store with query.
    pub const TOOL_SEARCH_MEMORIES: &str = "search_memories";

    /// list_memories — list all stored memories.
    pub const TOOL_LIST_MEMORIES: &str = "list_memories";

    // ============================================================================
    // Shell / execution tools
    // ============================================================================

    /// bash — run a shell command (Unix).
    pub const TOOL_BASH: &str = "bash";

    /// powershell — run a PowerShell command (Windows).
    pub const TOOL_POWERSHELL: &str = "powershell";

    // ============================================================================
    // Web / external tools
    // ============================================================================

    /// web_fetcher — HTTP GET/POST requests to URLs.
    pub const TOOL_WEB_FETCHER: &str = "web_fetcher";

    /// websearch — search the web using Exa.
    pub const TOOL_WEBSEARCH: &str = "websearch";

    /// codesearch — search code using Exa.
    pub const TOOL_CODESEARCH: &str = "codesearch";

    /// twitter_search — search Twitter/X.
    pub const TOOL_TWITTER_SEARCH: &str = "twitter_search";

    // ============================================================================
    // Other tools
    // ============================================================================

    /// date — returns the current date and time.
    pub const TOOL_DATE: &str = "date";

    /// lsp — Language Server Protocol for code intelligence.
    pub const TOOL_LSP: &str = "lsp";

    /// skill_list — list all available skills with descriptions.
    pub const TOOL_SKILL_LIST: &str = "skill_list";

    /// skill_view — load a skill's full content by name.
    pub const TOOL_SKILL_VIEW: &str = "skill_view";

    /// help — show help information about Loom, Skills, and MCP.
    pub const TOOL_HELP: &str = "help";

    /// batch — execute multiple independent tool calls in parallel.
    pub const TOOL_BATCH: &str = "batch";

    /// invoke_agent — delegate work to a sub-agent by profile name.
    pub const TOOL_INVOKE_AGENT: &str = "invoke_agent";

    /// git_worktree — manage git worktrees for isolated parallel execution.
    pub const TOOL_GIT_WORKTREE: &str = "git_worktree";

    // ============================================================================
    // Todo tools
    // ============================================================================

    /// todo_read — read the current to-do list for the session.
    pub const TOOL_TODO_READ: &str = "todo_read";

    /// todo_write — create and manage a structured task list.
    pub const TOOL_TODO_WRITE: &str = "todo_write";

    // ============================================================================
    // Experimental tools
    // ============================================================================

    /// llm — direct LLM invocation with multimodal inputs and provider/model
    /// discovery (queries models.dev for pricing and capabilities).
    pub const TOOL_LLM: &str = "llm";
