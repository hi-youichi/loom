# Claude ACP Terminal 实现与 ACP 标准对比分析

> 基于 `json/claude_acp_terminal.json`（Claude 实际流量）、`acp-terminals-protocol.md`（ACP 标准）、`json/opencode_acp_terminal.json`（OpenCode 参考）三方对比。

---

## 1. 核心架构差异

### ACP 标准模式：独立的 terminal/* JSON-RPC 方法

标准定义了 5 个独立 JSON-RPC 方法，形成完整的终端生命周期：

```
Client (initialize: capabilities.terminal=true)
    │
    ▼
Agent ──► terminal/create ──► Client    返回 terminalId
Agent ──► terminal/output ──► Client    拉取输出
Agent ──► terminal/wait_for_exit ──► Client  阻塞等待
Agent ──► terminal/kill ──► Client      可选，终止
Agent ──► terminal/release ──► Client   释放资源
```

Agent 作为终端生命周期的**主动管理者**，显式调用每个阶段。

### Claude 模式：嵌入 tool_call 生命周期

Claude 不使用任何 `terminal/*` 方法，将终端执行完全嵌入 `session/update` 的 `tool_call` / `tool_call_update` 流中：

```
Agent ──► session/update(tool_call, Pending)
Agent ──► session/update(tool_call_update, Running)
Agent ──► session/update(tool_call_update, Completed + _meta.terminal_exit)
```

终端的生命周期隐含在 tool call 的 Pending → InProgress → Completed 状态变迁中，没有独立的 create/output/wait/release。

### OpenCode 模式：标准 tool_call + 结构化 rawOutput

OpenCode 也不用 `terminal/*` 方法，但严格遵循 ACP `tool_call_update` 标准字段：

```
Agent ──► session/update(tool_call, rawInput={})
Agent ──► session/update(tool_call_update, in_progress)
Agent ──► session/update(tool_call_update, completed + rawOutput={output, metadata})
```

---

## 2. 逐字段对比

### 2.1 tool_call 初始通知

| 字段 | ACP 标准 | Claude | OpenCode |
|------|---------|--------|----------|
| `sessionUpdate` | `"tool_call"` | `"tool_call"` | `"tool_call"` |
| `toolCallId` | 必填 | `"call_ef8756e..."` | `"call_function_pqh..."` |
| `title` | 必填 | `"Terminal"` | `"bash"` |
| `kind` | 必填 | `"execute"` | `"execute"` |
| `content` | 可选 | `[{"type":"terminal","terminalId":"call_ef..."}]` | 缺失 |
| `rawInput` | 可选 | `{}` | `{}` |
| `status` | ACP 标准 | 缺失（隐含 Pending） | 缺失（隐含 Pending） |
| `_meta.claudeCode` | 无 | `{"toolName":"Bash"}` | 无 |
| `_meta.terminal_info` | 无 | `{"terminal_id":"call_ef..."}` | 无 |

**差异点：**

1. **content type**：Claude 使用私有 `type: "terminal"` 内容类型，携带 `terminalId`。ACP 标准中 `tool_call` 的 content 通常不携带此类信息，OpenCode 初始通知中不设 content。

2. **`_meta` 扩展**：Claude 添加了 `claudeCode.toolName` 和 `terminal_info`，这些在 ACP 标准中不存在。

### 2.2 tool_call_update（输入更新）

| 字段 | ACP 标准 | Claude | OpenCode |
|------|---------|--------|----------|
| `sessionUpdate` | `"tool_call_update"` | `"tool_call_update"` | `"tool_call_update"` |
| `toolCallId` | 必填 | 同上 | 同上 |
| `kind` | 可选 | `"execute"` | `"execute"` |
| `title` | 可选 | `"pwd"` | `"bash"` |
| `content` | 可选 | `[{"type":"terminal","terminalId":"call_ef..."}]` | 缺失 |
| `rawInput` | 可选 | `{"command":"pwd","description":"..."}` | `{"command":"pwd","description":"..."}` |
| `status` | 可选 | 缺失 | `"in_progress"` |
| `_meta.claudeCode` | 无 | `{"toolName":"Bash"}` | 无 |

