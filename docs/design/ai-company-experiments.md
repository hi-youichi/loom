# AI Company 实验方案

## 目标
通过渐进式实验，验证并修复 `loom task` 命令和 AI 公司系统，直到公司能自动运行。

## 实验环境
- 工作目录：当前项目
- Agent profiles：.loom/agents/ 下已创建的 8 个角色
- Task DB：~/.loom/tasks/tasks.db

---

## 实验1: 基础功能验证
验证 `loom task list/show` 命令能正常工作。

```bash
# 预期：显示 "No tasks found."
loom task list

# 预期：报错 task not found
loom task show abc
```

## 实验2: CEO 单轮响应
验证 CEO agent 能被正确加载，理解需求并给出回应。

```bash
# 简单任务，观察 CEO 是否理解并直接派给 engineer
loom task new "在 README.md 末尾加一行当前日期"
```

验证点：
- [ ] Task 创建成功
- [ ] CEO agent 被加载
- [ ] CEO 理解需求
- [ ] CEO 决定分配给谁
- [ ] 子 Task 被创建
- [ ] engineer agent 被调用
- [ ] 文件被修改
- [ ] CEO 汇报结果

## 实验3: CEO 拆子任务
中等复杂度任务，观察 CEO 是否能正确拆解。

```bash
# 中等任务，需要先分析再执行
loom task new "给项目添加一个 .gitignore 文件，忽略常见的 Rust 和 Node.js 构建产物"
```

验证点：
- [ ] CEO 评估复杂度
- [ ] 是否拆成子任务
- [ ] 子任务是否有明确验收标准
- [ ] 分配是否合理

## 实验4: 完整项目流程
复杂任务，触发完整 PM → Architect → Engineer → QA 流程。

```bash
# 复杂任务
loom task new "在项目中添加一个简单的 CLI 命令 hello，打印 hello world，包含单元测试"
```

验证点：
- [ ] PM 拆解需求
- [ ] Architect 给出技术方案
- [ ] Engineer 实现
- [ ] QA 测试
- [ ] CEO 汇总交付

## 实验5: 任务恢复
验证 `loom task continue` 能恢复上下文。

```bash
# 先创建任务
loom task new "添加一个计算斐波那契数列的函数"
# 记下 task id
# 然后恢复
loom task continue <task_id>
```

---

## 每轮实验流程
1. 运行实验命令
2. 观察输出，记录问题
3. 修改 agent profile / system prompt / 代码
4. 重新实验
5. 直到通过

## 常见问题预判
- CEO 可能不知道怎么调用 invoke_agent（需要在 prompt 中更明确）
- CEO 可能直接自己回答而不派任务
- 子 agent 可能没有正确上下文
- Task 工具可能不在 agent 可用工具列表中
- 多轮交互可能丢失上下文
