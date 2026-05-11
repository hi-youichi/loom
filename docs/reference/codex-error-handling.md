---
sidebar_position: 5
title: "Codex 异常处理"
description: "Codex 事件流中 turn.failed、error 和 item 级失败的处理策略、事件格式和消费端指南"
---

# Codex 异常处理

Codex 事件流定义了三个层级的异常，每个层级有不同的语义和消费端处理策略。

## 异常分类

| 层级 | 事件 | 语义 | 流行为 |
|------|------|------|--------|
| Turn 级 | `turn.failed` | 当前轮次 LLM 调用失败 | 流**不中断**，可能重试 |
| 全局级 | `error` | 不可恢复错误 | 流**终止**，消费端停止 |
| Item 级 | item `.status = "failed"` | 单个工具调用失败 | 流**不中断**，继续下一个 item |

## 事件格式

### turn.failed — LLM 调用失败

替代 `turn.completed`，表示该 turn 未能产出结果：

```json
{"type":"turn.started"}
{"type":"turn.failed","error":{"message":"Model request failed: rate limit exceeded"}}
```

触发场景：

- HTTP 429 — rate limit
- HTTP 5xx — 模型服务端错误
- 超时 / 网络断连
- 响应内容为空（重试耗尽）

不触发场景：

- 工具执行失败 → item 级处理
- 首次请求即认证失败 → 全局 `error`

### error — 不可恢复错误

流终止信号，消费端收到后**不应处理后续事件**：

```json
{"type":"error","message":"Authentication failed: invalid API key"}
```

触发场景：

- API key 无效 / 账户欠费（首个请求即失败）
- graph 编译失败（`CompilationError`）
- checkpoint 存储不可用（`CheckpointError`）
- 配置文件损坏 / 缺失必要字段

### item 级失败 — 工具执行错误

已支持 ✓，`session cat` 可完整重建。

**mcp_tool_call 失败**：

```json
{"type":"item_started","item":{"id":"item_3","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"/foo"},"result":null,"error":null,"status":"in_progress"}}
{"type":"item_completed","item":{"id":"item_3","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"/foo"},"result":null,"error":{"message":"file not found: /foo"},"status":"failed"}}
```

**command_execution 失败**：

```json
{"type":"item_started","item":{"id":"item_4","type":"command_execution","command":"make","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item_completed","item":{"id":"item_4","type":"command_execution","command":"make","aggregated_output":"make: *** No targets specified","exit_code":2,"status":"completed"}}
```

> `command_execution` 用 `exit_code != 0` + `status: "completed"` 表示命令执行失败。只有工具层面报错（如无法启动进程）才用 `status: "failed"`。

## 异常流示例

### Rate limit 后重试成功

```json
{"type":"thread_started","thread_id":"session-abc"}
{"type":"turn.started"}
{"type":"turn.failed","error":{"message":"Model request failed: rate limit exceeded"}}
{"type":"turn_started"}
{"type":"item_started","item":{"id":"item_0","type":"agent_message","text":"Let me help..."}}
{"type":"item_completed","item":{"id":"item_0","type":"agent_message","text":"Let me help..."}}
{"type":"turn_completed","usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"reasoning_output_tokens":0}}
```

消费端处理：`turn.failed` 后不应退出，等待后续 `turn.started` 的重试结果。

### 工具链中有失败项

```json
{"type":"thread_started","thread_id":"session-def"}
{"type":"turn_started"}
{"type":"item_started","item":{"id":"item_0","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"/missing"},"result":null,"error":null,"status":"in_progress"}}
{"type":"item_completed","item":{"id":"item_0","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"/missing"},"result":null,"error":{"message":"file not found: /missing"},"status":"failed"}}
{"type":"item_started","item":{"id":"item_1","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"/exists"},"result":null,"error":null,"status":"in_progress"}}
{"type":"item_completed","item":{"id":"item_1","type":"mcp_tool_call","server":"loom","tool":"read","arguments":{"path":"/exists"},"result":{"content":[{"type":"text","text":"file content"}],"structured_content":null},"error":null,"status":"completed"}}
{"type":"item_started","item":{"id":"item_2","type":"agent_message","text":"第一个工具失败了，但第二个成功..."}}
{"type":"item_completed","item":{"id":"item_2","type":"agent_message","text":"第一个工具失败了，但第二个成功..."}}
{"type":"turn_completed","usage":{"input_tokens":200,"cached_input_tokens":0,"output_tokens":100,"reasoning_output_tokens":30}}
```

消费端处理：单个 item 失败不影响后续 item，由 LLM 决定如何处理失败结果。

### 认证失败（致命）

```json
{"type":"thread_started","thread_id":"session-ghi"}
{"type":"turn.started"}
{"type":"error","message":"Authentication failed: invalid API key"}
```

消费端处理：流到此结束，无 `turn.completed` / `turn.failed`，应终止并报告错误。

## Loom 中的错误来源映射

| Loom 错误类型 | Codex 事件 | 说明 |
|---------------|-----------|------|
| `AgentError::ExecutionFailed(msg)` | `turn.failed` | LLM 调用失败 |
| `RunError::Compilation(e)` | `error` | graph 编译失败 |
| `RunError::Checkpoint(e)` | `error` | 存储层错误 |
| `RunError::StreamEndedWithoutState` | `error` | 流异常终止 |
| Tool message `is_error=true` | item `status: "failed"` | 工具执行失败 |

## 实现阶段

### Phase 1 — checkpoint 重建（当前）

| 异常类型 | 支持情况 | 说明 |
|---------|---------|------|
| item 级失败 | ✅ 已支持 | 从 Tool 消息重建 |
| `turn.failed` | ❌ 未支持 | checkpoint 不记录 LLM 错误 |
| `error` | ❌ 未支持 | 运行时直接返回，不产生 checkpoint |

降级策略：最后一个 checkpoint 无 `usage` 且无 Assistant 回复时，emit `turn.failed`：

```json
{"type":"turn.failed","error":{"message":"turn did not complete (checkpoint may be incomplete)"}}
```

## 相关文档

- [Codex 事件协议字段参考](/docs/reference/codex-event-protocol) — 完整的事件类型和字段定义
- [session cat — 会话回放](/docs/deployment/cli-session-cat) — 会话回放命令使用指南
- [CLI JSON 流式输出](/docs/deployment/cli-json-output) — 运行时的 `--json` 事件流
