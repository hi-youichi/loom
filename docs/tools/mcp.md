---
sidebar_position: 2
title: "MCP 集成"
description: "Model Context Protocol 工具发现和调用"
---

# MCP (Model Context Protocol) 集成

标准化的工具发现和调用协议，让 Loom 智能体能够与 MCP 服务器无缝集成，扩展工具能力。

## 使用场景

| 场景 | 适用性 | 说明 |
|------|--------|------|
| 需要外部工具集成 | ✅ 最佳选择 | 标准协议支持各种 MCP 服务器 |
| 文件系统操作 | ✅ 推荐使用 | MCP 文件系统服务器提供安全文件访问 |
| 代码分析工具 | ✅ 推荐使用 | GitHub MCP 等代码仓库工具 |
| 搜索和数据访问 | ✅ 专门设计 | Exa MCP 等专业搜索服务 |
| 自定义工具开发 | ✅ 灵活扩展 | 支持开发自己的 MCP 服务器 |

## 核心概念

### MCP 协议集成

**MCP (Model Context Protocol)** 是工具发现和调用的标准化协议，Loom 通过以下组件实现集成：

- **McpToolSource**: 实现 `ToolSource` trait，桥接 MCP 服务器到 Loom
- **McpToolAdapter**: 将 MCP 工具适配为 Loom 工具格式
- **register_mcp_tools**: 辅助函数，批量注册 MCP 工具

### 传输方式

**Stdio 传输**: 通过子进程 stdio 进行 JSON-RPC 通信
```rust
McpServerDef::Stdio {
    name: "filesystem".to_string(),
    command: "npx".to_string(),
    args: vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp".to_string()],
    env: HashMap::new(),
}
```

**HTTP 传输**: 通过 HTTP POST 请求与 MCP 服务器通信
```rust
McpServerDef::Http {
    name: "exa".to_string(),
    url: "https://mcp.exa.ai/mcp".to_string(),
    headers: vec![
        ("Authorization".to_string(), "Bearer ${EXA_API_KEY}".to_string())
    ].into_iter().collect(),
}
```

## 代码示例

### 配置 MCP 服务器

```rust
use loom::tool_source::{McpToolSource, McpSessionKind};
use loom::tools::{register_mcp_tools, AggregateToolSource};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建聚合工具源
    let aggregate = AggregateToolSource::new();

    // 创建文件系统 MCP 服务器 (stdio)
    let filesystem_mcp = McpToolSource::new(
        "filesystem".to_string(),
        "npx".to_string(),
        vec!["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        HashMap::new(),
    ).await?;

    // 注册 MCP 工具
    register_mcp_tools(&aggregate, Arc::new(filesystem_mcp)).await?;

    println!("MCP 工具注册完成，可用工具数量: {}", aggregate.list_tools().await?.len());

    Ok(())
}
```

### HTTP MCP 服务器配置

```rust
use loom::tool_source::{McpToolSource, McpSessionKind};
use loom::tools::{register_mcp_tools, AggregateToolSource};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let aggregate = AggregateToolSource::new();

    // 创建 Exa 搜索 MCP 服务器 (HTTP)
    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", std::env::var("EXA_API_KEY")?)
    );

    let exa_mcp = McpToolSource::new_http(
        "exa".to_string(),
        "https://mcp.exa.ai/mcp".to_string(),
        headers,
    ).await?;

    register_mcp_tools(&aggregate, Arc::new(exa_mcp)).await?;

    // 查看注册的工具
    let tools = aggregate.list_tools().await?;
    for tool in tools {
        println!("注册工具: {} - {}", tool.name, tool.description);
    }

    Ok(())
}
```

