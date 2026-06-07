# 方案 B：Agent 层 Tool Session Lost 自动恢复

## 1. 问题定义

MiniMax API（code 2013）在收到 tool result 时校验 `tool_call_id` 是否存在于**服务端会话**中。
当工具执行耗时较长、或存在并发请求时，服务端会话状态丢失导致：

```
OpenAI-compat stream error 400 Bad Request:
  invalid params, tool result's tool id(call_xxx) not found (code: 2013)
```

此错误发生在 **ReAct 循环的第 N+1 次 Think 节点**——当 ThinkNode 将上一轮 ObserveNode 组装的 tool result 消息回传给 LLM 时。

## 2. 错误发生位置

```
think_node.rs:262-291  invoke_think_llm() 或 llm.invoke()
                      → AgentError::ExecutionFailed("...code: 2013...")
                      → 直接 return Err(e) 传播到 graph runner
                      → runner 终止整个 run
```

### 关键数据流

```
Turn N:
  ThinkNode.run_with_context()
    └─ state.apply_think()                          ← 推送 Message::Assistant { tool_calls: [call_A, call_B] }
    └─ 设置 state.message_count_after_last_think     ← 记录 assistant 消息位置
  ActNode.run()
    └─ 执行工具 → state.tool_results                ← call_id 从 tool_calls 回填
  ObserveNode.run()
    └─ 推送 Message::Tool { tool_call_id: call_A }  ← 多个 tool 消息
    └─ 推送 Message::Tool { tool_call_id: call_B }
    └─ 清空 state.tool_calls / state.tool_results
  CompressNode (可选)
    └─ 可能修剪旧消息 (prune)

Turn N+1:
  ThinkNode.run_with_context()
    └─ llm.invoke(&state.messages)                  ← 发送全部消息给 MiniMax
    └─ MiniMax: "tool_call_id call_A not found"     ← 2013 错误！
    └─ return Err(e) → 终止
```

## 3. 恢复策略

### 3.1 核心思路

在 ThinkNode 内部捕获 2013 错误，回滚消息历史到上一轮开始之前，然后重试 LLM 调用。
LLM 看到"干净"的消息历史后会重新生成 tool_calls（带新的 call_id），ActNode 重新执行工具，
新生成的 tool result 与新的 tool_call_id 匹配，MiniMax 校验通过。

### 3.2 回滚方法

不依赖 `message_count_after_last_think`（CompressNode 的 prune 可能使其失效），
而是**搜索最后一个包含 tool_calls 的 assistant 消息**：

```rust
/// 找到最后一条带 tool_calls 的 assistant 消息的索引
fn find_last_assistant_with_tool_calls(messages: &[Message]) -> Option<usize> {
    messages.iter().rposition(|m| {
        matches!(m, Message::Assistant(p) if !p.tool_calls.is_empty())
    })
}

/// 回滚到该消息之前（删除 assistant + 后续所有 tool 消息）
fn rollback_last_tool_round(state: &mut ReActState) -> bool {
    if let Some(pos) = find_last_assistant_with_tool_calls(&state.messages) {
        state.messages.truncate(pos);
        state.tool_calls.clear();
        state.tool_results.clear();
        state.message_count_after_last_think = Some(state.messages.len());
        true
    } else {
        // 没有 tool round 可回滚，说明是首轮 Think 就报错了（不应发生）
        false
    }
}
```

### 3.3 错误识别

仅针对 MiniMax 2013：

```rust
fn is_minimax_session_lost(err: &AgentError) -> bool {
    match err {
        AgentError::ExecutionFailed(msg) => {
            msg.to_lowercase().contains("(code: 2013)")
        }
        _ => false,
    }
}
```

### 3.4 重试循环（ThinkNode.run_with_context）

```
pseudocode:

fn run_with_context(state, ctx):
    let llm = resolve_client()
    let mut session_retries = 0
    const MAX = 3

    loop:
        // 构建 LLM 调用 future
        let llm_call = if streaming { invoke_think_llm(...) } else { llm.invoke(...) }

        match run_cancellable(llm_call, cancel_token).await:
            Ok(Ok(triple)) → break (response, chunks, first_token)
            Ok(Err(e)) if is_session_lost(e) AND retries < MAX:
                retries += 1
                if rollback(state):
                    warn!("session lost, rolled back, retry {}/{}", retries, MAX)
                else:
                    warn!("session lost but no tool round to rollback, failing")
                    return Err(e)
            Ok(Err(e)) → return Err(e)    // 其他错误直接传播
            Err(e)    → return Err(e)

    // 正常流程继续
    emit_post_response_events(...)
    new_state = state.apply_think(...)
    emit_usage_event(...)
    Ok((new_state, Next::Continue))
```

