# 方案：OpenAI 兼容的多轮工具对话格式

## 1. 背景与问题

在相同 ReAct 流程下，使用 **智谱 BigModel**（`ChatBigModel`）时工具链路可正常工作，而使用 **OpenAI Chat Completions**（`ChatOpenAI`）时容易出现：

- 首轮或后续轮次 **API 返回 400**（消息序列不合法）；
- 或模型 **无法稳定继续多轮工具调用**。

现象并非必然来自 `openai.rs` / `bigmodel.rs` 中「是否附带 `tools` 列表」的差异：ReAct 构建路径里两者都会 `with_tools`。根因在于 **对话历史在 HTTP 请求中的编码方式** 与 **OpenAI 的校验规则** 不一致。

## 2. 现状摘要

| 环节 | 当前行为 |
|------|----------|
| `Message` | 仅 `System` / `User` / `Assistant` 三种，内容为字符串。 |
| Think 落盘 | `apply_think_response` 只追加 `Message::Assistant(content)`，**不保存**当轮 `tool_calls`（含 `id`）。 |
| Observe 落盘 | 将每条 `ToolResult` 格式化为 `User` 文案：`Tool {name} result/error:\n{observation}`。 |
| `ChatOpenAI` | `messages_to_request` 仅映射为带文本的 assistant，**不发出** `tool_calls` / `tool` 角色。 |
| `ChatBigModel` | 同样只发 `role` + `content` 字符串；依赖网关对非严格格式的容忍。 |

相关代码位置（便于对照）：

- `loom/src/message.rs` — `Message` 定义与 `assistant_content_for_chat_api`
- `loom/src/agent/react/think_node.rs` — `apply_think_response`
- `loom/src/agent/react/observe_node.rs` — 工具结果写入 `messages`
- `loom/src/llm/openai.rs` — `messages_to_request`
- `loom/src/llm/bigmodel.rs` — `messages_to_request`

## 3. 根因说明

[OpenAI Chat Completions](https://platform.openai.com/docs/api-reference/chat/create) 要求：

1. 若某条 **assistant** 消息表示「模型选择调用工具」，则该条必须包含 **`tool_calls`**（含 `id`、`function.name`、`function.arguments`）；`content` 可为空。
2. 紧随其后的每条工具输出必须是 **`role: "tool"`**，且 **`tool_call_id`** 与上面对应。

当前实现把工具输出写成 **`User`**，且历史中的 assistant **没有** `tool_calls`，等价于：

- 「空 content、且无 tool_calls 的 assistant」后接一条 user — 违反 OpenAI 规则；
- 或模型侧无法把「Tool xxx result」与官方 `tool` 轮次对齐。

### 3.1 HTTP 请求案例

以下为 **第二轮 Think**（已完成一次工具执行并把结果写回 `messages` 之后）再次调用 Chat Completions 时，`POST /v1/chat/completions` 请求体中与 **`messages`** 相关的片段。`model`、`tools`、`temperature` 等与首轮类似，此处省略。

**（1）OpenAPI 规范期望的历史片段** — assistant 重放当轮的 `tool_calls`，工具输出用 `role: "tool"` 且带上 `tool_call_id`：

```http
POST /v1/chat/completions HTTP/1.1
Host: api.openai.com
Authorization: Bearer sk-...
Content-Type: application/json
```

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "现在几点？" },
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [
        {
          "id": "call_abc123",
          "type": "function",
          "function": {
            "name": "get_time",
            "arguments": "{}"
          }
        }
      ]
    },
    {
      "role": "tool",
      "tool_call_id": "call_abc123",
      "content": "{\"iso\":\"2025-03-25T12:00:00Z\"}"
    }
  ]
}
```

**（2）当前 Loom 在同类场景下更易形成的片段** — assistant 只有文本（常为 `""`），工具结果被拼进 **user**，且 **没有** `tool_calls` / `tool`：

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "现在几点？" },
    { "role": "assistant", "content": "" },
    {
      "role": "user",
      "content": "Tool get_time result:\n{\"iso\":\"2025-03-25T12:00:00Z\"}"
    }
  ]
}
```

OpenAI 对 **（2）** 常见反应包括：`400` 与 *"assistant message must have either content or tool_calls"* 一类错误，或后续轮次模型行为异常。 **（1）** 与规范一致，多轮工具可继续。

智谱等 **OpenAI 兼容** 接口往往校验较松，同样的「User 伪装工具结果」仍能跑通，因此出现 **BigModel 正常、OpenAI 异常** 的对比。

