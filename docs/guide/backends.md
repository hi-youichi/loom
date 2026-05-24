# Backend 切换指南

Levol 通过 Backend Adapter 支持多个底层 AI Coding CLI。当前支持 Loom 和 Codex。

## 快速切换

```bash
# 查看当前 backend
$ levol config show cli.backend
loom

# 切换到 Codex
$ levol config set cli.backend codex
[levol] Backend switched to codex (context file: AGENTS.md)

# 切换回 Loom
$ levol config set cli.backend loom
[levol] Backend switched to loom (context file: CLAUDE.md)
```

## Loom vs Codex 对比

| 维度 | Loom | Codex (OpenAI) |
|------|------|----------------|
| 启动命令 | `loom chat` | `codex` |
| 非交互模式 | `echo X \| loom chat` | `codex --quiet` |
| Context 文件 | `CLAUDE.md` | `AGENTS.md` |
| 单次 Agent 执行 | `loom agent run` | ❌ 无等价命令 |
| 模型选择 | 内置 | `--model` flag |
| 退出码 | 待确认 | 0=成功, 1=error |

## 切换时发生了什么

切换 backend 只改变底层 CLI 的调用方式和 context 注入目标，进化系统的数据（记忆、技能、会话）完全共享：

```
Loom 会话 ──→ 共享记忆/技能/会话 ←── Codex 会话
```

所以你在 Loom 里积累的技能和偏好，切换到 Codex 后一样能用。

## 自定义 CLI 路径

如果 CLI 不在 PATH 中：

```yaml
cli:
  backend: "loom"
  loom:
    command: "/usr/local/bin/loom"
  codex:
    command: "/opt/homebrew/bin/codex"
```

## 支持新的 Backend

如果你想让 Levol 支持其他 CLI（如 Claude Code、Aider 等），参考 [backend-trait.md](../dev/backend-trait.md)。
