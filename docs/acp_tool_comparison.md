# Agent Client Protocol (ACP) Tool 对比分析

## 1. ACP 协议概述

### 什么是 ACP?

Agent Client Protocol (ACP) 是一个标准化协议，用于**代码编辑器**和**AI 编码代理**之间的通信。

- **类比**: ACP 对于 AI 编码代理，就像 LSP (Language Server Protocol) 对于语言服务器
- **目标**: 将 M×N 的集成问题变成 M+N
- **创建者**: JetBrains 和 Zed 合作开发

### 核心概念

```
┌─────────────┐         ACP          ┌─────────────┐
│   Client    │ ◄──────────────────► │    Agent    │
│  (Editor)   │      JSON-RPC        │  (AI Bot)   │
└─────────────┘                       └─────────────┘
     │                                      │
     │  - session/start                     │  - prompt
     │  - session/load                      │  - tool calls
     │  - session/stop                      │  - file operations
     │  - prompt                            │  - terminal
     └──────────────────────────────────────┘
```

---

## 2. ACP Tool 相关规范

### 2.1 Agent Capabilities

```json
{
  "AgentCapabilities": {
    "loadSession": false,
    "mcpCapabilities": {
      "http": false,
      "sse": false
    }
  }
}
```

### 2.2 工具调用流程

ACP 本身不定义具体的工具规范，而是依赖 **MCP (Model Context Protocol)** 来提供工具能力：

```
ACP (通信层) + MCP (工具层)
```

### 2.3 MCP 集成

ACP 支持 MCP 能力:
- `mcpCapabilities.http` - HTTP 传输
- `mcpCapabilities.sse` - Server-Sent Events

---

## 3. 当前 Loom 实现分析

### 3.1 Tool Trait

```rust
// loom/src/tools/trait.rs
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn spec(&self) -> ToolSpec;
    async fn call(
        &self,
        args: serde_json::Value,
        ctx: Option<&ToolCallContext>,
    ) -> Result<ToolCallContent, ToolSourceError>;
}
```

### 3.2 ToolSpec

```rust
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,  // JSON Schema
    pub output_hint: Option<String>,
}
```

### 3.3 已实现的工具

| 类别 | 工具 | 说明 |
|------|------|------|
| 文件操作 | read, write, edit, glob, grep, ls, delete, move, apply_patch, multiedit | 文件系统操作 |
| Shell | bash | 执行命令 |
| 搜索 | exa_codesearch, exa_websearch | 网络搜索 |
| 网络获取 | web_fetcher | HTTP 请求 |
| 记忆 | remember, recall, search_memories, list_memories | 持久化记忆 |
| Todo | todo_read, todo_write | 任务管理 |
| 对话 | get_recent_messages | 获取上下文 |
| Agent | invoke_agent | 子代理调用 |
| 技能 | skill | 加载技能 |
| LSP | lsp | 语言服务器集成 |
| Twitter | twitter_search | 社交媒体 |
| 批量 | batch | 批量执行 |

---

## 4. 对比分析

### 4.1 架构对比

| 方面 | ACP/MCP | Loom |
|------|---------|------|
| **协议层** | ACP (JSON-RPC) | 自定义 (内部调用) |
| **工具层** | MCP | Tool trait |
| **传输** | stdio, HTTP, SSE | 函数调用 |
| **标准化** | 开放协议 | 私有实现 |

### 4.2 Tool 定义对比

**MCP Tool 定义:**
```json
{
  "name": "string",
  "description": "string",
  "inputSchema": {
    "type": "object",
    "properties": {...},
    "required": [...]
  }
}
```

**Loom ToolSpec:**
```rust
pub struct ToolSpec {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,  // JSON Schema
    pub output_hint: Option<String>,
}
```

**结论**: 基本一致，Loom 增加了 `output_hint`

### 4.3 Tool 调用对比

**MCP:**
```json
{
  "method": "tools/call",
  "params": {
    "name": "tool_name",
    "arguments": {...}
  }
}
```

**Loom:**
```rust
tool.call(args, ctx).await
```

---

## 5. 差距分析

### 5.1 当前 Loom 缺少的 ACP 能力

| 能力 | ACP | Loom | 优先级 |
|------|-----|------|--------|
| 标准化协议 | ✅ | ❌ | 高 |
| 会话持久化 | ✅ | ✅ | - |
| 多客户端支持 | ✅ | ❌ | 中 |
| 标准化工具发现 | ✅ | ✅ | - |
| 资源管理 | ✅ | ❌ | 低 |

### 5.2 当前 Loom 优势

| 方面 | Loom | 说明 |
|------|------|------|
| 内置工具丰富 | ✅ | 20+ 工具 |
| MCP 兼容 | ✅ | mcp_adapter |
| 记忆系统 | ✅ | SQLite 持久化 |
| 子代理 | ✅ | invoke_agent |
| 技能系统 | ✅ | 可扩展 |

---

## 6. 改进建议

### 6.1 短期 (Phase 1)

1. **MCP 完整兼容**
   - 完善 `mcp_adapter.rs`
   - 支持所有 MCP 工具类型

2. **Tool Spec 标准化**
   ```rust
   // 确保与 MCP schema 完全兼容
   pub struct ToolSpec {
       pub name: String,
       pub description: Option<String>,
       pub input_schema: Value,
       // 移除 output_hint 或放入 _meta
   }
   ```

### 6.2 中期 (Phase 2)

1. **ACP 服务端实现**
   ```rust
   // 新增 loom-acp crate
   pub struct AcpServer {
       tools: ToolRegistryLocked,
       sessions: SessionManager,
   }
   
   impl AcpServer {
       pub async fn handle_session_start(&self, ...) {}
       pub async fn handle_prompt(&self, ...) {}
       pub async fn handle_tool_call(&self, ...) {}
   }
   ```

2. **标准化传输**
   - stdio 传输
   - HTTP/SSE 传输

### 6.3 长期 (Phase 3)

1. **IDE 集成**
   - VS Code 插件
   - Zed 集成
   - JetBrains 插件

2. **多客户端支持**
   - 会话共享
   - 状态同步

---

## 7. 实施路线图

```
Phase 1 (1-2周)
├── MCP 完整兼容
├── Tool Spec 标准化
└── 测试覆盖

Phase 2 (2-4周)
├── ACP 服务端框架
├── stdio 传输
└── 基本协议实现

Phase 3 (1-2月)
├── HTTP/SSE 传输
├── IDE 集成
└── 文档完善
```

---

## 8. 参考资料

- [ACP 官网](https://agentclientprotocol.com)
- [ACP GitHub](https://github.com/agentclientprotocol/agent-client-protocol)
- [MCP 规范](https://modelcontextprotocol.io)
- [acpx CLI](https://github.com/openclaw/acpx)
