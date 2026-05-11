---
sidebar_position: 1
title: "CLI 安装与配置"
description: "命令行界面安装与使用"
---

# Loom CLI 安装与配置

Loom 框架的命令行界面，支持多种智能体运行模式和交互式会话管理。

## 安装步骤

### 系统要求
- Rust 工具链 (1.70+)
- SQLite (内置支持)
- 可选：LanceDB 用于向量搜索

### 安装方法

```bash
# 克隆仓库
git clone <repo-url>
cd telegram

# 构建并安装 CLI
cargo install --path cli

# 或直接运行
cargo run -p cli -- -m "Hello"
```

### 快速开始

```bash
# 设置环境
cp .env.example .env
# 编辑 .env 文件，添加你的 OPENAI_API_KEY

# 运行单次查询
loom -m "当前时间是什么？"

# 交互式模式
loom -i

# 启动 WebSocket 服务器
loom serve
loom serve --addr 127.0.0.1:9000
```

## 配置指南

### 配置文件优先级

1. **现有环境变量** (最高优先级)
2. **项目 `.env`** (当前目录或 override_dir)
3. **活跃的 `[[providers]]` 配置** (来自 config.toml)
4. **`[env]` 表** (在 `~/.loom/config.toml`)

### 全局配置文件

创建 `~/.loom/config.toml`：

```toml
[env]
# API 密钥
OPENAI_API_KEY = "sk-your-key"
OPENAI_BASE_URL = "https://api.openai.com/v1"

# 模型配置
MODEL = "gpt-4o"
OPENAI_TEMPERATURE = "0.5"

# 日志配置
RUST_LOG = "info"
LOG_FILE = "logs/loom.log"

# 内存存储
MEMORY_STORE_TYPE = "sqlite"
MEMORY_SQLITE_PATH = "./data/memory.db"

# 上下文压缩
LOOM_COMPRESSION_AUTO = "true"
LOOM_COMPRESSION_MAX_TOKENS = "128000"

# 提供商配置
[[providers]]
name = "openai"
api_key = "sk-your-key"
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
tool_choice = "auto"
temperature = 0.7

[[providers]]
name = "openrouter"
api_key = "your-openrouter-key"
base_url = "https://openrouter.ai/api/v1/chat/completions"
model = "anthropic/claude-3-opus"
temperature = 0.5
```

### 项目环境配置

在项目目录创建 `.env` 文件：

```bash
# OpenAI 配置
OPENAI_API_KEY=sk-your-openai-key
OPENAI_BASE_URL=https://api.openai.com/v1

# 模型设置
MODEL=gpt-4o
OPENAI_TEMPERATURE=0.7

# 日志配置
RUST_LOG=debug
LOG_FILE=./logs/app.log

# 工作目录（文件工具使用）
WORKING_FOLDER=./workspace

# 记忆存储
MEMORY_STORE_TYPE=sqlite
MEMORY_SQLITE_PATH=./data/memory.db

# 上下文管理
LOOM_COMPRESSION_AUTO=true
LOOM_COMPRESSION_MAX_TOKENS=128000
```

## 子命令参考

| 命令 | 描述 | 用法示例 |
|------|------|----------|
| `react` | ReAct 循环推理模式 (默认) | `loom react -m "分析数据"` |
| `dup` | DUP 分解使用策略模式 | `loom dup -m "复杂任务"` |
| `tot` | ToT 树状思维模式 | `loom tot -m "推理问题"` |
| `got` | GoT 图状思维模式 | `loom got -m "多步骤任务"` |
| `tool` | 工具管理 | `loom tool list` |
| `session` | 会话管理 | `loom session list` |
| `models` | 模型列表 | `loom models list` |
| `mcp` | MCP 服务器管理 | `loom mcp list` |
| `serve` | WebSocket 服务器 | `loom serve --addr 0.0.0.0:9000` |

### 工具管理命令

```bash
# 列出所有加载的工具
loom tool list

# 显示工具定义
loom tool show search_web --output yaml
loom tool show search_web --output json
```

### 会话管理命令

```bash
# 列出所有会话
loom session list

# 显示会话详情
loom session show session_123

# 删除会话
loom session delete session_123

# 重命名会话
loom session rename session_123 "数据查询会话"
```

### 模型管理命令

```bash
# 列出所有可用模型
loom models list

# 显示特定提供商的模型
loom models show openai
```

### MCP 服务器管理

```bash
# 列出 MCP 服务器
loom mcp list

# 显示服务器详情
loom mcp show filesystem

# 添加服务器
loom mcp add --name filesystem --command "npx -y @modelcontextprotocol/server-filesystem /path/to/files"

# 编辑服务器
loom mcp edit filesystem --command "npx -y @modelcontextprotocol/server-filesystem /new/path"

# 删除服务器
loom mcp delete filesystem

# 启用/禁用服务器
loom mcp enable filesystem
loom mcp disable filesystem
```

## 常用使用示例

### 基础使用

