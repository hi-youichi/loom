---
sidebar_position: 4
title: Meta-Agent 架构：AI 组织
description: 像公司 CEO 一样运作的 Meta-Agent，动态创建、调度和销毁子 Agent，形成自组织的 AI 团队
---

## 核心理念

传统 code agent 的子代理模式是**静态的**——开发者预先定义好一组 agent，每个 agent 有固定的角色和工具。但真实的团队不是这样运作的。

一个 CEO 不会预先招聘所有可能需要的人。他会：

1. **识别需求** — 公司面临什么问题？
2. **定义岗位** — 解决这个问题需要什么能力？
3. **招聘上岗** — 找到合适的人，给足上下文
4. **交付验收** — 检查结果，给出反馈
5. **项目收尾** — 团队解散，经验沉淀

**Meta-Agent（AI CEO）** 将这个模式引入 agent 架构：一个能**动态创建其他 agent** 的顶层 agent。

## 架构设计

### 角色定义

```
┌─────────────────────────────────────┐
│           Meta-Agent (CEO)          │
│  - 理解任务，分解目标                │
│  - 管理能力注册表                    │
│  - 调度、监控、验收                  │
│  - 沉淀经验为 Skill                 │
└──────────┬──────────┬───────────────┘
           │          │
     ┌─────▼──┐  ┌───▼──────┐
     │ Agent A │  │ Agent B  │  ...
     │ (前端)  │  │ (数据库) │
     └────────┘  └──────────┘
```

### 能力注册表（Capability Registry）

每个 agent 声明自己的能力画像：

```toml
[agent.capabilities]
role = "frontend-developer"
skills = ["react", "typescript", "css", "testing"]
tools = ["file-system", "browser", "npm"]
context_limit = "128k"
priority = 80  # 信任权重，基于历史表现动态调整
```

Meta-Agent 维护这个注册表，根据任务需求做最优匹配。不是"谁空闲就给谁"，而是"谁最适合就给谁"。

### 生命周期：Spawn → Execute → Retire

子 agent 的生命周期是**按需的**，不是永驻的：

```
任务到达
  │
  ▼
Meta-Agent 分析任务
  │
  ├── 需要新能力？ ──→ 创建 agent（spawn）
  │                    │
  │                    ├── 分配 system prompt
  │                    ├── 注入项目上下文
  │                    └── 配置工具集
  │
  ├── 已有匹配？ ──→ 从注册表中选择
  │
  ▼
执行任务
  │
  ▼
验收结果
  │
  ├── 质量达标 ──→ 采纳，更新信任权重
  │
  └── 质量不达标 ──→ 反馈重试 / 替换 agent
  │
  ▼
任务完成 ──→ 销毁 agent（retire），释放资源
```

## 进阶机制

### 竞标机制（Bidding）

一个复杂任务不直接分配，而是广播给多个候选 agent：

```
Meta-Agent: "需要一个实现用户认证模块的 agent"
  │
  ├── Agent A: "我可以做，预估 3 轮对话，使用 JWT"
  ├── Agent B: "我擅长这个，预估 2 轮对话，使用 OAuth2"
  └── Agent C: "我能做，但需要额外查阅文档，预估 5 轮"
  │
  ▼
Meta-Agent 综合评估：成本、质量、速度 ──→ 选择 Agent B
```

这模拟了真实的招聘面试过程：不是指定谁来做，而是让有能力的人来竞标。

### 绩效考核（Performance Tracking）

每个 agent 的表现被持续追踪：

| 指标 | 说明 |
|------|------|
| 完成率 | 分配的任务中成功完成的比例 |
| 代码质量 | lint 通过率、测试覆盖率、review 反馈 |
| 耗时 | 从开始到交付的实际耗时 vs 预估耗时 |
| 修正次数 | 需要返工的次数 |

信任权重随表现动态调整。表现好的 agent 优先获得重要任务，表现差的被降级或不再使用。

### 经验沉淀（Skill Extraction）

高质量的 agent 交互可以被提炼为可复用的 **Skill**：

