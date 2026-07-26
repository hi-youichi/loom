# Loom

Loom 是一个本地优先的 AI Agent 运行环境。它让开发者在 CLI、IDE（ACP）和消息 Bot 中运行 Agent，并将工具调用、会话、记忆、技能和工作流留在可控的项目上下文中。

Loom 的目标不是替代代码审查或让 Agent 在无人监管下修改系统，而是让它能持续、可解释地完成真实项目任务。

> 当前版本仍在演进。工作流、浏览器扩展和任务模式包含 Experimental 能力；`evolve` 尚未实现。详见[产品文档](docs/prd/README.md)。

## 5 分钟开始

### 1. 准备模型配置

复制示例环境文件并填入你的模型凭据：

```powershell
Copy-Item .env.example .env
```

也可以在用户配置目录（默认 `~/.loom/`，可由 `LOOM_HOME` 覆盖）中创建 `config.toml`。项目根目录中的 `.env` 会覆盖该配置中的环境变量。

### 2. 在项目目录运行 Agent

```powershell
# 运行默认 ReAct Agent
cargo run -p cli -- -m "概览这个仓库，并列出测试入口"

# 明确指定 Agent 可工作的目录
cargo run -p cli -- --working-folder . "定位失败的测试并说明原因"

# 在同一会话中继续
cargo run -p cli -- --session-id bug-123 "现在修复它，并运行相关测试"
```

首次运行前请检查 Agent 的有效工作目录、模型和工具权限。对修改型任务，可使用 `--worktree` 在隔离的 Git worktree 中运行。

## Loom 能做什么

| 能力 | 用途 |
| --- | --- |
| 本地 Agent | 使用 ReAct、DUP、ToT 或 GoT 完成多步骤任务。 |
| 模型与工具 | 配置多个提供商、模型层级、MCP、文件、Shell、Web 等能力。 |
| 持续上下文 | 使用会话、checkpoint、记忆和技能延续项目工作。 |
| 工作流 | 以 Lua 编排多 Agent 任务，查看实例摘要、事件、取消与恢复。 |
| 多入口 | 从 CLI 使用，或通过 ACP 接入兼容 IDE；另有 Telegram 多 Bot。 |

## 常用命令

```text
loom -m "任务"                         # 发起单次任务
loom -i -m "任务"                      # 进入连续交互会话
loom --session-id <id> "继续任务"       # 继续会话
loom session list                       # 查看会话
loom models                             # 查看可用模型
loom tool list                          # 查看工具
loom mcp list                           # 管理 MCP 服务
loom skills list / loom memory list     # 管理可复用上下文
loom acp                                # 作为 ACP server 启动
```

完整操作请见 [CLI 使用指南](docs/guides/cli.md)。

## 文档

- [文档总览](docs/README.md)
- [CLI 使用指南](docs/guides/cli.md)
- [IDE / ACP 集成](docs/guides/acp-ide.md)
- [`.loom/` 与配置参考](docs/reference/loom-directory-and-config.md)
- [工作流指南](docs/guides/workflows.md)
- [安全与隐私](docs/guides/security-and-privacy.md)
- [故障排查](docs/guides/troubleshooting.md)
- [产品 PRD](docs/prd/README.md)

## 开发

```powershell
cargo build -p cli
cargo test -p cli
```

Loom 是 Rust workspace；crate 与实验模块的实现细节应从各模块的 `Cargo.toml`、源码和 `docs/design/` 查阅。用户体验和范围以 `docs/` 下的指南与 PRD 为准。

## 许可证

MIT
