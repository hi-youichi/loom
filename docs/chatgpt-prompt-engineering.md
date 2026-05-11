# ChatGPT 提示词工程（Prompt Engineering）指南

> 来源：OpenAI 官方文档，整理于 2025-08

## 一、官方文档索引

| 文档 | 链接 | 适用对象 |
|---|---|---|
| ChatGPT Prompt Engineering Best Practices | https://help.openai.com/en/articles/10032626-prompt-engineering-best-practices-for-chatgpt | 所有 ChatGPT 用户 |
| ChatGPT Enterprise Prompting Guide | https://developers.openai.com/cookbook/examples/chatgpt/chatgpt_prompt_guide/chatgpt_prompt_guide | 企业用户 / 日常办公 |
| Prompt Engineering (API) | https://platform.openai.com/docs/guides/prompt-engineering | API 开发者 |
| GPT-5 Prompting Guide | https://developers.openai.com/cookbook/examples/gpt-5/gpt-5_prompting_guide | GPT-5 用户 |
| GPT-5.5 Prompt Guidance | https://developers.openai.com/api/docs/guides/prompt-guidance | GPT-5.5 用户 |

---

## 二、核心概念

### 什么是 Prompt？

Prompt（提示词）是发给大语言模型的文本输入，用于发起对话或触发模型响应。也可以是图像、音频等其他形式。

### 什么是 Prompt Engineering？

Prompt Engineering 是设计和优化输入提示词的过程，目的是有效地引导语言模型生成符合预期的响应。

---

## 三、通用最佳实践

### 1. 清晰且具体（Be clear and specific）

- 确保提示词清晰、具体，提供足够的上下文让模型理解你的需求
- 避免歧义，尽可能精确以获得准确、相关的响应
- **反面**：「帮我写点东西」
- **正面**：「为一家 B2B SaaS 公司撰写一封 200 字以内的客户跟进邮件，语气专业但友好，包含产品演示邀请」

### 2. 迭代优化（Iterative refinement）

- 提示词工程是一个迭代过程
- 从初始提示词开始 → 审查响应 → 根据输出优化提示词
- 调整措辞、增加更多上下文、或简化请求
- **不要期望第一个提示词就完美**——迭代是正常的

### 3. 控制语气（Requesting a different tone）

- 使用描述性形容词来指示语气
- 例如：formal（正式）、informal（非正式）、friendly（友好）、professional（专业）、humorous（幽默）、serious（严肃）
- 示例：「用友好且引人入胜的语气解释这个概念」

---

## 四、ChatGPT Enterprise 提示词指南（实用框架）

### 心态（Mindset）

- 提示词不是在"欺骗"模型——而是明确任务、上下文和成功标准
- 提示词是迭代的（这很正常）——尝试、失败、学习、改进

### 四步法

#### Step 1：界定问题范围（Scope the problem）

- 在开始前定义成功标准和限制
- 好的范围界定防止冗长无效的提示词，减少浪费的迭代
- 明确"好的输出"长什么样

#### Step 2：清晰编写提示词（Write the prompt clearly）

- 写完整提示词前，用 30 秒列出大纲
- 使用标题/结构化格式组织你的需求

#### Step 3：用 ChatGPT 帮你写提示词（Meta-prompting）

- 让 ChatGPT 帮你优化提示词本身
- 例如：「这是一个我打算用来做 X 的提示词，请帮我改进它」

#### Step 4：提高准确性（Improve accuracy）

- 提供具体示例（few-shot）
- 设置输出格式要求
- 明确约束和边界条件

---

## 五、API 提示词工程技术

### 1. 消息角色（Message Roles）

- `system` / `instructions`：高级指令，定义模型行为、语气、目标
- `user`：具体任务和上下文
- `assistant`：模型响应，也可用于预设对话历史

`instructions` 参数优先级高于 `input` 中的提示词。

### 2. 格式化：Markdown 和 XML

- 使用 Markdown 格式化帮助模型理解逻辑边界
- 使用 XML 标签分隔不同部分的内容
- 示例：

```markdown
## 任务
分析以下文本的情感倾向。

## 输入文本
<text>
今天天气真好，心情愉快！
</text>

## 输出格式
返回 JSON：{"sentiment": "positive/negative/neutral", "confidence": 0.0-1.0}
```

### 3. Few-shot Learning（少样本学习）

- 在提示词中提供几个输入-输出示例
- 模型会从示例中学习模式并应用到新输入上
- 示例比纯文字描述更有效

### 4. 可复用提示词（Reusable Prompts）

- 在 OpenAI Dashboard 中创建可复用的提示词模板
- 支持版本管理，无需修改代码即可更新提示词
- 支持变量模板，通过 `variables` 参数传入动态内容
- 与 Eval 配合使用，监控提示词性能
- 目前仅在 Responses API 中支持，Chat Completions API 不可用

### 5. Prompt Caching（提示词缓存）

- 减少延迟最高 **80%**，降低成本最高 **75%**
- 自动缓存重复的提示词前缀
- 适合包含大量固定上下文（系统指令、知识库文档）的场景
- 无需额外配置，API 自动处理

### 6. 检索增强生成（RAG）— 添加相关上下文

- 通过向量数据库查询结果将额外上下文注入提示词
- 或使用 OpenAI 内置的 file search 工具，基于上传文档生成内容
- 核心思想：给模型提供与任务相关的背景信息，而不是仅依赖训练知识
- 适用于：问答系统、文档分析、知识密集型任务

### 7. 上下文窗口规划

