## v0.2.0 — Workspace File Browser & Dev Bots

### Features
- **Workspace File API**: New WebSocket protocol for listing and reading workspace files
  - `workspace_file_list`: List directory entries with security validation
  - `workspace_file_read`: Read file content within workspace root
  - Hidden files excluded, folders sorted first
- **Web File Browser**: Real workspace file tree with lazy loading
  - `useWorkspaceFiles` hook with directory caching
  - `TabBar` component for multi-tab navigation
  - `FileContentView` for reading files in browser
  - Dashboard as default tab, file tabs closeable
- **New Dev Bots**:
  - MCP Dev Bot (`@mcpdevbot`): For MCP protocol development
  - Loom Dev Bot (`@loomdevelopbot`): For framework development with cargo cache

### Backend
- Protocol types: `WorkspaceFileListRequest/Response`, `WorkspaceFileReadRequest/Response`, `FileEntry`
- Path traversal protection on file operations
- E2E test suite for file list and read operations

### Bug Fixes
- Fix telegram-bot test compilation (StreamCommand, StreamingConfig, InteractionMode)
- Clippy idioms (`is_some_and`)

### Infrastructure
- Docker Compose: mcp-dev-bot and loom-dev-bot services
- Loom Dev Bot: port 9000, cargo cache volumes
- Version bump 0.1.7 -> 0.2.0
