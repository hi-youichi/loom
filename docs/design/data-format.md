# 数据格式设计

## 目录结构

```
~/.loom/data/
├── memory/                          → [evolution/memory.md](../evolution/memory.md)
├── skills/                          → [evolution/skills.md](../evolution/skills.md)
├── sessions/
│   ├── *.jsonl                      # 会话记录
│   └── index.db                     # SQLite + FTS5 搜索索引
├── evolution/                       → [evolution/gepa.md](../evolution/gepa.md)
├── curator/
│   └── state.json                   → [evolution/curator.md](../evolution/curator.md)
└── levol.yaml                       → [guide/config.md](../guide/config.md)
```

## 会话记录 (JSONL)

每行一条消息，JSON 格式：

```json
{"role": "user", "content": "帮我调试这个 rust 编译错误", "timestamp": "2025-08-19T14:30:00Z"}
{"role": "assistant", "content": "让我看看错误输出...", "tools": [{"name": "read", "args": {"path": "src/main.rs"}}], "timestamp": "2025-08-19T14:30:05Z"}
{"role": "tool", "name": "read", "output": "fn main() { ... }", "timestamp": "2025-08-19T14:30:06Z"}
```

**字段说明**：

| 字段 | 说明 |
|------|------|
| `role` | `user` / `assistant` / `tool` |
| `content` | 消息正文 |
| `tools` | Assistant 调用的工具列表（仅 assistant 角色） |
| `name` | 工具名（仅 tool 角色） |
| `output` | 工具输出（仅 tool 角色） |
| `timestamp` | ISO 8601 时间戳 |

## SQLite FTS5 索引

用于 `levol sessions search <query>` 全文搜索：

```sql
CREATE VIRTUAL TABLE sessions_fts USING fts5(
    session_id,
    role,
    content,
    timestamp
);
```

## 设计原则

1. **人可读**：所有文件都是 Markdown/YAML/JSON，可用编辑器直接查看修改
2. **可 git 管理**：如果想版本控制，直接 git init
3. **可手动修复**：出问题时直接编辑文件，不需要特殊工具
4. **渐进式**：简单的用 Markdown，复杂的（搜索）用 SQLite
