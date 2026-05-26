# 配置参考 (config.yaml)

配置文件位于 `~/.loom/config.yaml`，可用 `loom config set` 修改。

## 完整配置

```yaml
version: 1

cli:
  backend: "loom"                    # "loom" | "codex"
  context_file: "CLAUDE.md"          # 自动按 backend 切换
  loom:
    command: "loom"
    context_file: "CLAUDE.md"
  codex:
    command: "codex"
    context_file: "AGENTS.md"
    quiet_flag: "--quiet"

model:
  main: "claude-sonnet-4-20250514"
  review: "claude-sonnet-4-20250514"
  evolution: "openai/gpt-4.1"

# memory / skills / review / curator / evolution 配置
# 详见 → evolution/config.md
```

> 进化子系统的完整配置（memory、skills、review、curator、evolution）已移至 [evolution/config.md](../evolution/config.md)。

## 配置项详解

### cli — 底层 CLI 选择

| 键 | 默认 | 说明 |
|----|------|------|
| `cli.backend` | `loom` | 底层 CLI 选择 |
| `cli.context_file` | 自动 | 按 backend 自动设置，一般不需要手动改 |
| `cli.loom.command` | `loom` | Loom 可执行文件路径 |
| `cli.codex.command` | `codex` | Codex 可执行文件路径 |
| `cli.codex.quiet_flag` | `--quiet` | Codex 非交互模式 flag |

### model — 模型选择

| 键 | 说明 |
|----|------|
| `model.main` | 主会话使用的模型 |
| `model.review` | 后台审查使用的模型（可用较便宜的） |
| `model.evolution` | GEPA 进化使用的模型（需要支持 function calling） |


