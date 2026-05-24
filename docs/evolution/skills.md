# 技能系统

技能是可复用的工作流，以 Markdown 文件存储，包含 YAML 元数据和正文。

## 技能文件格式 (SKILL.md)

```yaml
---
name: debug-rust-errors
description: "系统性排查 Rust 编译错误和运行时 panic"
triggers:
  - "rust 编译错误"
  - "cargo build 失败"
  - "panic"
lifecycle: active   # active | stale | archived
source: auto        # auto | manual | evolved
---

## 步骤
1. 读取 `cargo build` 输出，定位第一个错误
2. 分析错误类型（类型不匹配 / 生命周期 / 借用检查）
3. 查看相关源码上下文
4. 给出修复建议
```

## 目录结构

```
~/.loom/data/skills/
├── auto/                     # Review Agent 自动创建
│   └── debug-rust/
│       └── SKILL.md
├── curated/                  # 手动创建
│   └── deploy-guide/
│       └── SKILL.md
├── evolved/                  # GEPA 进化产生
│   └── debug-rust/
│       └── SKILL.md
└── curator/
    └── state.json            # Curator 状态（last_used 等）
```

## YAML Frontmatter 字段

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 技能唯一标识，用 kebab-case |
| `description` | string | 否 | 简短描述 |
| `triggers` | string[] | 否 | 触发关键词，用于 `find_matching` |
| `lifecycle` | enum | 否 | `active`（默认）/ `stale` / `archived` |
| `source` | enum | 否 | `auto` / `manual` / `evolved` |

## 技能来源

| 来源 | 说明 | 目录 |
|------|------|------|
| **Auto** | Review Agent 在会话审查时自动创建 | `auto/` |
| **Manual** | 用户通过 `loom skills create` 创建 | `curated/` |
| **Evolved** | GEPA 进化产生的优化版本 | `evolved/` |

## 技能匹配

`find_matching(query, threshold)` 使用 Jaccard 相似度匹配：

1. 精确匹配 trigger → 分数 1.0
2. 子串包含 → 分数 0.85
3. 词集 Jaccard 重叠 → 按比例评分
4. 描述/名称包含查询词 → 分数 0.5

只返回分数 ≥ `threshold`（默认 0.6）的技能，按分数降序排列。

## 技能生命周期

```
Active → (未使用天数超限) → Stale → (超限) → Archived
```

- Auto 技能：60 天未用 → Stale
- Manual 技能：30 天未用 → Stale
- Stale 技能：90 天 → Archived

生命周期由 [Curator](curator.md) 管理。`touch_skill()` 在技能被使用时更新 `last_used` 时间戳。

## 相关命令

```bash
loom skills list                    # 列出所有技能
loom skills show <name>             # 查看详情
loom skills create <name>           # 创建技能
loom skills edit <name>             # 编辑技能
loom skills delete <name>           # 删除技能
loom skills evolve <name>           # 进化技能
```

## 相关文档

- [Curator](curator.md) — 生命周期管理
- [Review Agent](review.md) — 自动技能创建
- [GEPA 进化](gepa.md) — 技能优化
- [命令参考](commands.md) — 完整命令列表
