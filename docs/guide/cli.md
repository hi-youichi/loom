# CLI 命令参考

## 会话

| 命令 | 说明 |
|------|------|
| `levol chat` | 核心入口，带进化增强的会话 |
| `levol chat --no-review` | 跳过后台审查 |
| `levol chat --no-memory` | 跳过记忆注入 |

## 配置

| 命令 | 说明 |
|------|------|
| `levol init` | 初始化数据目录和配置 |
| `levol config show` | 显示当前配置 |
| `levol config set <key> <value>` | 修改配置 |
| `levol config set cli.backend codex` | 切换底层 CLI |

## 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功 |
| 1 | 一般错误 |
| 2 | 配置错误 |
| 3 | 底层 CLI 未找到 |

## 进化相关命令

技能、记忆、审查、维护等命令统一在 [evolution/commands.md](../evolution/commands.md) 中。