### 3.5 ThinkNode.run 同步处理

非 streaming 路径（`run` 方法）同样处理：

```rust
async fn run(&self, mut state: ReActState) -> Result<(ReActState, Next), AgentError> {
    let llm = self.resolve_client(&state.model_config).await?;
    let mut session_retries = 0;
    let response = loop {
        match llm.invoke(&state.messages).await {
            Ok(resp) => break resp,
            Err(ref e) if is_provider_session_lost(e) && session_retries < 3 => {
                session_retries += 1;
                if !rollback_last_tool_round(&mut state) {
                    return Err(e);
                }
                tracing::warn!(attempt = session_retries, "Session lost, retrying Think");
            }
            Err(e) => return Err(e),
        }
    };
    let new_state = state.apply_think(response.content, ...);
    Ok((new_state, Next::Continue))
}
```

## 4. 涉及文件

| 文件 | 改动 |
|------|------|
| `loom-agent/src/agent/react/think_node.rs` | **主要改动**：添加 3 个辅助函数 + 修改 run/run_with_context 的 LLM 调用为带重试的循环 |
| 无需改动其他文件 | — |

## 5. 边界情况分析

### 5.1 首轮 Think 报 2013
- `rollback_last_tool_round` 找不到 assistant with tool_calls → 返回 false
- 不执行回滚，直接传播错误（不应该发生，因为没有 tool round 可回滚）
- 说明 MiniMax 会话在首轮就丢失（可能是账户/API Key 问题）

### 5.2 CompressNode 已 prune 消息
- `rposition` 查找不依赖 `message_count_after_last_think`
- 只要 assistant(tool_calls) 消息没被 prune（它是最新的，prune 通常删旧消息），就能正确找到
- truncate 后 `message_count_after_last_think` 被重置为当前 messages.len()

### 5.3 多次连续 2013（达到重试上限）
- 3 次重试后仍然失败 → 传播原始错误
- 错误消息包含原始 2013 信息，用户可据此排查

### 5.4 工具幂等性问题
- 回滚 + 重试导致 LLM 重新生成 tool_calls，ActNode 重新执行工具
- **读工具**（read_file, grep, glob, lsp）：完全幂等，无副作用
- **写工具**（write_file, edit, delete_file, bash）：可能重复执行
  - 重试上限 3 次，风险可控
  - 更彻底的方案：记录已执行工具的结果，重试时复用（P2，后续优化）

### 5.5 streaming 路径
- `invoke_think_llm` 内部创建 chunk_tx/tool_delta_tx 并 join
- 错误发生在 `llm.invoke_stream_with_tool_delta` 返回时
- chunk_tx 已在闭包中被 drop，重试时重建新的 channel
- 无资源泄漏

### 5.6 CancellationToken
- 每轮重试都经过 `run_cancellable` 检查
- 用户 Ctrl+C 可随时中断重试循环

## 6. 常量定义

```rust
/// 服务端会话丢失时最大重试次数
const MAX_SESSION_LOST_RETRIES: u32 = 3;
```

放在 `think_node.rs` 顶部，与其他常量一起。

## 7. 日志与可观测性

每次回滚 + 重试时打 `tracing::warn`：

```
warn!(
    attempt = session_retries,
    max_retries = MAX_SESSION_LOST_RETRIES,
    messages_before = old_msg_count,
    messages_after = state.messages.len(),
    "Provider session lost (2013), rolled back tool round and retrying Think"
);
```

## 8. 测试策略

### 单元测试
1. `is_provider_session_lost` 匹配 "code: 2013"
2. `is_provider_session_lost` 匹配 "tool result's tool id ... not found"
3. `is_provider_session_lost` 不匹配其他 ExecutionFailed
4. `find_last_assistant_with_tool_calls` 正确找到位置
5. `find_last_assistant_with_tool_calls` 无 tool_calls 时返回 None
6. `rollback_last_tool_round` 正确截断消息

### 集成测试
- 使用 MockLlm 模拟：第 1 次返回 2013 错误，第 2 次返回成功
- 验证消息历史正确回滚
- 验证 tool_calls 使用新 ID 重新生成

## 9. 实施步骤

1. 在 `think_node.rs` 添加 3 个辅助函数（`is_provider_session_lost`、`find_last_assistant_with_tool_calls`、`rollback_last_tool_round`）
2. 添加单元测试
3. 修改 `run` 方法的 LLM 调用为 `loop { match ... }`
4. 修改 `run_with_context` 方法的 LLM 调用为 `loop { match ... }`
5. `cargo test -p loom-agent` 验证
6. `cargo clippy -- -D warnings` 检查
