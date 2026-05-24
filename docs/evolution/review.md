# 后台审查 (Review Agent)

> 基于 Hermes Agent 源码 `agent/background_review.py` 的实际实现。

## 流程

```
每轮对话结束
    │
    ▼
spawn_background_review()
    │
    ▼
创建 daemon 线程，fork 一个独立 AIAgent 实例
    │
    ▼
回放对话快照（messages）+ 构建 Review Prompt
    │
    ▼
Fork Agent 调用 LLM 分析（白名单工具：memory + skill_manage）
    │
    ├── memory 更新 → 写入 USER.md / PROJECT.md / FACTS.md
    └── skill 操作 → 按优先级链执行
```

## Hermes 实际实现细节

### 触发时机
- 每轮对话结束后，`AIAgent.run_conversation` 调用 `_spawn_background_review`
- fork 的 agent 继承父 agent 的 provider/model/credentials/base_url（命中同一 prefix cache）
- 运行在独立 daemon 线程，不阻塞主对话

### 工具白名单
- fork 的 agent 只能使用 `memory` 和 `skill_manage` 工具
- 其他工具在运行时被拒绝

### Review Prompt 类型
1. `_MEMORY_REVIEW_PROMPT`: 仅审查记忆——用户偏好、个人信息、行为期望
2. `_SKILL_REVIEW_PROMPT`: 仅审查技能——更新/创建/修补技能文档
3. `_COMBINED_REVIEW_PROMPT`: 同时处理记忆和技能（实际默认使用的版本）

### 技能 Review 优先级链

```
1. PATCH 当前会话已加载的技能（通过 /skill-name 或 skill_view 加载的）
   ↓ 无匹配
2. PATCH 已有的 class-level umbrella 技能（通过 skills_list + skill_view 查找）
   ↓ 无匹配
3. 在已有 umbrella 下添加 support file
   ├── references/<topic>.md   — 会话细节、API 文档摘录、领域笔记
   ├── templates/<name>.<ext>  — 可复用的模板文件
   └── scripts/<name>.<ext>    — 可直接执行的脚本
   ↓ 无匹配
4. CREATE 新的 class-level umbrella 技能（SKILL.md）
   命名要求：类级别，禁止 PR 号/错误串/一次性任务名
```

### 触发信号（Review Prompt 中定义）
- 用户纠正了 agent 的风格/语气/格式/详细程度
- 用户表达了挫败感："stop doing X"、"太冗长了"、"不要这样格式化"
- 非平凡技巧、修复、变通方案、调试路径出现
- 已加载/查阅的技能被发现过时/缺失/错误

### 反模式保护（不保存的内容）
- 环境依赖的临时故障（缺二进制、路径不匹配、未安装包）
- 工具负面断言（"browser 工具不工作"、"X 工具坏了"）——会硬化为长期拒绝
- 已解决的瞬时错误（retry 成功 → 教训是 retry 模式，不是原始故障）
- 一次性任务叙事（"总结今天市场"、"分析这个 PR"）

### agent-created 标记
- 后台 review fork 通过 `skill_manage(action=create)` 创建的技能，由 `mark_agent_created()` 标记
- 判断方式：`is_background_review()` 检查当前是否在 review fork 线程中（`skill_manager_tool.py:773-782`）
- 标记为 agent-created 的技能可被 Curator 操作（patch/archive/consolidate）

### 安全扫描
- `skills.guard_agent_created` 配置项（默认 `false`）
- 开启后，agent-created 技能创建时经过 `skills_guard.py` 安全扫描
- "ask" 判定对 agent-created 技能视为危险（默认拒绝）

## Loom 适配方案

用 Loom 的 `invoke_agent` 实现类似机制：

```yaml
# .loom/agents/reviewer/profile.yaml
name: background-reviewer
tools: [read, write_file, edit, glob, grep]
system_prompt: |
  Review the conversation and update memory + skills...
  Follow the priority chain: patch loaded → patch umbrella → add support file → create umbrella
```

触发方式：主对话结束后，异步 `invoke_agent(agent="reviewer", task="<对话快照>", async=true)`

## 错误处理

- LLM 调用失败：3 次重试，指数退避（2s / 4s / 8s）
- Review 结果通过 `summarize_background_review_actions()` 汇总
- 失败通过 `agent.background_review_callback` 回调通知
- Memory 写入失败：记录错误日志，不阻塞主对话

## 配置

```yaml
review:
  enabled: true
  max_session_chars: 12000
  guard_agent_created: false    # 是否对 agent-created 技能做安全扫描
```

## 相关文档

- [记忆系统](memory.md) — memory 文件格式
- [技能系统](skills.md) — 技能结构与 Curator
- [配置参考](config.md) — review 配置项
