# 文档编写优秀实践

> 来源：OpenAI Agents SDK 文档 (https://developers.openai.com/api/docs/guides/agents)

## 1. 入口页即路线图

入口页不做教程，做决策导航。用一张表格回答"我想做 X，从哪开始"：

```
| 如果你想                     | 读这里              | 为什么                           |
|------------------------------|---------------------|----------------------------------|
| 快速跑通第一个 demo          | Quickstart          | 最短路径到可运行的集成           |
| 定义一个专家 Agent           | Agent definitions   | 塑造单个 Agent 的契约            |
| 理解运行时循环和状态         | Running agents      | Agent 循环、流式、续传策略都在这  |
| 在容器环境中运行             | Sandbox agents      | 需要文件、命令、快照、挂载时使用  |
| 设计多 Agent 协作            | Orchestration       | 需要多个 Agent 且要决定谁掌控回复 |
| 添加校验或人工审批           | Guardrails          | 阻止或暂停高风险操作             |
```

关键做法：
- 表格三列：意图 → 入口 → 理由
- 每行是一个用户角色 + 场景，不是功能列表
- 覆盖从入门到进阶的完整路径

## 2. 明确推荐阅读顺序

不要让读者自己猜该先读什么。直接给出线性的阅读路径：

```
推荐阅读顺序：
1. Quickstart — 先跑通一个可工作的 run
2. Agent definitions + Models — 塑造一个干净的专家 Agent
3. Running agents + Orchestration + Guardrails — 工作流变复杂时继续
4. Results and state + Integrations — 依赖运行结果或需要深度可观测时使用
```

原则：
- 按复杂度递增排列
- 每一步说明"什么时候该读"
- 不强求全部读完，按需取用

## 3. 每页自包含，目标驱动

每个页面遵循统一结构：

```
标题：一句话说清这页是什么
─────────────────────────
用途表格：什么场景用这页
核心概念：2-3 段说清机制
代码示例：最小可运行示例
关键决策点：不同策略的对比
下一步链接：指向后续页面
```

关键做法：
- 标题 + 副标题直接回答"这页是什么、什么时候用"
- 场景优先于概念："Use this when..." 先给使用场景，再讲机制
- 不假设读者读过前面的页面

## 4. 表格驱动决策，替代冗长段落

文档中大量使用对比表格，而不是长段文字：

```
| 策略                | 状态存储位置          | 适用场景                   |
|---------------------|-----------------------|----------------------------|
| 手动管理            | 你的应用              | 小型对话、最大控制          |
| session             | 你的存储 + SDK        | 持久聊天、可续传审批流      |
| conversationId      | OpenAI Conversations | 跨服务的共享服务器管理状态  |
| previous_response_id| OpenAI Responses API  | 最轻量的服务端续传          |
```

原则：
- 并列选项用表格，不用无序列表
- 每列是一个决策维度
- 表格后紧跟一句话推荐："Sessions are the best default when..."

## 5. 代码示例：完整可运行，紧跟概念

每个概念讲完立刻给代码，不是片段，是可以直接运行的完整示例：

```python
# 讲完概念后立刻给代码
result = await Runner.run(agent, "hello")
print(result.final_output)

# 不同策略给不同代码
result = await Runner.run(agent, state=state)  # 续传
result = await Runner.run(agent, session=session)  # session
```

原则：
- 包含 import、定义、调用，不留"// 其余代码省略"
- 一个示例只演示一个概念
- 示例后立刻给"下一步"

## 6. 渐进披露，高级内容不挡路

文档按"大多数人需要的"和"少数人需要的"分层：

- 第一层：Quickstart — 5 分钟跑通
- 第二层：核心概念页 — 定义、运行、编排
- 第三层：进阶页 — Results、Integrations、Evals

明确标注哪些是高级内容：
> "Richer run items, raw model responses, and detailed diagnostics are useful for audits and deep debugging, but they don't need to be the first thing most developers learn."

做法：
- 高级 API 和诊断信息放在页面底部或独立页面
- 用文字明确说"most developers don't need this first"

## 7. 交叉引用明确边界

每个页面明确说明什么在本页、什么不在：

```
Tool capability semantics live in Using tools.
This page focuses on SDK-specific MCP wiring and observability.
```

原则：
- 不重复内容，指向源页面
- 每页末尾给"Continue to..."链接
- 用简短句子划清边界

## 8. 嵌入最佳实践和反模式

不把最佳实践单独放一个页面，而是嵌入对应概念的文档中：

```
推荐写法：
✅ "Pick one strategy per conversation."
✅ "Sessions are the best default when you want durable memory."
✅ "In most applications, pick one strategy per conversation.
    Mixing local replay with server-managed state can duplicate context."

反模式警告：
⚠️ "Mixing strategies can duplicate context unless you are deliberately
    reconciling both layers."
```

原则：
- 金句直接写在正文中，不藏在水 Blog 里
- 标注推荐默认值
- 反模式用警告框突出

## 9. 默认开启可观测性

文档鼓励"先 inspect 再 tune"的工作流：

```
As soon as the first run works, open the Traces dashboard to inspect
model calls, tool calls, handoffs, and guardrails before you start
tuning prompts.
```

做法：
- Tracing 默认开启
- Quickstart 完成后立刻引导查看 Traces
- 把调试流程嵌入文档路径，不是事后补充

## 10. 文档即产品

- 元描述精炼：每页 meta description 就是文档的一句话定位
- AGENTS.md 作为活文档：用 `.md` 文件在仓库中持久化团队约定，随代码演进
- 搜索友好：关键词、标题、描述一致

---

## 总结：文档编写检查清单

- [ ] 入口页是否有一张"意图→入口→理由"的导航表格？
- [ ] 是否有明确的推荐阅读顺序？
- [ ] 每页是否能独立阅读，不依赖前置页面？
- [ ] 并列选项是否用表格对比，而非长段落？
- [ ] 代码示例是否完整可运行？
- [ ] 高级内容是否标记为可选/进阶？
- [ ] 是否明确标注了"什么在本页，什么不在"？
- [ ] 最佳实践是否嵌入正文，而非单独成页？
- [ ] 是否标注了推荐默认和反模式？
- [ ] 读完第一个文档后，用户能否在 5 分钟内跑通？
