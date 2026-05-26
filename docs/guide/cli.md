# CLI 命令参考

## 基本用法

```bash
loom -m "你的消息"            # 单次查询 (默认 ReAct)
loom -i                        # 交互式 REPL 模式
loom --json -m "查询"          # JSON 流式输出
```

## 子命令

| 子命令 | 说明 | 示例 |
|--------|------|------|
| `react` | ReAct 循环推理 (默认) | `loom react -m "分析数据"` |
| `dup` | DUP 分解-使用-策略 | `loom dup -m "复杂任务"` |
| `tot` | ToT 树状思维 | `loom tot -m "多方案探索"` |
| `got` | GoT 图状思维 | `loom got --got-adaptive -m "多步任务"` |
| `tool` | 工具管理 | `loom tool list` |
| `session` | 会话管理 | `loom session list` |
| `models` | 模型列表 | `loom models list` |
| `mcp` | MCP 服务器管理 | `loom mcp list` |
| `agent` | Agent 配置管理 | `loom agent list` |
| `goal` | 自治目标循环 | `loom goal "实现功能"` |
| `skills` | 技能管理 | `loom skills list` |
| `evolve` | 技能进化管理 | `loom evolve run` |
| `curator` | 技能生命周期 | `loom curator` |
| `memory` | 记忆查看/编辑 | `loom memory show` |
| `review` | 会话审查 | `loom review session <id>` |
| `review-skill` | 技能审查 | `loom review-skill --input file` |
| `task` | 公司任务管理 | `loom task new "任务描述"` |
| `serve` | WebSocket 服务器 | `loom serve --addr 127.0.0.1:9000` |

## 常用全局参数

- `-m TEXT` — 用户消息
- `-M MODEL` — 模型 (如 `gpt-4o`)
- `-P NAME` — Agent 配置名
- `-v` — 详细输出
- `--json` — JSON 流式输出
- `--dry` — 模拟运行 (不执行工具)
- `--worktree` / `-w` — 隔离 git worktree 执行
- `--session-id ID` — 会话 ID 续传
- `--debug-llm` — 调试 LLM (打印完整 prompt)

## REPL 命令

进入交互模式 (`loom -i`) 后支持：

- `/reset-context` — 重置对话上下文
- `/compact` — 压缩历史
- `/summarize` — 生成摘要
- `quit` / `exit` — 退出 REPL

## 配置优先级

1. 环境变量 (最高)
2. 项目 `.env`
3. `config.toml` 中的 `[[providers]]` 和 `[env]`