补充：`message.rs` 中注释提到空 assistant 可用 WORD JOINER 占位，但若实现仍返回空字符串，会加剧「无 content、无 tool_calls」的非法组合；**仅靠占位符不能替代在请求中重放 `tool_calls`**。

## 4. 目标与非目标

### 目标

- 对 **OpenAI 及严格兼容的实现**，请求体符合 **assistant `tool_calls` + `tool` 消息** 规范。
- 多轮 ReAct（think → act → observe → think）在 OpenAI 上可稳定连续调用工具。
- 在可行范围内 **保持** BigModel 等行为不变或仅做向后兼容扩展。

### 非目标

- 不在本文档内规定具体模型名或 `tool_choice` 策略（仍由现有配置与产品决策决定）。
- 不强制一次性迁移所有持久化/序列化格式（可分阶段）。

## 5. 方案设计

### 5.1 扩展对话消息模型（推荐）

在 `Message`（或并列的「LLM 线程消息」类型）中显式表达工具轮次，例如：

- **`Assistant`**：保留可读 `content`；增加可选字段 **`tool_calls: Vec<AssistantToolCall>`**（`id`、`name`、`arguments` 字符串），与 `LlmResponse.tool_calls` / `state.tool_calls` 对齐。
- **`Tool`**：`tool_call_id`、`content`（字符串，一般为 JSON 或观测文本；与当前 `ToolResult.observation()` 一致即可）。

可选：保留现有 **`User` 包裹工具结果** 的路径，通过 **特性开关或 provider 类型** 在序列化时选择：

- `openai`：**必须** 发出 `tool` 消息（或拒绝发送不合法历史）。
- `bigmodel`：可继续发 `User`（若线上仍依赖），或统一为新格式（若网关确认兼容）。

**优点**：状态与 wire 格式一致，Think/Observe 与 LLM 客户端职责清晰。  
**缺点**：需改 serde、摘要、持久化、以及所有构造 `Message` 的测试。

### 5.2 ThinkNode：写入完整 assistant 轮

在 `apply_think_response`（及流式路径的等价逻辑）中：

- 追加 `Message::Assistant { content, tool_calls: Option<...> }`。
- 当 `tool_calls` 非空时，**即使** `content` 为空，也要把 `tool_calls` 写入状态（供下次 `invoke` 序列化）。

### 5.3 ObserveNode：写入 `Tool` 消息

对每条 `ToolResult`：

- 追加 `Message::Tool { tool_call_id, content }`，其中 `tool_call_id` 来自 `ToolResult.call_id`（与 Act 阶段一致）。
- 若缺少 `call_id`，OpenAI 路径无法合规：应在 Act/Think 全链路保证 **每个 `ToolCall` 带稳定 `id`**（空则生成 UUID），并在结果中回传。

### 5.4 `ChatOpenAI::messages_to_request`

使用 `async_openai` 中对应类型，将：

- `Assistant`（含 `tool_calls`）→ `ChatCompletionRequestMessage::Assistant` 的完整结构（`content` + `tool_calls`）。
- `Tool` → `ChatCompletionRequestMessage::Tool`（`tool_call_id` + `content`）。

无 `tool_calls` 且 `content` 为空（或仅空白）的 assistant：在构建请求前 **拒绝** 或 **归一化**（例如仅用于内部展示的占位符），避免提交非法 payload。

### 5.5 `ChatBigModel`

- **优先**：与 OpenAI 相同结构发 JSON（多数兼容网关支持 `tool` 角色）。
- **若** 确认某环境不接受 `tool`：在 `ChatBigModel::messages_to_request` 中保留 **降级** 为当前「User 工具结果」编码（由配置或探测决定）。

### 5.6 HTTP 请求说明（落地后的目标形态）

本小节描述 **方案实施后** `ChatOpenAI` / `ChatBigModel`（默认路径）发往 `POST .../chat/completions` 的 JSON 应呈现的形状，与 §3.1「规范期望」对齐。

**（1）首轮带工具定义**：除 `messages` 外，请求体仍包含 `tools`（函数名、描述、`parameters` JSON Schema），与现状一致；可选 `tool_choice`（`auto` / `none` / `required` 等）。示例仅突出 `messages` 与 `tools` 的并列关系：