```
Agent 完成了一个复杂的数据库迁移任务
  │
  ▼
Meta-Agent 分析交互过程
  │
  ▼
提炼为 Skill: "database-migration-sop"
  - 步骤 1: 分析当前 schema
  - 步骤 2: 生成迁移脚本
  - 步骤 3: 编写回滚方案
  - 步骤 4: 验证数据完整性
  │
  ▼
下次同类任务，新 agent 直接加载 Skill，不需要从零开始
```

### 层级组织（Hierarchical Organization）

不限于两层。复杂系统可以形成多级结构：

```
Meta-Agent (CEO)
  ├── VP-Agent (后端)
  │     ├── Agent (数据库)
  │     └── Agent (API 设计)
  ├── VP-Agent (前端)
  │     ├── Agent (UI 组件)
  │     └── Agent (状态管理)
  └── VP-Agent (DevOps)
        ├── Agent (CI/CD)
        └── Agent (监控)
```

每一层只关心自己的抽象级别。CEO 不关心数据库表怎么设计，只关心"后端模块是否完成"。

## 上下文管理

### 共享上下文 vs 私有上下文

子 agent 需要两种上下文：

- **共享上下文** — 项目结构、编码规范、技术栈信息，所有 agent 可见
- **私有上下文** — 当前任务的细节、中间状态，仅该 agent 可见

类似操作系统的共享内存 + 进程私有内存模型。

### 上下文窗口优化

每层 agent 都消耗 token，需要控制层级深度：

- **建议最多 3 层** — CEO → Director → Worker
- **共享上下文用摘要** — 不是把整个代码库塞给每个 agent，而是给结构化的项目摘要
- **工具结果共享** — 一个 agent 的工具调用结果可以被其他 agent 引用，避免重复调用

## 与现有框架对比

| 特性 | AutoGen | CrewAI | MetaGPT | Meta-Agent（本方案） |
|------|---------|--------|----------|------|
| 动态创建 agent | 有限 | 否 | 否 | **是** |
| 按需销毁 | 否 | 否 | 否 | **是** |
| 竞标机制 | 否 | 否 | 否 | **是** |
| 绩效追踪 | 否 | 有限 | 否 | **是** |
| 经验沉淀 | 否 | 否 | 有限 | **是** |
| 层级组织 | 否 | 是 | 是 | **是** |

核心差异：现有框架倾向于**预定义团队**，本方案倾向于**动态组织**。

## 面临的挑战

### Token 成本

多 agent 层级意味着 token 消耗倍增。缓解策略：

- 用小模型做简单任务，大模型只处理复杂决策
- 共享上下文用压缩摘要而非原始数据
- 限制并发 agent 数量

### 一致性

多个 agent 并行修改代码时可能出现冲突：

- 引入文件级锁——同一文件同时只有一个 agent 操作
- 合并前做冲突检测
- Meta-Agent 负责最终集成和验证

### 可观测性

多层 agent 嵌套容易形成黑盒：

- 每个 agent 的决策必须有日志
- 使用 `trace_id` 贯穿整个调用链
- Meta-Agent 提供任务视图，展示当前所有 agent 的状态

## 实现路径

### Phase 1: 基础框架

- 定义 Agent Spec（能力声明格式）
- 实现 Agent Factory（动态创建/销毁）
- 建立能力注册表

### Phase 2: 调度与编排

- 实现任务分解算法
- 建立匹配引擎（任务需求 ↔ agent 能力）
- 添加上下文注入机制

### Phase 3: 进阶能力

- 竞标机制
- 绩效追踪与信任权重
- Skill 提取与复用

### Phase 4: 生产化

- Token 成本优化
- 并发冲突处理
- 可观测性与调试工具

## 参考

- [AutoGen](https://microsoft.github.io/autogen/) — 微软多 agent 对话框架
- [CrewAI](https://www.crewai.com/) — 角色扮演多 agent 框架
- [MetaGPT](https://github.com/geekan/MetaGPT) — 多 agent 协作框架，模拟软件公司