**差异点：**

1. **status**：OpenCode 明确设置 `status: "in_progress"`，Claude 不设 status（直到完成）。

### 2.3 tool_call_update（输出 / 完成）

Claude 的完成过程分三步通知：

**步骤 A — 工具响应（含 stdout/stderr）**
```json
{
  "sessionUpdate": "tool_call_update",
  "toolCallId": "call_ef...",
  "_meta": {
    "claudeCode": {
      "toolResponse": {
        "stdout": "/Users/apple/dev/loom/dev",
        "stderr": "",
        "interrupted": false,
        "isImage": false,
        "noOutputExpected": false
      },
      "toolName": "Bash"
    }
  }
}
```

**步骤 B — 终端输出**
```json
{
  "sessionUpdate": "tool_call_update",
  "toolCallId": "call_ef...",
  "_meta": {
    "terminal_output": {
      "terminal_id": "call_ef...",
      "data": "/Users/apple/dev/loom/dev"
    }
  }
}
```

**步骤 C — 终端退出 + 完成**
```json
{
  "sessionUpdate": "tool_call_update",
  "toolCallId": "call_ef...",
  "status": "completed",
  "content": [{"type": "terminal", "terminalId": "call_ef..."}],
  "rawOutput": "/Users/apple/dev/loom/dev",
  "_meta": {
    "claudeCode": {"toolName": "Bash"},
    "terminal_exit": {
      "terminal_id": "call_ef...",
      "exit_code": 0,
      "signal": null
    }
  }
}
```

对比 **OpenCode** 的单步完成：
```json
{
  "sessionUpdate": "tool_call_update",
  "toolCallId": "call_function_pqh...",
  "status": "completed",
  "title": "Print current working directory",
  "content": [{"type": "content", "content": {"type": "text", "text": "/Users/apple/dev/loom/dev\n"}}],
  "rawInput": {"command": "pwd", "description": "..."},
  "rawOutput": {
    "output": "/Users/apple/dev/loom/dev\n",
    "metadata": {
      "output": "/Users/apple/dev/loom/dev\n",
      "exit": 0,
      "description": "Print current working directory",
      "truncated": false
    }
  }
}
```

| 维度 | Claude | OpenCode | ACP 标准 |
|------|--------|----------|---------|
| 完成通知数 | 3 步 | 1 步 | 无规定 |
| `rawOutput` 类型 | 纯字符串 | 结构化对象 | 无规定 |
| 退出码位置 | `_meta.terminal_exit.exit_code` | `rawOutput.metadata.exit` | `terminal/wait_for_exit` 响应 |
| stdout/stderr | `_meta.claudeCode.toolResponse` | 无（合并到 output） | `terminal/output` 响应 |
| `truncated` 信息 | 无 | `rawOutput.metadata.truncated` | `terminal/output` 响应 |
| `content` 类型 | `type: "terminal"` | `type: "content"` | 标准内容类型 |

---

## 3. 终端生命周期管理

| 阶段 | ACP 标准 | Claude | OpenCode |
|------|---------|--------|----------|
| **创建** | `terminal/create` → 返回独立 terminalId | 复用 toolCallId 作为 terminalId | 不创建终端 |
| **获取输出** | `terminal/output` → `{output, exitStatus, truncated}` | `_meta.terminal_output.data` | 直接在 content 中返回 |
| **等待退出** | `terminal/wait_for_exit` → `{exitStatus}` | `_meta.terminal_exit.{exit_code, signal}` | `rawOutput.metadata.exit` |
| **终止** | `terminal/kill` | 不支持（tool call 取消？） | 不支持 |
| **释放** | `terminal/release`（必须调用） | 无显式释放 | 无显式释放 |
| **ID 格式** | `term_xyz789` | `call_ef8756e991ef47a298c5f631` | 不使用终端 ID |

---

## 4. `content` 类型扩展

### ACP 标准定义的内容类型

ACP `tool_call`/`tool_call_update` 的 `content` 字段标准定义了以下类型：

| type | 说明 |
|------|------|
| `content` | 通用内容块，包含 `{type: "text", text: "..."}` 或 `{type: "image", ...}` |
| `diff` | 文件差异 |