```http
POST /v1/chat/completions HTTP/1.1
Content-Type: application/json
```

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    { "role": "system", "content": "…" },
    { "role": "user", "content": "现在几点？" }
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "get_time",
        "description": "返回当前时间",
        "parameters": { "type": "object", "properties": {} }
      }
    }
  ]
}
```

**（2）第二轮及以后（已完成一轮工具执行）**：内部状态经 Think / Observe 写入 `Assistant(tool_calls)` 与 `Tool` 后，序列化结果应满足：

- 每条「模型曾发起工具调用」的 assistant 在 JSON 中带 **`tool_calls`**（`id`、`type: "function"`、`function.name`、`function.arguments` 字符串）；
- 每条工具输出为 **`role: "tool"`**，**`tool_call_id`** 与上一条 assistant 中对应项一致，**`content`** 为工具返回字符串（可与当前 `ToolResult` 观测文本一致）。

同一轮若并行多个工具，则 **一条** assistant 可含 **多条** `tool_calls`，其后紧跟 **多条** `tool` 消息（`tool_call_id` 各对应其一），顺序与 Act 执行顺序或 OpenAI 建议顺序一致即可。示例（单工具）：

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    { "role": "system", "content": "…" },
    { "role": "user", "content": "现在几点？" },
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [
        {
          "id": "call_abc123",
          "type": "function",
          "function": { "name": "get_time", "arguments": "{}" }
        }
      ]
    },
    {
      "role": "tool",
      "tool_call_id": "call_abc123",
      "content": "{\"iso\":\"2025-03-25T12:00:00Z\"}"
    }
  ],
  "tools": [ "… 与首轮相同 …" ]
}
```

**（3）与内部 `Message` 的对应关系（实现 checklist）**

| 内部消息（概念） | HTTP `messages[]` 中的形态 |
|------------------|----------------------------|
| `Assistant`，无工具仅文本 | `role: "assistant"`, `content: "…"`，无 `tool_calls` 或空数组（以 API 要求为准） |
| `Assistant`，当轮调用了工具 | `role: "assistant"`, `content` 可为 `null` 或与文本并存，**必须**含 `tool_calls` |
| `Tool`（Observe 写入） | `role: "tool"`, `tool_call_id`, `content` |
| `User` / `System` | 与现状相同 |

**（4）BigModel 降级路径（可选）**：当配置为「兼容旧网关」时，同一 `Vec<Message>` 可序列化为 §3.1 **（2）** 那种 user 包裹工具结果的片段；**OpenAI 路径不得**在未迁移历史的情况下静默使用该降级，否则仍会 400。

### 5.7 其它受影响模块（实施时逐项核对）

- **摘要 / 压缩 / completion_check**：若只扫描 `User`/`Assistant` 文本，需识别 `Tool` 或跳过工具轮。
- **context_persistence / 日志**：新变体需可序列化或可 redact。
- **ToolCallContext.recent_messages**：类型若仍为 `Vec<Message>`，需包含新变体。
- **MockLlm / 集成测试**：构造多轮工具对话的用例应使用新消息形状。

## 6. 测试策略

1. **单元测试**：`messages_to_request`（OpenAI）对「assistant+tool_calls → tool → user」序列的 JSON 快照或与结构体等价断言。
2. **Mock HTTP**：本地 TCP/mock server 接收 POST body，断言含 `tool_calls` 与 `role":"tool"`。
3. **集成**（可选 `ignored`）：真实 `OPENAI_API_KEY` 两轮工具调用（例如先调只读工具再基于结果回答）。

## 7. 风险与开放问题

- **破坏性变更**：`Message` 的 serde 形状变化会影响已存 session/checkpoint；需版本字段或迁移脚本。
- **多工具并行**：同一 assistant 多条 `tool_calls` 时，`Tool` 消息顺序通常需与规范一致；需与当前 Act 顺序对齐。
- **流式 Think**：确保流式结束态写入状态的 `tool_calls` 与 non-stream 一致。
- **第三方网关**：部分代理对 `tool` 支持不完整，可能需要白名单或回退策略。

## 8. 文档与后续

- 实施后更新 [LLM Integration](../guides/llm-integration.md) 中 ChatOpenAI 小节，说明 **工具多轮必须使用 `Tool` 消息**。
- 可更新 [Tool System Architecture](../architecture/tool-system.md) 中 ReAct 数据流描述，与 Observe 写入格式一致。

## 9. 小结

| 项目 | 说明 |
|------|------|
| 问题性质 | OpenAI 严格校验 Chat 消息序列；当前用 `User` 承载工具结果且 assistant 未带 `tool_calls`。 |
| 方向 | 扩展 `Message`，Think/Observe 写入规范语义；OpenAI 客户端按 API 序列化。 |
| BigModel | 现状宽松；建议统一新格式并在必要时保留降级。 |

本方案为实施蓝图；具体类型命名、特性开关与迁移步骤可在开发 PR 中细化并与此文档交叉引用。
