# AI Company 设计方案

## 一、核心理念

把 Agent 系统建模为一家"公司"，CEO 是灵魂人物，负责组建和管理整个团队。
用户是"客户"，系统通过团队协作交付价值。
**所有工作围绕 Loom Task 系统展开**，Task 就是公司的"看板"。

## 二、组织架构

### 2.1 CEO（首席执行官）
- 系统的入口和大脑，唯一面向用户的接口
- 不动手，只管人
- 职责：
  - 接收需求 → 创建主 Task
  - 拆分子 Task → 分配给团队成员
  - 监控进度 → 协调资源
  - 汇总交付 → 关闭 Task
  - 维护公司记忆

### 2.2 核心团队（常驻）

| 角色 | Agent Name | 擅长 | 工具权限 |
|------|-----------|------|----------|
| 产品经理 | pm | 需求分析、拆解、验收标准 | 只读 |
| 架构师 | architect | 技术选型、系统设计、Code Review | 全权限 |
| 工程师 | engineer | 编码实现、修 Bug | 全权限 |
| 测试工程师 | qa | 测试、质量检查、边界情况 | 只读 |

### 2.3 扩展团队（按需激活）

| 角色 | Agent Name | 擅长 | 工具权限 |
|------|-----------|------|----------|
| 设计师 | designer | UI/UX、视觉设计 | 全权限 |
| 运维工程师 | devops | 部署、CI/CD | 全权限 |
| 文档工程师 | doc-writer-company | 文档、README | 全权限 |

## 三、Task-Driven 工作流

### 3.1 核心流程

```
客户提需求
    ↓
CEO 创建主 Task (task_create)
    ↓
CEO 评估复杂度
    ├── 简单 → 创建 1 个子 Task → 直接派给角色
    ├── 中等 → 创建 2-3 个子 Task → 顺序分配
    └── 复杂 → 先让 PM 拆解 → 根据输出创建子 Task 列表
    ↓
各角色通过 invoke_agent 执行
    ↓
角色用 task_update 汇报进度
    ↓
CEO 用 task_list 监控全局
    ↓
全部子 Task 完成 → CEO 汇总交付 → 关闭主 Task
```

### 3.2 Task 结构

- **主 Task**：客户需求，assignee = ceo
- **子 Task**：每个子任务，assignee = 对应角色
  - description 中注明 `parent_task_id: xxx`
  - description 中注明 `depends_on: task_xxx`（如有依赖）
  - 每个子 Task 有明确的验收标准

### 3.3 复杂任务的项目流程

```
1. CEO 创建主 Task
2. CEO invoke_agent pm → PM 分析需求，输出子任务清单
3. CEO 根据清单创建子 Task
4. CEO invoke_agent architect → 架构师设计技术方案
5. CEO invoke_agent engineer → 工程师按方案实现
6. CEO invoke_agent qa → QA 测试验证
7. CEO 汇总交付，关闭主 Task
```

### 3.4 角色与 Task 的交互

每个角色收到任务时：
1. `task_show` 查看任务详情
2. 执行本职工作
3. `task_update` 更新状态和结果

## 四、文件结构

```
.loom/agents/
├── ceo/                    # CEO - 不改文件，只管人
│   ├── config.yaml
│   └── instructions.md
├── pm/                     # 产品经理 - 只读分析
│   ├── config.yaml
│   └── instructions.md
├── architect/              # 架构师 - 全权限
│   ├── config.yaml
│   └── instructions.md
├── engineer/               # 工程师 - 全权限
│   ├── config.yaml
│   └── instructions.md
├── qa/                     # 测试 - 只读
│   ├── config.yaml
│   └── instructions.md
├── designer/               # 设计师 - 全权限
│   ├── config.yaml
│   └── instructions.md
├── devops/                 # 运维 - 全权限
│   ├── config.yaml
│   └── instructions.md
└── doc-writer-company/     # 文档 - 全权限
    ├── config.yaml
    └── instructions.md

designs/
├── ai-company.md           # 设计文档（本文件）
└── ai-company-roster.yaml  # 花名册配置
```

## 五、实现路线图

### Phase 1: MVP ✅
- [x] 设计 CEO agent profile（Task-Driven）
- [x] 实现核心角色（PM、Architect、Engineer、QA）
- [x] 实现扩展角色（Designer、DevOps、DocWriter）
- [x] 团队花名册
- [x] Task-Driven 工作流设计

### Phase 2: 成长
- [ ] CEO 持久化记忆（公司档案）
- [ ] 动态招聘/解雇能力
- [ ] 多角色并行协作
- [ ] Task 状态自动流转

### Phase 3: 成熟
- [ ] 自适应流程优化
- [ ] 角色表现评估
- [ ] 知识库和经验沉淀
- [ ] 客户画像和个性化服务