### Claude 私有扩展

Claude 添加了非标准类型：

```json
{"type": "terminal", "terminalId": "call_ef..."}
```

这个类型的语义是：**此 tool call 的可视化展示应使用内嵌终端组件**，由客户端（如 Zed）识别并渲染为交互式终端面板。

### loom-acp 适配

项目在 `loom-acp/src/content.rs` 中已定义对应枚举：

```rust
pub enum ToolCallContent {
    Content { content: ContentBlock },
    Diff { path, old_text, new_text },
    Terminal { terminal_id },  // Claude 私有扩展
}
```

---

## 5. `_meta` 扩展字段汇总

Claude 使用 `_meta` 传递所有终端特有信息。以下按通知阶段汇总：

### tool_call 阶段

| _meta 路径 | 类型 | 说明 |
|-----------|------|------|
| `claudeCode.toolName` | string | 固定 `"Bash"` |
| `terminal_info.terminal_id` | string | 终端 ID（= toolCallId） |

### tool_call_update（响应）阶段

| _meta 路径 | 类型 | 说明 |
|-----------|------|------|
| `claudeCode.toolResponse.stdout` | string | 标准输出 |
| `claudeCode.toolResponse.stderr` | string | 错误输出 |
| `claudeCode.toolResponse.interrupted` | bool | 是否被中断 |
| `claudeCode.toolResponse.isImage` | bool | 输出是否为图片 |
| `claudeCode.toolResponse.noOutputExpected` | bool | 是否无输出预期 |
| `claudeCode.toolName` | string | 固定 `"Bash"` |

### tool_call_update（终端输出）阶段

| _meta 路径 | 类型 | 说明 |
|-----------|------|------|
| `terminal_output.terminal_id` | string | 终端 ID |
| `terminal_output.data` | string | 终端输出数据 |

### tool_call_update（退出）阶段

| _meta 路径 | 类型 | 说明 |
|-----------|------|------|
| `claudeCode.toolName` | string | 固定 `"Bash"` |
| `terminal_exit.terminal_id` | string | 终端 ID |
| `terminal_exit.exit_code` | int | 退出码 |
| `terminal_exit.signal` | int\|null | 终止信号 |

---

## 6. 总结

| 维度 | ACP 标准 | Claude | OpenCode |
|------|---------|--------|----------|
| **终端方法** | `terminal/*` 5 个独立方法 | 不使用，嵌入 tool_call | 不使用，嵌入 tool_call |
| **content type** | `type: "content"` | `type: "terminal"` (私有) | `type: "content"` (标准) |
| **rawOutput** | — | 纯字符串 | 结构化对象 |
| **退出码** | `terminal/wait_for_exit.exitStatus` | `_meta.terminal_exit.exit_code` | `rawOutput.metadata.exit` |
| **terminalId** | 独立 ID（`term_*`） | 复用 toolCallId | 不使用 |
| **资源释放** | 显式 `terminal/release` | 隐含（tool call 完成） | 隐含 |
| **_meta 扩展** | 无 | 大量 `claudeCode` / `terminal_*` | 无 |
| **符合标准度** | — | 低（大量私有扩展） | 高（使用标准字段） |

### 关键结论

1. **Claude 的终端实现是私有协议**，核心信息全部通过 `_meta` 和 `type: "terminal"` content 传递，与 ACP 标准的 `terminal/*` 方法体系完全不同。

2. **OpenCode 更贴近 ACP 标准的 tool_call 模式**，使用标准字段 `rawOutput`（结构化）、标准 content type，不依赖 `_meta` 扩展。

3. **loom-acp 需要同时兼容两种模式**：
   - 支持 Claude 的 `type: "terminal"` content 解析（已实现：`ToolCallContent::Terminal`）
   - 支持 Claude 的 `_meta.terminal_*` 字段提取（用于获取输出和退出码）
   - 支持 OpenCode 的标准 `rawOutput.metadata` 路径
   - 适配层将两种模式统一为内部 `TerminalManager` 的 `terminal/create` / `terminal/output` / `terminal/wait_for_exit` 调用