- 模型每次请求能处理的数据量有上限，称为**上下文窗口（Context Window）**
- 以 token 为单位计算（文本、图像等都占用 token）
- 不同模型窗口大小不同：从 100k 到 100 万 token（如 GPT-4.1）
- 提示词 + 对话历史 + 输出总和不能超过窗口限制
- 建议：控制输入长度，截断不必要的历史，优先保留关键信息

### 8. 模型选择

- **推理模型（Reasoning Models）**：如 o3、GPT-5，适合复杂任务和多步规划，但更慢更贵
- **GPT 模型**：快速、经济、高度智能，但需要更明确的指令
- 建议将生产应用固定到特定模型快照（如 `gpt-4.1-2025-04-14`）以确保行为一致

**比喻（来自 OpenAI 官方）：**
- 推理模型像**资深同事** —— 给他目标，他自主搞定细节
- GPT 模型像**初级同事** —— 需要明确的步骤指令才能产出最佳结果

### 9. 构建评估体系（Evals）

- 构建自动化评估来衡量提示词的性能
- 监控迭代过程中和模型版本变更时的行为变化
- 建议为每个关键场景定义评估标准和测试用例
- 可结合 OpenAI Dashboard 的 Prompt 管理功能使用

### 10. 使用 Playground 迭代

- 在 OpenAI Playground 中快速开发和测试提示词
- 实时调整参数、查看输出、对比不同模型的表现
- 满意后再迁移到代码中

---

## 六、GPT-5 提示词技巧

- GPT-5 默认行为是**全面且深入**地收集上下文，确保正确答案
- **降低 Agent 行为范围**：设置 `reasoning_effort`、在 `instructions` 中定义明确的停止条件
- **鼓励自主性**：提高 `reasoning_effort`，使用鼓励持续完成任务的提示词
- 关键原则：**明确说明你想要什么，以及什么时候该停下来**

### GPT-5 编码提示技巧（Coding）

- GPT-5 在前端和软件工程任务上有专门优化
- Cursor 等代码编辑器已基于 GPT-5 做了提示词调优
- 建议：明确描述技术栈、框架版本、项目结构和约束条件
- 对于重构任务，提供修改前后的期望行为对比

### Prompt Optimizer 工具

- OpenAI 提供内置的 Prompt Optimizer，自动优化提示词
- 输入原始提示词和期望输出，工具自动生成改进版本
- 可作为迭代的起点，再根据实际需求手动调整

---

## 七、GPT-5.5 提示词技巧

- **Outcome-first（结果优先）**：描述成功的标准、约束条件、可用证据、最终答案应包含什么
- 短小、以结果为导向的提示词通常比流程繁重的提示词效果更好
- 明确设定人格（Personality）和行为规则（Behavior）：
  - **Personality** 控制"怎么说"：语气、温度、幽默感、共情程度、措辞精细度
  - **Behavior** 控制"怎么做"：何时提问 vs 自行推断、主动程度、上下文详略、何时自查、如何处理不确定性
  - 两者都应保持简短，避免冗长的行为描述堆叠
- 设定检索预算（retrieval budget）和验证规则
- 使用 **preamble（序言）** 提升感知响应速度——让模型先输出一个简短的状态更新
- 低/中等推理努力（reasoning effort）已足够应对大多数场景，先评估再决定是否升级

---

## 八、Enterprise 实用 Prompt 模板

以下是常见办公场景的提示词模板（来自 Enterprise Prompting Guide）：

### 文档摘要

```markdown
## 任务
将以下文档摘要为 3-5 个要点。

## 要求
- 每个要点不超过 2 句话
- 保留关键数据和结论
- 标注信息来源章节

## 输入文档
<doc>
[粘贴文档内容]
</doc>
```

### 邮件起草

```markdown
## 任务
起草一封 [目的] 的邮件，发给 [收件人角色]。

## 语气
[专业 / 友好 / 正式]

## 关键信息
- [要点 1]
- [要点 2]
- [要点 3]

## 约束
- 字数控制在 [N] 字以内
- 包含明确的行动号召（Call to Action）
```

### 内容翻译

```markdown
## 任务
将以下内容从 [源语言] 翻译为 [目标语言]。

## 要求
- 保持原文的语气和风格
- 专业术语保留原文并附翻译
- 如有文化差异，做适当本地化

## 输入
[粘贴内容]
```

### 场景分析 / 决策支持

```markdown
## 角色
你是一位 [领域] 专家。

## 任务
分析以下场景并给出建议。

## 场景
[描述场景]

## 输出格式
1. 现状分析（3-5 个要点）
2. 可选方案（至少 3 个，含优缺点）
3. 推荐方案及理由
4. 风险提示
5. 下一步行动建议
```

### 头脑风暴

```markdown
## 任务
为 [主题] 进行头脑风暴，生成 [数量] 个创意方案。

## 约束
- 每个方案包含：标题 + 一句话描述 + 可行性评估
- 覆盖不同方向（保守型 / 创新型 / 激进型）
- 标注潜在风险
```

---

## 九、总结：好提示词的公式

```
好提示词 = 明确的任务 + 充分的上下文 + 具体的约束 + 期望的输出格式 [+ 示例]
```

### 快速检查清单

- [ ] 我要模型做什么？（任务）
- [ ] 模型需要知道什么背景？（上下文）
- [ ] 有什么限制或要求？（约束）
- [ ] 我期望的输出格式是什么？（格式）
- [ ] 能否提供一个好输出的例子？（示例）
- [ ] 总长度是否在上下文窗口内？（token 预算）
- [ ] 是否需要检索外部知识？（RAG）
- [ ] 有没有对应的 Eval 来验证输出质量？（评估）
- [ ] 是否可以用 Playground 先快速验证？（迭代）
