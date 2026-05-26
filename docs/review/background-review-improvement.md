# Background Review 全面改进分析

## 一、架构级问题

### 1. 会话内容收集过于简陋

`cli/src/run/agent.rs:349`:
```rust
let session_content = format!("User: {}\n\nAssistant: {}", user_msg, reply);
```

*问题*：只收集了最后一轮 User+Assistant 文本，丢弃了：
- 多轮对话历史（前几轮的工具调用、中间推理）
- 工具调用结果（最关键的学习信号 — 哪些工具成功了/失败了）
- reasoning_content（思考链）
- 加载过的技能列表（prompt 说"check what skills were loaded"，但 review agent 看不到）

*影响*：review agent 只能从一问一答中学习，错过最有价值的调试路径和工具交互模式。

**改进**：传入完整 messages 数组序列化，或至少包含 tool_call/result 摘要。

### 2. Agent Loop 缺少终止质量评估

`agent_loop.rs:84-88`：
```rust
if iterations >= config.max_iterations {
    info!("Review agent reached max iterations ({})", config.max_iterations);
    break;
}
```

*问题*：达到 max_iterations 时直接 break，没有总结已完成的工作。review agent 可能做了很多有价值的 tool call，但因为被迫中断而没有生成 summary。

**改进**：达上限时追加一条 user message 要求 LLM 总结已有操作，确保 `summary` 不为空。

### 3. 工具执行结果不验证语义正确性

`tools.rs:68-97`（memory_set）：
```rust
let result = match action {
    "append" => self.memory.append(file, content),
    "replace" => self.memory.replace(file, content),
    _ => return json!({...}),
};
```

*问题*：
- append 不检查内容是否已存在（重复追加同一事实）
- replace 不检查新旧内容差异（可能把 1000 行替换为 10 行，丢失大量信息）
- 没有对 memory 文件总大小的限制（可以无限增长）

**改进**：
- append 前检查 substring 去重
- replace 前检查新旧长度比，差异过大时返回 warning
- 增加 memory 文件总大小限制（如 64KB）

### 4. Review 触发时机单一

`cli/src/run/agent.rs:336`：只在 `EndTurn` 且 reply 非空时触发。

*缺失的触发场景*：
- `MaxTokens`（回复被截断但对话有价值）
- 连续多轮对话后才触发一次（单轮 review 看不到对话模式）
- 用户主动 `/review` 命令

### 5. ReviewModel 硬编码为 gpt-4o-mini

`workflow.rs:51`：
```rust
model: "gpt-4o-mini".to_string(),
```

*问题*：不可配置。review 质量完全取决于此模型能力，且用户无法选择更便宜的模型或更强大的模型。

## 二、质量问题

### 6. Security 验证不覆盖 skill_edit/skill_patch

`tools.rs:184-209`（skill_edit）：
```rust
fn skill_edit(&mut self, args: &Value) -> Value {
    let name = args["name"].as_str().unwrap_or("");
    let content = args["content"].as_str().unwrap_or("");
    // 直接赋值 skill.body = content，没有走 validate_skill_create
```

`skill_patch` 同样没有安全验证。LLM 可以通过 patch 注入 DANGEROUS_PATTERNS 中的内容。

**改进**：`skill_edit` 和 `skill_patch` 后也应调用 `validate_skill_create` 做安全检查。

### 7. Summarize 逻辑脆弱

`agent_loop.rs:171-195`：
```rust
fn summarize_actions(actions: &[ReviewAction]) -> String {
    let filtered: Vec<_> = actions.iter()
        .filter(|a| {
            a.summary.contains("created")
                || a.summary.contains("updated")
                || a.summary.contains("appended")
                // ...
        })
        .collect();
```

*问题*：summary 过滤依赖英文字符串匹配。如果 LLM 返回中文 summary（如"已创建技能 xxx"），会被过滤为 "No updates."。

**改进**：让 ReviewAction 增加 `has_modification: bool` 字段，由工具执行器直接设置。

### 8. Curator 同步执行阻塞 tokio 任务

`workflow.rs:133`：
```rust
if let Err(e) = run_curator_if_needed(...) {
```

Curator::run 是同步文件 IO 操作（扫描所有技能、比较文本、写文件），在 tokio::spawn 的异步任务中直接调用会阻塞 worker thread。

**改进**：用 `tokio::task::spawn_blocking` 包裹。

### 9. Evolution 触发器是空壳

`evolution.rs` 只定义了 config 类型，没有实际实现：
```rust
//! The actual EvolutionTrigger implementation lives in `cli` because it depends on
//! `loom-evolution`, which in turn depends on `loom` (cyclic dependency).
```

但 `workflow.rs` 中 `evolution_enabled: bool` 被配置但从未使用。配置项误导用户。

## 三、可观测性缺失

### 10. 无 Review 效果反馈闭环

- review 产出的 memory/skill 是否在后续会话中被使用？无追踪
- review 的 action 质量如何？是否有 "review 又把记忆搞坏了" 的信号？无检测
- `ObservabilityStore` 只记录 `record_review(session_id, memory_count, skill_count, duration_ms)`，无 action 级别详情

### 11. History 只记录成功，不记录跳过

`workflow.rs:197-211`：history.append 只在成功路径调用。跳过的 session（too short / no creds）没有记录，无法回答 "review 上次运行了吗？" 的问题。

## 四、改进优先级

| 优先级 | 问题 | 改进 | 影响 |
|--------|------|------|------|
| **P0** | #1 会话内容只有1轮 | 传入完整 messages 或含工具调用的摘要 | review 质量飞跃 |
| **P0** | #6 skill_edit/patch 无安全验证 | 执行后 validate | 安全漏洞修复 |
| **P0** | #7 summarize 依赖英文字符串 | ReviewAction 加 has_modification 标记 | 中文 review 不再丢失 |
| **P1** | #3 append 不去重 | 检查 substring | 避免记忆膨胀 |
| **P1** | #8 curator 同步阻塞 | spawn_blocking | 避免影响其他异步任务 |
| **P1** | #2 max_iterations 无总结 | 追加总结请求 | 不丢失已执行的操作 |
| **P1** | #11 history 不记录跳过 | 所有路径都 append record | 完整审计 |
| **P2** | #4 触发时机单一 | 支持 MaxTokens、多轮合并、手动触发 | 覆盖更多场景 |
| **P2** | #5 model 硬编码 | 可配置化 | 灵活性 |
| **P2** | #9 evolution 空壳 | 清理或实现 | 减少误导 |
| **P2** | #10 无效果反馈 | 记录 skill/memory 被使用次数 | 质量评估 |