### 在 Agent 中使用 MCP 工具

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::tool_source::{McpToolSource, AggregateToolSource};
use loom::tools::register_mcp_tools;
use std::sync::Arc;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建工具源聚合器
    let aggregate = AggregateToolSource::new();

    // 添加文件系统 MCP 工具
    let fs_mcp = McpToolSource::new(
        "filesystem".to_string(),
        "npx".to_string(),
        vec!["-y", "@modelcontextprotocol/server-filesystem", "./workspace"],
        HashMap::new(),
    ).await?;

    register_mcp_tools(&aggregate, Arc::new(fs_mcp)).await?;

    // 构建配置，使用自定义工具源
    let config = ReactBuildConfig {
        custom_tool_source: Some(Box::new(aggregate)),
        model: "gpt-4o".to_string(),
        ..Default::default()
    };

    // 构建并运行 ReAct 智能体
    let runner = build_react_runner(&config, None, true).await?;

    let result = runner.invoke(
        "请帮我读取当前目录下的 README.md 文件内容，并总结其中的要点"
    ).await?;

    println!("智能体回复: {}", result.messages.last().unwrap());

    Ok(())
}
```

### 通过 config.json 配置 MCP

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": {
        "CUSTOM_VAR": "value"
      },
      "disabled": false
    },
    "exa": {
      "url": "https://mcp.exa.ai/mcp",
      "headers": {
        "Authorization": "Bearer ${EXA_API_KEY}"
      }
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_TOKEN": "${GITHUB_TOKEN}"
      }
    }
  }
}
```

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use env_config::{discover_mcp_config_path, parse_mcp_config, McpServerDef};
use loom::tool_source::{McpToolSource, AggregateToolSource};
use loom::tools::register_mcp_tools;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 自动发现 MCP 配置文件
    let config_path = discover_mcp_config_path(None, None)
        .ok_or("未找到 MCP 配置文件")?;

    // 解析配置
    let server_defs: Vec<McpServerDef> = parse_mcp_config(&config_path)?;

    // 创建工具源聚合器
    let aggregate = AggregateToolSource::new();

    // 根据配置创建并注册 MCP 服务器
    for server_def in server_defs {
        match server_def {
            McpServerDef::Stdio { name, command, args, env } => {
                let mcp = McpToolSource::new(name, command, args, env).await?;
                register_mcp_tools(&aggregate, Arc::new(mcp)).await?;
            },
            McpServerDef::Http { name, url, headers } => {
                let mcp = McpToolSource::new_http(name, url, headers).await?;
                register_mcp_tools(&aggregate, Arc::new(mcp)).await?;
            }
        }
    }

    // 使用配置好的工具源构建智能体
    let config = ReactBuildConfig {
        custom_tool_source: Some(Box::new(aggregate)),
        ..Default::default()
    };

    let runner = build_react_runner(&config, None, true).await?;
    let result = runner.invoke("搜索最新的 Rust 编程教程").await?;

    println!("搜索结果: {}", result.messages.last().unwrap());

    Ok(())
}
```

### GitHub MCP 集成

```rust
use loom::agent::react::{build_react_runner, ReactBuildConfig};
use loom::tool_source::{McpToolSource, AggregateToolSource};
use loom::tools::register_mcp_tools;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let aggregate = AggregateToolSource::new();

    // 配置 GitHub MCP 服务器
    let mut env = HashMap::new();
    env.insert("GITHUB_TOKEN".to_string(), std::env::var("GITHUB_TOKEN")?);

    let github_mcp = McpToolSource::new(
        "github".to_string(),
        "npx".to_string(),
        vec!["-y", "@modelcontextprotocol/server-github"],
        env,
    ).await?;

    register_mcp_tools(&aggregate, Arc::new(github_mcp)).await?;

    let config = ReactBuildConfig {
        custom_tool_source: Some(Box::new(aggregate)),
        model: "gpt-4o".to_string(),
        ..Default::default()
    };

    let runner = build_react_runner(&config, None, true).await?;

    let result = runner.invoke(
        "帮我查看 rust-lang/rust 仓库的最新 issue，并分析主要问题类型"
    ).await?;

    println!("GitHub 分析结果: {}", result.messages.last().unwrap());

    Ok(())
}
```

## 配置参考

### McpServerDef 结构

| 字段 | 类型 | 必需 | 说明 |
|------|------|------|------|
| **Stdio 模式** | | | |
| name | String | ✅ | 服务器唯一标识符 |
| command | String | ✅ | 启动命令 |
| args | `Vec<String>` | ✅ | 命令参数 |
| env | `HashMap<String, String>` | ❌ | 环境变量 |
| **HTTP 模式** | | | |
| name | String | ✅ | 服务器唯一标识符 |
| url | String | ✅ | MCP 服务器 URL |
| headers | `HashMap<String, String>` | ❌ | HTTP 请求头 |

### 环境变量

| 变量名 | 用途 | 示例 |
|--------|------|------|
| `EXA_API_KEY` | Exa 搜索 API 密钥 | `your-exa-api-key` |
| `GITHUB_TOKEN` | GitHub 访问令牌 | `ghp_xxx` |
| `MCP_VERBOSE` | 启用详细日志 | `1` 或 `true` |
| `MCP_EXA_URL` | 覆盖默认 Exa URL | `https://custom-exa.com/mcp` |

### 配置文件位置

MCP 配置文件按以下优先级查找：
1. 命令行指定的路径
2. 项目目录：`.loom/mcp.json`
3. 全局目录：`~/.loom/mcp.json`

## MCP 工具调用流程

```
配置阶段
    ↓
发现 MCP 配置文件 → 解析 McpServerDef
    ↓
创建 McpToolSource (stdio/http)
    ↓
调用 list_tools() → 获取工具列表
    ↓
register_mcp_tools() → 创建 McpToolAdapter
    ↓
运行阶段
    ↓
LLM 决定调用工具 → ThinkNode
    ↓
ActNode 调用 ToolSource::call_tool()
    ↓
McpToolAdapter 委托给 McpToolSource
    ↓
发送 JSON-RPC 请求 → tools/call
    ↓
解析响应 → ToolCallContent
    ↓
返回结果 → ObserveNode
```

## 最佳实践

### 配置管理
- 使用配置文件集中管理 MCP 服务器
- 为不同环境使用不同的配置文件
- 敏感信息通过环境变量传递
- 定期更新 MCP 服务器版本

### 性能优化
- 对频繁使用的工具选择 HTTP 传输方式
- 合理设置超时时间，避免长时间等待
- 监控 MCP 服务器的响应时间和可用性
- 缓存工具规格信息减少重复请求

### 错误处理
- 实现完善的 MCP 服务器连接失败处理
- 为不稳定的 MCP 服务器配置重试机制
- 提供降级方案，当 MCP 工具不可用时回退到内置工具
- 记录详细的错误日志便于调试

### 安全考虑
- 限制文件系统 MCP 的访问路径
- 为 HTTP MCP 服务器使用 HTTPS
- 定期轮换 API 密钥和访问令牌
- 验证 MCP 服务器的身份和响应内容

---

## 相关概念

- **工具系统**: Loom 工具开发和集成指南
- **ReAct 运行模式**: 基础的循环推理模式
- **自定义工具**: 开发自定义工具的完整指南

---

**下一页**: [工具系统](../core/tool-system.md) | [自定义工具开发](./custom-tools.md) | [ReAct 运行模式](../core/react.md)