# 记忆系统

记忆是跨会话持久化的用户偏好和项目事实，存储为 Markdown 文件。

## 文件结构

```
~/.loom/data/memory/
├── USER.md              # 用户画像（声明式事实）
├── PROJECT.md           # 项目上下文
└── FACTS.md             # 通用持久事实
```

## 三种记忆文件

### USER.md — 用户画像

存储跨项目的用户偏好，写为**声明式事实**：

```markdown
- 偏好 Rust 语言，避免 Python
- 使用 vim 作为编辑器
- 终端使用 zsh + oh-my-zsh
- commit message 风格：conventional commits
```

### PROJECT.md — 项目上下文

存储当前项目的架构决策和关键信息：

```markdown
- 技术栈：Rust + Tokio + SQLite
- 数据库路径：./data/app.db
- 测试命令：cargo test --all
- API 端点前缀：/api/v1
```

### FACTS.md — 通用事实

存储具体的事实数据（API key 引用、版本号、URL）：

```markdown
- Rust stable 版本：1.85
- tokio 当前使用版本：1.42
- 项目使用 workspace 多 crate 结构
```

## 注入机制

`load_all_for_prompt()` 在 system prompt 组装阶段注入记忆内容：

1. 按 **FACTS > PROJECT > USER** 优先级加载
2. 总长度受 `max_memory_chars`（默认 4000）限制
3. 超限时按优先级从低到高截断

## 截断策略

单个文件超过 `max_chars`（默认 8000）时自动截断：

1. 保留 YAML frontmatter 等结构性元数据
2. 正文按 `---` 分隔的条目，从最旧的开始移除
3. 确保总长度 ≤ max_chars

## 相关命令

```bash
loom memory show                     # 显示所有记忆
loom memory edit USER                # 编辑用户画像
loom memory edit PROJECT             # 编辑项目上下文
loom memory edit FACTS               # 编辑事实
loom memory search "rust"            # 搜索记忆
```

## 写入时机

| 事件 | 写入的文件 | 说明 |
|------|-----------|------|
| 会话结束 | USER / PROJECT / FACTS | Review Agent 分析会话后写入 |
| 用户手动 | 任意 | `loom memory edit` 直接编辑 |
| 技能创建 | FACTS | 记录新技能的存在 |

## 相关文档

- [Review Agent](review.md) — 自动写入记忆
- [配置参考](config.md) — memory 配置项
- [命令参考](commands.md) — memory 命令
