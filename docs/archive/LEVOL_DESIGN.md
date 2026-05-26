# Levol 设计方案

> 本文档已拆分为多文件结构，详见 [docs/](docs/README.md)

## 文档结构

```
docs/
├── README.md                        # 总览（TL;DR + 概念 + 架构）
│
├── guide/                           # 使用视角
│   ├── getting-started.md           # 安装、初始化、第一次会话
│   ├── cli.md                       # 全部命令参考
│   ├── config.md                    # levol.yaml 配置项说明
│   └── backends.md                  # Loom vs Codex 切换指南
│
├── design/                          # 设计视角
│   ├── architecture.md              # 三层架构设计
│   ├── session-lifecycle.md         # 会话全流程详解
│   ├── data-format.md               # 数据格式设计
│   └── decisions.md                 # 关键设计决策记录
│
├── dev/                             # 实现视角
│   ├── tech-stack.md                # 技术选型 + 项目结构 + 接口定义
│   ├── backend-trait.md             # Backend trait + 写新 Adapter 指南
│   └── roadmap.md                   # 路线图 + 风险 + Hermes 对比
│
└── evolution/                       # 进化子系统
    ├── README.md                    # 进化系统概述
    ├── skills.md                    # 技能系统设计
    ├── review.md                    # 后台审查机制
    ├── curator.md                   # 技能定期维护
    └── gepa.md                      # DSPy+GEPA 进化优化
```