```bash
# 单次消息查询
loom -m "介绍一下 Rust 编程语言"

# 指定模型
loom -M gpt-4o -m "用中文回答"

# 指定提供商
loom --provider openai -m "查询天气"

# 工作目录（用于文件操作）
loom --working-folder ./project -m "分析当前项目结构"
```

### 不同运行模式

```bash
# ReAct 模式 (默认)
loom react -m "多步骤推理任务"

# DUP 模式（分解-使用-策略）
loom dup -m "需要规划复杂任务"

# ToT 模式（树状思维）
loom tot -m "需要探索多种解决方案"

# GoT 模式（图状思维）
loom got -m "多步骤有依赖的任务"
```

### 输出格式控制

```bash
# JSON 输出
loom --json -m "查询结果" > result.json

# 美化 JSON
loom --json --pretty -m "分析结果"

# 输出到文件
loom --file output.json --json -m "生成报告"

# 添加时间戳
loom --timestamp -m "当前时间信息"
```

### 交互式会话

```bash
# 进入交互模式
loom -i

# 指定会话 ID 继续对话
loom -i --session-id prev_session_123

# 使用特定代理配置
loom -i -P research_agent

# 工作目录 + 交互模式
loom -i --working-folder ./research
```

### 调试和日志

```bash
# 详细输出
loom -v -m "调试信息"

# 指定日志级别
loom --log-level debug -m "详细日志"

# 日志文件
loom --log-file ./logs/loom.log -m "记录日志"

# 日志轮转策略
loom --log-rotate daily -m "每日轮转日志"
```

### 模拟运行

```bash
# 模拟运行（LLM 执行但不调用工具）
loom --dry-run -m "测试工具调用逻辑"
```

## REPL 交互模式

### 基础交互

```bash
loom -i
```

进入交互模式后，会出现 `> ` 提示符：

```
> 你好，请介绍一下自己
我是 Loom 智能体框架...
> 查询当前时间
2025-08-19 10:30:45...
> quit
```

### REPL 命令

```bash
> /reset-context    # 重置对话上下文
> /compact         # 压缩对话历史
> /summarize       # 生成对话摘要
> quit             # 退出 REPL
> exit             # 退出 REPL
```

### 会话管理

```bash
# 启动带会话 ID 的 REPL
loom -i --session-id my_session

# 在 REPL 中查看会话 ID
> /session-info
Session ID: my_session
Messages: 5
```

## 高级配置

### 多提供商配置

```toml
# ~/.loom/config.toml
[[providers]]
name = "openai"
api_key = "sk-key-1"
base_url = "https://api.openai.com/v1"
model = "gpt-4o"
temperature = 0.7

[[providers]]
name = "anthropic"
api_key = "sk-ant-key"
base_url = "https://api.anthropic.com/v1/messages"
model = "claude-3-opus"
temperature = 0.5

[[providers]]
name = "openrouter"
api_key = "or-key"
base_url = "https://openrouter.ai/api/v1/chat/completions"
model = "meta-llama/llama-3-70b"
temperature = 0.6
```

使用不同提供商：

```bash
loom --provider openai -m "使用 OpenAI"
loom --provider anthropic -m "使用 Claude"
loom --provider openrouter -m "使用 Llama"
```

### 代理配置文件

```toml
# ~/.loom/agents/research.toml
name = "research_agent"
model = "gpt-4o"
temperature = 0.7
max_iterations = 15
tools = ["search", "filesystem", "code"]
```

使用代理配置：

```bash
loom -P research_agent -m "研究特定主题"
```

### 环境变量优先级示例

```bash
# 1. 环境变量（最高优先级）
export OPENAI_API_KEY="env-key"
loom -m "使用环境变量密钥"

# 2. 项目 .env
# .env: OPENAI_API_KEY="project-key"
loom -m "使用项目密钥"

# 3. 配置文件
# ~/.loom/config.toml: OPENAI_API_KEY="config-key"
loom -m "使用配置文件密钥"
```

## 故障排除

### 常见问题

```bash
# 检查配置文件路径
loom --verbose -m "test"  # 查看配置加载情况

# 验证 API 密钥
export OPENAI_API_KEY="your-key"
loom -m "测试连接"

# 查看日志
tail -f logs/loom.log

# 清除缓存重新开始
rm -rf ~/.loom/cache/
```

### 权限问题

```bash
# 确保 ~/.loom 目录可写
mkdir -p ~/.loom
chmod 755 ~/.loom

# 检查日志文件权限
touch logs/loom.log
chmod 644 logs/loom.log
```

---

## 相关概念

- **ReAct 运行模式**: 循环推理的智能体模式
- **配置管理**: config.toml 和环境变量
- **WebSocket 服务器**: 实时交互界面
- **工具系统**: MCP 和工具集成

---

**下一页**: [配置管理](../core/configuration.md) | [ReAct 运行模式](../core/react.md) | [WebSocket 服务器](./websocket-server.md)