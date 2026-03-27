# 方案：LlmUsage 对齐 OpenAI CompletionUsage（含 usage 明细）

> 状态：**已实施**（类型、OpenAI / compat 映射、ThinkNode 累计语义、测试）  
> 范围：`loom::llm::LlmUsage`、OpenAI / OpenAI-compat 映射、ThinkNode 用量合并、协议与持久化 JSON、测试  
> 相关指南（实施后需同步一句）：[../guides/llm-integration.md](../guides/llm-integration.md)

## 1. 背景

当前 `LlmUsage` 仅包含 OpenAI Chat Completions 响应里 `usage` 对象的三个顶层计数：

- `prompt_tokens`
- `completion_tokens`
- `total_tokens`

官方同一字段在文档中类型为 **`CompletionUsage`**，另有两块**可选**嵌套对象（随模型与场景出现）：

- `prompt_tokens_details`
- `completion_tokens_details`

Loom 在 `ChatOpenAI`、`openai_compat` 与流式 `StreamAccumulator` 中只映射上述三数，**明细被丢弃**。若要在产品内做缓存命中分析、推理 token 统计或与账单字段对齐，需要在类型与映射层补全。

**权威参考**（以官方为准，字段名以后台文档为准）：

- [Chat completion object](https://platform.openai.com/docs/api-reference/chat/object) → `usage`
- [CompletionUsage / Completions object](https://platform.openai.com/docs/api-reference/completions/object)（`CompletionUsage`  schema）

## 2. 目标与非目标

### 2.1 目标

1. 扩展 `LlmUsage`，使 **JSON 形状与 OpenAI `CompletionUsage` 兼容**（在 Loom 侧可序列化/反序列化同等信息）。
2. 在 **非流式**、**流式**、**openai_compat** 三条路径上，只要底层响应/SDK 提供明细，则 **原样填入** `LlmUsage`。
3. 明确 **ThinkNode 对 `usage` / `total_usage` 的合并语义**（尤其明细是否跨轮累计），避免错误相加。
4. 保持 **向后兼容**：旧客户端/旧日志仅三字段时仍能 `Deserialize`；新字段一律可选。

### 2.2 非目标

- 不在本方案中统一 **Responses API、Embeddings、Images** 等其它端点的 usage 形状（结构不同，另立任务）。
- 不强制实现 **流式 `raw_response` 全文拼接**（与 usage 扩展正交）。
- 不因本方案升级或替换 `async-openai`（若 SDK 缺字段，见 §5 兜底策略）。

## 3. OpenAI 侧 `usage` 结构（实施时的字段清单）

以下与官方 `CompletionUsage` 描述一致，用于核对 Loom 类型与映射是否漏项。

**顶层**

| 字段 | 说明 |
|------|------|
| `prompt_tokens` | 提示 token 数 |
| `completion_tokens` | 生成 token 数 |
| `total_tokens` | 合计 |

**`prompt_tokens_details`（可选 object）**

| 字段 | 说明 |
|------|------|
| `cached_tokens` | 提示中缓存命中 token（可选） |
| `audio_tokens` | 提示侧音频相关 token（可选） |

**`completion_tokens_details`（可选 object）**

| 字段 | 说明 |
|------|------|
| `reasoning_tokens` | 推理用 token（可选） |
| `audio_tokens` | 生成侧音频相关 token（可选） |
| `accepted_prediction_tokens` | Predicted Outputs 场景（可选） |
| `rejected_prediction_tokens` | Predicted Outputs 场景（可选） |

**流式**：需在请求中设置 `stream_options.include_usage: true`，通常在**最后一个 chunk** 上出现完整 `usage`；中断流可能拿不到。Loom 已有对 stream usage 的消费路径，扩展后应在同一位置写入完整 `LlmUsage`。

## 4. Loom 类型设计

### 4.1 放置位置

- 在 `loom/src/llm/mod.rs` 中定义：
  - `LlmUsage`（扩展）
  - `PromptTokensDetails` / `CompletionTokensDetails`（或与 OpenAI 命名对齐的私有/公开子结构）

### 4.2 约定

- 所有明细字段使用 **`Option<u32>`**（或与现有三字段一致的整数类型，全仓统一即可）。
- `serde`：`Serialize` / `Deserialize` 与现有 `LlmUsage` 一致；新增字段建议 `skip_serializing_if = "Option::is_none"`（与子结构一致），减少日志与协议噪音。
- **`Default`**：`LlmUsage` 仍为「三数为 0、明细为 `None`」，便于测试与占位。

### 4.3 命名

- 优先与 **OpenAI JSON 键名**一致（`prompt_tokens_details`、`completion_tokens_details`），便于对照文档与抓包。

## 5. 映射层改动范围

| 模块 | 内容 |
|------|------|
| `loom/src/llm/openai/mod.rs` | 非流式：`response.usage` → 完整 `LlmUsage` |
| `loom/src/llm/openai/stream.rs` | 流式：chunk 上 `usage` 更新时拷贝明细（通常最后一包覆盖即可） |
| `loom/src/llm/openai_compat.rs` | 非流式与流式两处 `LlmUsage { ... }` 构造补全 |

### 5.1 SDK 能力不足时的兜底

1. 查阅 **`async-openai` 当前版本**中 `CompletionUsage`（及流式 chunk 的 usage）是否已包含 `*_details`。
2. **若已包含**：直接字段映射。
3. **若未包含**：可选路径（按成本递增）：
   - 升级 `async-openai` 到已支持版本；或
   - 非流式下在已有 `serde_json::Value` 或 `raw_response` 上对 `usage` 子树做**轻量解析**，仅填充 `LlmUsage`（避免引入第二套全量响应类型）；或
   - 流式仅在 SDK 能给出的范围内填充，文档标明「明细依赖 SDK/网关」。

## 6. ThinkNode 合并语义（必须写清）

现状：`usage` / `total_usage` 对三计数有合并逻辑。扩展后**明细不能随意按轮相加**（语义未必与 OpenAI「整次请求一份 usage」一致）。

**推荐策略（默认采纳）**：

- **`total_usage`（累计）**：仅对 **`prompt_tokens` / `completion_tokens` / `total_tokens`** 做现有意义上的累计；**`prompt_tokens_details` / `completion_tokens_details` 不参与累计**，置为 `None`，或**仅保留最后一轮**响应中的明细（二选一，建议 **`None` + 单轮 `usage` 保留最后一轮明细** 更易解释）。
- **`usage`（当轮/最近）**：与单次 `LlmResponse.usage` 一致，**可含完整明细**。

在 `think_node.rs` 用**简短注释**固定上述语义，避免后续把 `cached_tokens` 等错误地跨轮相加。

若产品明确要求「跨 run 汇总缓存 token」，再单开需求定义可加字段的代数规则。

## 7. 协议、CLI 与持久化

- **`loom/src/protocol/mod.rs`**：若直接使用 `LlmUsage` 序列化，扩展后 JSON 自动带新键；检查是否有**手写** JSON 或仅解构三字段的代码，改为「三字段必选逻辑不变，明细可选」。
- **`loom/src/llm/context_persistence.rs`**：`llm_response` 中的 `usage` 体积略增；**隐私策略不变**（勿对外泄露含密钥的 `raw_*`）。
- **`cli/src/run/agent.rs`** 等：若仅使用三计数打印统计，可保持不变；若 `--verbose` 需展示明细，再增加输出（可选，不阻塞本方案核心）。

## 8. 测试计划

1. **单元 / 集成**：构造带 `prompt_tokens_details` 与 `completion_tokens_details` 的 **SSE 最后一帧** JSON（可沿用 `loom/tests/openai_sse.rs` 风格），断言 `LlmUsage` 解析完整。
2. **openai_compat**：fixture 或 mock 响应含明细时映射正确；省略明细时为 `None`。
3. **`react_nodes` / `MockLlm`**：`with_usage` 传入带明细的 `LlmUsage`，覆盖合并与序列化（与 §6 语义一致）。
4. **serde 兼容**：仅含三字段的旧 JSON 反序列化仍为成功，`details` 为 `None`。

## 9. 实施顺序建议

1. 类型与 serde（`mod.rs`）。
2. OpenAI 非流式 + 流式映射。
3. `openai_compat` 映射。
4. ThinkNode 合并逻辑与注释（§6）。
5. 全仓 `grep LlmUsage` 扫调用点；修协议/CLI 如有硬编码。
6. 测试与 `docs/guides/llm-integration.md` 一句说明。

## 10. 验收标准

- 对返回明细的 OpenAI（或兼容网关），**非流式与流式**（`include_usage`）下 `LlmResponse.usage` 中 **details 与 wire JSON 一致**（在 SDK 支持或兜底解析范围内）。
- 无明细时行为与今一致，**不破坏**现有测试与 JSON 兼容。
- 文档 §6 合并语义与代码一致，避免错误累计明细。

---

*文档版本：草案 v1*
