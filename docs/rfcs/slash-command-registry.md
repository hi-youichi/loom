# RFC: Loom Slash Command 统一注册系统

> 状态: Draft  
> 作者: Code Review  
> 日期: 2025-08-19  
> 范围: `loom/src/command/`, `loom-acp/src/agent.rs`, `telegram-bot/src/command/`, `telegram-bot/src/pipeline/mod.rs`

---

## 1. 背景与动机

Loom 项目当前有 **4 套独立的命令处理机制**，同一个 slash 命令（如 `/reset`）在 ACP、Telegram、CLI 中的注册和分发逻辑各不相同，且互不共享：

| 层级 | 文件 | 注册方式 | 命令发现 |
|---|---|---|---|
| Core parser | `loom/src/command/parser.rs` | 手写 `match token` | 编译时枚举 |
| Core executor | `loom/src/command/builtins.rs` | `trait ResetState + CompactState + SummarizeState` | 编译时枚举 |
| ACP prompt | `loom-acp/src/agent.rs:647-713` | 复用 core parser + 手写 `match cmd` | 无 |
| Telegram pipeline | `telegram-bot/src/pipeline/mod.rs:126-250` | 复用 core parser + 手写 `match cmd` | 无 |
| Telegram bot | `telegram-bot/src/command/mod.rs` | `BotCommand` trait + `CommandDispatcher` | 运行时 Vec 遍历 |

### 1.1 当前问题

**P0 — ACP 静默吞掉命令**

`agent.rs:709` 的 `_ =>` 分支将 `/compact`、`/summarize` 静默返回 `EndTurn`，用户无反馈：

```rust
// loom-acp/src/agent.rs:647-713
if let Some(cmd) = loom::command::parse(text) {
    match cmd {
        Command::ResetContext => { /* 处理 */ }
        Command::Goal { description } => { /* 处理 */ }
        Command::Models { .. } | Command::ModelsUse { .. } => { /* 忽略 */ }
        _ => { return Ok(PromptResponse::new(StopReason::EndTurn)); }  // ← bug
    }
}
```

**P1 — 新增命令需修改 N 处**

添加一个新 slash 命令需要改动至少 4 个位置：
1. `loom/src/command/command.rs` — 添加 `Command` enum 变体
2. `loom/src/command/parser.rs` — 添加解析规则
3. `loom/src/command/builtins.rs` — 添加执行逻辑
4. 每个接入层（ACP `agent.rs`、Telegram `pipeline/mod.rs`）— 添加 match 分支

**P2 — ACP 未向 IDE 声明可用命令**

根据 [ACP Slash Commands 规范](https://agentclientprotocol.com/protocol/slash-commands)，Agent 应通过 `available_commands_update` 通知向客户端声明可用命令，IDE 才能提供命令补全。当前 `new_session()` 未发送此通知。

**P3 — Telegram 命令分发不一致**

`telegram-bot/src/pipeline/mod.rs` 中的命令处理有三层分派：
1. `loom_command::parse(text)` → 手写 match（处理 `/reset`、`/compact`、`/summarize`）
2. `CommandDispatcher::try_dispatch()`（处理 `/reset`、`/status`、`/model` 精确匹配）
3. `try_handle_model_command_input()`（处理 `/model gpt`、`/model use gpt-4o` 等子命令）

`/reset` 被**两层都处理**：先在 core parser 层处理（`pipeline/mod.rs:128`），又在 `CommandDispatcher` 层有 `ResetCommand`（`command/mod.rs:69`）。虽然当前 core 先匹配所以 dispatcher 的 ResetCommand 永远不会被触发，但这造成了逻辑混乱。

**P4 — 无法动态注册/发现命令**

没有任何运行时机制让外部模块或插件注册自定义命令。

---

## 2. 目标

1. **单一来源（Single Source of Truth）**: 命令定义、解析、描述集中在 core 层
2. **一次注册，到处可用**: 新命令只需在一个地方实现，所有接入层自动支持
3. **ACP 合规**: 通过 `available_commands_update` 向 IDE 声明命令
4. **可扩展**: 支持未来插件注册自定义命令
5. **向后兼容**: 不破坏现有命令行为，渐进式迁移

---

## 3. 详细方案

### 3.1 Core 层：命令注册表（Command Registry）

在 `loom/src/command/` 下新增 `registry.rs`，引入基于 trait 的注册机制。

#### 3.1.1 核心类型定义

```rust
// loom/src/command/registry.rs

use async_trait::async_trait;
use std::any::Any;

/// 命令参数定义（用于 ACP available_commands_update 声明）
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandArgSchema {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
}

/// 命令元数据（用于 ACP 声明和 help 展示）
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandInfo {
    /// 命令名，如 "/reset"
    pub name: String,
    /// 别名列表，如 ["/clear", "/new"]
    pub aliases: Vec<String>,
    /// 人类可读描述
    pub description: String,
    /// 参数 schema
    pub args: Vec<CommandArgSchema>,
}

/// 命令执行上下文（由各接入层提供）
pub struct CommandContext<'a> {
    /// 解析后的命令参数（具体类型由命令实现定义）
    pub parsed_args: Box<dyn Any + Send>,
    /// 接入层特定的状态（ACP session / Telegram chat_id 等）
    pub platform: &'a mut dyn PlatformContext,
}

/// 接入层上下文 trait（ACP / Telegram / CLI 各自实现）
pub trait PlatformContext: Send + Sync {
    /// 重置上下文
    fn reset_context(&mut self) -> Result<String, String>;
    /// 获取平台名称
    fn platform_name(&self) -> &str;
}

/// 命令执行结果
#[derive(Debug, Clone)]
pub enum CommandOutcome {
    /// 命令已处理，返回文本回复
    Handled(String),
    /// 命令已处理，返回错误提示
    Error(String),
    /// 命令已处理，无回复
    HandledSilent,
    /// 命令未被识别，传递给后续处理
    PassThrough,
    /// 命令需要异步 LLM 调用（compact / summarize）
    AsyncRequired,
}

/// Slash 命令 trait
#[async_trait]
pub trait SlashCommand: Send + Sync {
    /// 命令元数据
    fn info(&self) -> CommandInfo;

    /// 尝试解析输入文本。匹配成功返回解析后的参数。
    fn parse(&self, text: &str) -> Option<Box<dyn Any + Send>>;

    /// 同步执行命令
    fn execute(&self, ctx: &mut CommandContext<'_>) -> Result<CommandOutcome, String>;

    /// 异步执行命令（需要 LLM 的命令覆盖此方法）
    async fn execute_async(
        &self,
        ctx: &mut CommandContext<'_>,
        llm: &dyn crate::llm::LlmClient,
    ) -> Result<CommandOutcome, String> {
        let args = ctx.parsed_args.downcast_ref::<CompactArgs>()
            .ok_or("invalid args type")?;
        // 实际 compact 逻辑（从 builtins.rs 迁移）:
        // 1. 从 platform context 获取当前 messages
        // 2. 构建 compact prompt（如有 instructions 则附加）
        // 3. 调用 llm.summarize() 获取摘要
        // 4. 替换 messages 为摘要 + system prompt
        // 5. 返回 Compacted(messages)
        let _ = (args, llm);
        Ok(CommandOutcome::Handled("Context compacted.".into()))
    }
}
```

#### 3.1.2 注册表

```rust
// loom/src/command/registry.rs (续)

/// 命令注册表（全局单例）
pub struct CommandRegistry {
    commands: Vec<Box<dyn SlashCommand>>,
    /// name/alias → index 的查找表
    lookup: std::collections::HashMap<String, usize>,
}

impl CommandRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            lookup: HashMap::new(),
        }
    }

    /// 注册一个命令
    pub fn register(&mut self, cmd: impl SlashCommand + 'static) {
        let info = cmd.info();
        let idx = self.commands.len();
        self.lookup.insert(info.name.clone(), idx);
        for alias in &info.aliases {
            self.lookup.insert(alias.clone(), idx);
        }
        self.commands.push(Box::new(cmd));
    }

    /// 解析文本，返回匹配到的命令和解析后的参数
    pub fn parse(&self, text: &str) -> Option<(&dyn SlashCommand, Box<dyn Any + Send>)> {
        let trimmed = text.trim();
        let token = trimmed.split_whitespace().next()?;
        let token_lower = token.to_lowercase();

        // 仅按 lookup 表精确匹配，不做回退遍历
        // 命令的 parse() 应仅匹配自己的 name/alias 前缀
        if let Some(&idx) = self.lookup.get(&token_lower) {
            let cmd = &self.commands[idx];
            if let Some(args) = cmd.parse(trimmed) {
                return Some((cmd.as_ref(), args));
            }
        }

        None
    }

    /// 获取所有命令的元信息
    pub fn command_infos(&self) -> Vec<CommandInfo> {
        self.commands.iter().map(|c| c.info()).collect()
    }

    /// 按 name 查找命令
    pub fn get_by_name(&self, name: &str) -> Option<&dyn SlashCommand> {
        self.lookup.get(name).map(|&idx| self.commands[idx].as_ref())
    }
}
```

#### 3.1.3 全局实例

```rust
// loom/src/command/registry.rs (续)

use std::sync::{LazyLock, RwLock};

static REGISTRY: LazyLock<RwLock<CommandRegistry>> = LazyLock::new(|| {
    let mut reg = CommandRegistry::new();
    reg.register(ResetCommand);
    reg.register(CompactCommand);
    reg.register(SummarizeCommand);
    reg.register(ModelsCommand);
    reg.register(GoalCommand);
    RwLock::new(reg)
});

/// 获取全局注册表的只读引用（用于 parse / command_infos）
pub fn global_registry() -> &'static RwLock<CommandRegistry> {
    &REGISTRY
}

/// 注册插件命令（仅在初始化阶段调用）
pub fn register_plugin_command(cmd: impl SlashCommand + 'static) {
    let mut reg = REGISTRY.write().unwrap();
    reg.register(cmd);
}
```

### 3.2 内置命令实现示例

以 `/reset` 和 `/compact` 为例：

```rust
// loom/src/command/builtins_v2.rs

struct ResetCommand;

#[async_trait]
impl SlashCommand for ResetCommand {
    fn info(&self) -> CommandInfo {
        CommandInfo {
            name: "/reset".into(),
            aliases: vec!["/clear".into(), "/new".into()],
            description: "Clear conversation context".into(),
            args: vec![],
        }
    }

    fn parse(&self, text: &str) -> Option<Box<dyn Any + Send>> {
        let token = text.trim().split_whitespace().next()?;
        match token {
            "/reset" | "/clear" | "/new" => Some(Box::new(())),
            _ => None,
        }
    }

    fn execute(&self, ctx: &mut CommandContext<'_>) -> Result<CommandOutcome, String> {
        ctx.platform.reset_context()?;
        Ok(CommandOutcome::Handled("Context cleared.".into()))
    }
}

struct CompactCommand;

#[derive(Clone)]
struct CompactArgs {
    instructions: Option<String>,
}

#[async_trait]
impl SlashCommand for CompactCommand {
    fn info(&self) -> CommandInfo {
        CommandInfo {
            name: "/compact".into(),
            aliases: vec![],
            description: "Compact conversation history with optional focus instructions".into(),
            args: vec![CommandArgSchema {
                name: "instructions".into(),
                description: Some("Focus instructions for compaction".into()),
                required: false,
            }],
        }
    }

    fn parse(&self, text: &str) -> Option<Box<dyn Any + Send>> {
        let trimmed = text.trim();
        let token = trimmed.split_whitespace().next()?;
        if token != "/compact" { return None; }
        let instructions = trimmed
            .strip_prefix("/compact")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Some(Box::new(CompactArgs { instructions }))
    }

    fn execute(&self, _ctx: &mut CommandContext<'_>) -> Result<CommandOutcome, String> {
        // compact 需要 LLM，同步模式下无法完成
        Ok(CommandOutcome::AsyncRequired)
    }

    async fn execute_async(
        &self,
        ctx: &mut CommandContext<'_>,
        llm: &dyn crate::llm::LlmClient,
    ) -> Result<CommandOutcome, String> {
        let args = ctx.parsed_args.downcast_ref::<CompactArgs>()
            .ok_or("invalid args type")?;
        // ... 实际 compact 逻辑（从 builtins.rs 迁移）
        Ok(CommandOutcome::Handled("Context compacted.".into()))
    }
}
```

### 3.3 ACP 接入层改造

#### 3.3.1 new_session 发送 available_commands_update

根据 ACP 规范，命令不在 `initialize` 的 `agentCapabilities` 中声明，而是在 `new_session()` 后通过 `available_commands_update` 通知发送。

```rust
// loom-acp/src/agent.rs — new_session() 末尾添加

pub async fn new_session(&self, args: NewSessionRequest) -> Result<NewSessionResponse> {
    // ... 现有逻辑 ...
    
    // 发送 available_commands_update
    if let Some(ref tx) = self.session_update_tx {
        let notifier = SessionNotifier::new(tx.clone(), session_id.clone());
        let reg = loom::command::registry::global_registry().read().unwrap();
        let commands = reg.command_infos();
        drop(reg);
        notifier.send_available_commands(&commands).await;
    }
    
    Ok(response)
}
```

#### 3.3.2 SessionNotifier 新增方法

```rust
// loom-acp/src/stream_bridge.rs — SessionNotifier impl 新增

pub async fn send_available_commands(&self, commands: &[loom::command::registry::CommandInfo]) {
    let commands_json: Vec<serde_json::Value> = commands.iter().map(|cmd| {
        let mut obj = serde_json::json!({
            "name": cmd.name,
            "description": cmd.description,
        });
        if !cmd.args.is_empty() {
            obj["args"] = serde_json::to_value(&cmd.args).unwrap();
        }
        obj
    }).collect();
    
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": self.session_id,
            "update": {
                "sessionUpdate": "available_commands_update",
                "commands": commands_json,
            }
        }
    });
    
    let _ = self.tx.send(notification).await;
}
```

#### 3.3.3 Prompt 中使用 Registry 分派

```rust
// loom-acp/src/agent.rs — prompt() 中替换现有命令分发 (647-713)

if let loom::message::UserContent::Text(ref text) = user_content {
    let reg = loom::command::registry::global_registry().read().unwrap();
    if let Some((cmd, args)) = reg.parse(text) {
        drop(reg);
        let mut cmd_ctx = CommandContext {
            parsed_args: args,
            platform: &mut AcpPlatformContext {
                sessions: &self.sessions,
                session_key: &key,
            },
        };
        
        match cmd.execute(&mut cmd_ctx)? {
            CommandOutcome::Handled(msg) => {
                if let Some(ref tx) = self.session_update_tx {
                    let notifier = SessionNotifier::new(tx.clone(), args_inner.session_id.clone());
                    notifier.send_text_reply(&msg).await;
                }
                return Ok(PromptResponse::new(StopReason::EndTurn));
            }
            CommandOutcome::Error(msg) => {
                if let Some(ref tx) = self.session_update_tx {
                    let notifier = SessionNotifier::new(tx.clone(), args_inner.session_id.clone());
                    notifier.send_error_reply(&msg).await;
                }
                return Ok(PromptResponse::new(StopReason::EndTurn));
            }
            CommandOutcome::HandledSilent => {
                return Ok(PromptResponse::new(StopReason::EndTurn));
            }
            CommandOutcome::PassThrough => {
                // 继续正常 prompt 流程
            }
            CommandOutcome::AsyncRequired => {
                // 1. 解析模型
                let model = self.resolve_model_with_tier_awareness(&entry.session_config).await;
                // 2. 创建 LLM client
                let llm = self.llm_provider.create_client(&model).await?;
                // 3. 获取当前 session messages
                let messages = entry.messages.clone();
                // 4. 重建 CommandContext（带 messages 引用）
                let mut async_ctx = CommandContext {
                    parsed_args: cmd_ctx.parsed_args,
                    platform: &mut AcpPlatformContext {
                        sessions: &self.sessions,
                        session_key: &key,
                    },
                };
                // 5. 执行异步命令（compact / summarize）
                let outcome = cmd.execute_async(&mut async_ctx, llm.as_ref()).await?;
                // 6. 发送结果
                if let CommandOutcome::Handled(msg) = &outcome {
                    if let Some(ref tx) = self.session_update_tx {
                        let notifier = SessionNotifier::new(tx.clone(), args_inner.session_id.clone());
                        notifier.send_text_reply(msg).await;
                    }
                }
                return Ok(PromptResponse::new(StopReason::EndTurn));
            }
        }
    }
}
```

#### 3.3.4 ACP PlatformContext 实现

```rust
// loom-acp/src/agent.rs — 新增

struct AcpPlatformContext<'a> {
    sessions: &'a SessionStore,
    session_key: &'a OurSessionId,
}

impl PlatformContext for AcpPlatformContext<'_> {
    fn reset_context(&mut self) -> Result<String, String> {
        self.sessions.cancel_current_generation(self.session_key);
        Ok("Context cleared.".into())
    }

    fn platform_name(&self) -> &str {
        "acp"
    }
}
```

### 3.4 Telegram 接入层改造

#### 3.4.1 统一分派入口

```rust
// telegram-bot/src/pipeline/mod.rs — 替换现有的三层分派

pub async fn handle_common_message(ctx: &MessageContext<'_>) -> Result<(), BotError> {
    if let Some(text) = ctx.msg.text() {
        // 1. 尝试 core registry 命令
        let reg = loom::command::registry::global_registry().read().unwrap();
        if let Some((cmd, args)) = reg.parse(text) {
            drop(reg);
            let mut cmd_ctx = CommandContext {
                parsed_args: args,
                platform: &mut TelegramPlatformContext {
                    chat_id: ctx.chat_id(),
                    deps: ctx.deps,
                },
            };
            
            match cmd.execute(&mut cmd_ctx) {
                Ok(CommandOutcome::Handled(msg)) => {
                    ctx.deps.sender.send_text(ctx.chat_id(), &msg).await?;
                    return Ok(());
                }
                Ok(CommandOutcome::Error(msg)) => {
                    ctx.deps.sender.send_text(ctx.chat_id(), &msg).await?;
                    return Ok(());
                }
                Ok(CommandOutcome::HandledSilent) => return Ok(()),
                Ok(CommandOutcome::AsyncRequired) => {
                    // Telegram async 命令走现有 streaming 流程
                    // 1. 解析模型
                    let model = resolve_model(ctx.deps).await;
                    // 2. 创建 LLM client
                    let llm = create_llm_client(&model).await?;
                    // 3. 执行异步命令
                    let outcome = cmd.execute_async(&mut cmd_ctx, llm.as_ref()).await?;
                    // 4. 流式发送结果
                    if let CommandOutcome::Handled(msg) = &outcome {
                        ctx.deps.sender.send_text(ctx.chat_id(), msg).await?;
                    }
                    return Ok(());
                }
                Ok(CommandOutcome::PassThrough) | Err(_) => {}
            }
        } else {
            drop(reg);
        }

        // 2. Bot 特有命令（/status，不在 core registry 中）
        let cmd_ctx = CommandContext { /* ... */ };
        let dispatcher = CommandDispatcher::default();
        if let Some(result) = dispatcher.try_dispatch(&cmd_ctx, text).await {
            return result;
        }

        // 3. /model 子命令（保持现有逻辑）
        if try_handle_model_command_input(&cmd_ctx, text).await? {
            return Ok(());
        }

        // 4. 正常消息处理...
    }
    Ok(())
}
```

### 3.5 文件变更清单

| 操作 | 文件 | 说明 |
|---|---|---|
| **新增** | `loom/src/command/registry.rs` | CommandRegistry + SlashCommand trait + RwLock 全局注册表 |
| **新增** | `loom/src/command/builtins_v2.rs` | 基于 trait 的 5 个内置命令实现 |
| **修改** | `loom/src/command/mod.rs` | 导出 registry 模块和 GLOBAL_REGISTRY |
| **修改** | `loom-acp/src/agent.rs` | prompt() 使用 registry 分派；new_session() 发送 commands |
| **修改** | `loom-acp/src/stream_bridge.rs` | SessionNotifier 新增 send_available_commands |
| **修改** | `telegram-bot/src/pipeline/mod.rs` | 使用 registry 替代手写 match |
| **修改** | `telegram-bot/src/command/mod.rs` | 移除与 core 重复的 ResetCommand |
| **修改** | `loom-acp/tests/capabilities_structure.rs` | 添加 available_commands_update 测试 |
| **保留** | `loom/src/command/parser.rs` | Phase 1-2 保留，Phase 3 标记 deprecated |
| **保留** | `loom/src/command/builtins.rs` | Phase 1-2 保留，Phase 3 标记 deprecated |

---

## 4. 迁移策略

分三阶段渐进式迁移，每阶段可独立发布。

### Phase 1: 基础设施（1-2 天）

1. 新增 `registry.rs`、`builtins_v2.rs`
2. 实现 5 个内置命令的 trait 版本（Reset、Compact、Summarize、Models、Goal）
3. 初始化 `GLOBAL_REGISTRY`（LazyLock）
4. **不改任何现有调用方**，只做 registry 的单元测试

验收标准：
```bash
cargo test -p loom -- command::registry
```

### Phase 2: ACP 接入（1-2 天）

1. `agent.rs::prompt()` 改用 `GLOBAL_REGISTRY.parse()` 分派
2. 修复 `/compact`、`/summarize` 静默吞掉的 bug
3. `new_session()` 后发送 `available_commands_update`
4. `SessionNotifier` 新增 `send_available_commands`
5. 更新 `capabilities_structure.rs` 测试

验收标准：
```bash
cargo test -p loom-acp -- capabilities
cargo test -p loom-acp -- e2e
```
IDE 能看到命令列表。

### Phase 3: Telegram 接入（1 天）

1. `pipeline/mod.rs` 统一使用 `GLOBAL_REGISTRY.parse()` 作为第一层分派
2. 移除 `CommandDispatcher` 中重复的 `ResetCommand`
3. `/model` 子命令保持独立（Telegram 特有，不纳入 core registry）
4. 旧 `parser.rs` / `builtins.rs` 标记 `#[deprecated]`

验收标准：
```bash
cargo test -p telegram-bot
```
Telegram `/reset`、`/compact`、`/summarize`、`/model` 行为不变。

---

## 5. ACP 协议合规细节

### 5.1 available_commands_update 格式

根据 [ACP Slash Commands 规范](https://agentclientprotocol.com/protocol/slash-commands)：

```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_abc123",
    "update": {
      "sessionUpdate": "available_commands_update",
      "commands": [
        {
          "name": "/reset",
          "description": "Clear conversation context (aliases: /clear, /new)"
        },
        {
          "name": "/compact",
          "description": "Compact conversation history",
          "args": [
            {
              "name": "query",
              "description": "Focus instructions for compaction",
              "required": false
            }
          ]
        },
        {
          "name": "/summarize",
          "description": "Summarize the current conversation"
        },
        {
          "name": "/models",
          "description": "List or search available models",
          "args": [
            {
              "name": "query",
              "description": "Search query",
              "required": false
            }
          ]
        },
        {
          "name": "/goal",
          "description": "Run autonomous goal loop",
          "args": [
            {
              "name": "description",
              "description": "Goal description",
              "required": true
            }
          ]
        }
      ]
    }
  }
}
```

### 5.2 命令执行流程

命令作为普通 prompt 发送（无需协议变更）：

```json
{
  "method": "session/prompt",
  "params": {
    "sessionId": "sess_abc123",
    "prompt": [
      { "type": "text", "text": "/compact focus on auth module" }
    ]
  }
}
```

Agent 在 `prompt()` 中通过 registry 识别命令前缀并处理。

### 5.3 动态命令更新

规范允许在 session 生命周期内随时发送新的 `available_commands_update`：

**触发调用点：**
1. `new_session()` — 初始注册后发送
2. `set_session_mode()` — mode 切换后过滤命令并重发（部分命令按 mode 可用/不可用）
3. `register_plugin_command()` — 插件注册新命令后对所有活跃 session 重发

每个触发点调用同一 `send_available_commands()` 方法，保证幂等。实现方式：

```rust
// 在 mode 切换后重发
pub async fn set_session_mode(&self, session_id: &str, mode: &str) -> Result<()> {
    // ... 现有 mode 切换逻辑 ...
    
    // 过滤当前 mode 可用的命令并重发
    if let Some(ref tx) = self.session_update_tx {
        let notifier = SessionNotifier::new(tx.clone(), session_id.into());
        let reg = loom::command::registry::global_registry().read().unwrap();
        let commands = reg.command_infos();
        drop(reg);
        let filtered = filter_commands_by_mode(&commands, mode);
        notifier.send_available_commands(&filtered).await;
    }
    Ok(())
}
```

---

## 6. 设计决策与 Trade-offs

### 6.1 为什么用 trait 而不是 enum + 函数指针？

| 方案 | 优点 | 缺点 |
|---|---|---|
| `enum Command` + 独立函数 | 零开销、编译时穷尽检查 | 新增命令必须改 enum |
| `trait SlashCommand` + `dyn` | 开闭原则、可动态注册 | 轻微虚表开销、无法穷尽 match |
| 宏声明式 | 简洁 | 调试困难、IDE 支持差 |

选择 **trait 方案**：命令数量有限（< 20），虚表开销可忽略；动态注册和开闭原则的收益更大。`GLOBAL_REGISTRY` 已使用 `RwLock` 包装，为插件注册预留了扩展能力。

### 6.2 为什么 `PlatformContext` 用 trait 而不是 enum？

不同接入层的上下文差异很大（ACP 有 session_id，Telegram 有 chat_id），trait 允许各层自定义上下文而不影响 core 层。

### 6.3 为什么保留旧 parser/builtins？

渐进式迁移。Phase 1-2 中旧代码仍作为 fallback 存在，Phase 3 完成后标记 deprecated。避免一次性大改动引入回归。

### 6.4 GLOBAL_REGISTRY 为什么用 LazyLock + RwLock？

- 命令集在编译时确定，`LazyLock` 保证线程安全的一次性初始化
- `RwLock` 包装为插件注册预留扩展能力（§9.1），读多写少场景性能开销极小
- `global_registry()` 函数封装访问，避免直接暴露内部类型

---

## 7. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 迁移中遗漏某个命令的 match 分支 | 中 | 用户可见 bug | Phase 1 先做新旧命令集对比测试 |
| `dyn SlashCommand` 的 parse 逻辑不一致 | 低 | 命令匹配失败 | parse 逻辑从 `parser.rs` 直接迁移，保持现有测试 |
| ACP `available_commands_update` 格式不符合客户端预期 | 中 | IDE 不显示命令 | 参考 ACP 规范示例，添加 e2e 测试 |
| Telegram async 命令（compact/summarize）迁移复杂 | 中 | 功能回退 | Phase 3 中 async 命令保持现有 streaming 路径 |

---

## 8. 测试计划

### 8.1 单元测试

```rust
// loom/src/command/registry.rs tests

#[test]
fn registry_parses_all_builtin_commands() {
    let reg = global_registry().read().unwrap();
    
    assert!(reg.parse("/reset").is_some());
    assert!(reg.parse("/clear").is_some());
    assert!(reg.parse("/new").is_some());
    assert!(reg.parse("/compact").is_some());
    assert!(reg.parse("/compact focus on auth").is_some());
    assert!(reg.parse("/summarize").is_some());
    assert!(reg.parse("/models").is_some());
    assert!(reg.parse("/models gpt").is_some());
    assert!(reg.parse("/models use gpt-4o").is_some());
    assert!(reg.parse("/goal fix the bug").is_some());
    
    assert!(reg.parse("hello world").is_none());
    assert!(reg.parse("/unknown").is_none());
}

#[test]
fn registry_boundary_cases() {
    let reg = global_registry().read().unwrap();
    
    // 大小写
    assert!(reg.parse("/Reset").is_some());
    assert!(reg.parse("/COMPACT").is_some());
    assert!(reg.parse("/ReSeT").is_some());
    
    // 前后空格
    assert!(reg.parse(" /reset").is_some());
    assert!(reg.parse("/reset ").is_some());
    assert!(reg.parse("  /reset  ").is_some());
    
    // 非命令前缀
    assert!(reg.parse("reset").is_none());
    assert!(reg.parse("not a /reset").is_none());
    
    // 空输入
    assert!(reg.parse("").is_none());
    assert!(reg.parse("   ").is_none());
}

#[test]
fn registry_aliases_resolve_to_same_command() {
    let reg = global_registry().read().unwrap();
    let reset_main = reg.parse("/reset").unwrap().0.info().name;
    let reset_alias = reg.parse("/clear").unwrap().0.info().name;
    assert_eq!(reset_main, reset_alias);
}

#[test]
fn registry_command_infos_covers_all_commands() {
    let reg = global_registry().read().unwrap();
    let infos = reg.command_infos();
    assert!(infos.len() >= 5);
    let names: Vec<&str> = infos.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"/reset"));
    assert!(names.contains(&"/compact"));
    assert!(names.contains(&"/summarize"));
    assert!(names.contains(&"/models"));
    assert!(names.contains(&"/goal"));
}
```

### 8.2 ACP 集成测试

```rust
// loom-acp/tests/command_dispatch.rs

#[tokio::test]
async fn acp_compact_command_not_silent() {
    let mut acp = AcpChild::spawn(None).expect("spawn");
    let session = create_session(&mut acp).await;
    
    let result = acp.send_request_and_wait(
        "session/prompt",
        json!({ "sessionId": session, "prompt": [
            { "type": "text", "text": "/compact" }
        ]}),
        Duration::from_secs(30),
    ).await;
    
    // 不应该静默返回
    assert!(result.is_ok());
}

#[tokio::test]
async fn acp_new_session_sends_available_commands() {
    let mut acp = AcpChild::spawn(None).expect("spawn");
    let session = create_session(&mut acp).await;
    
    // 应收到 available_commands_update 通知
    let notification = acp.wait_for_notification("session/update", Duration::from_secs(5)).await;
    let update = notification["params"]["update"]["sessionUpdate"].as_str();
    assert_eq!(update, Some("available_commands_update"));
    let commands = notification["params"]["update"]["commands"].as_array().unwrap();
    assert!(commands.len() >= 5);
}
```

### 8.3 回归测试

保持现有 `parser.rs` 和 `builtins.rs` 的测试不变，迁移完成后对比新旧解析结果一致。

---

## 9. 未来扩展

### 9.1 插件命令注册

```rust
loom::command::registry::register_plugin_command(MyPluginCommand);
```

需要在初始化阶段提供可修改的注册 API。`GLOBAL_REGISTRY` 可改为 `OnceLock<RwLock<CommandRegistry>>` 或提供 `register()` 函数。

### 9.2 命令权限控制（mode 过滤）

```rust
fn info(&self) -> CommandInfo {
    CommandInfo {
        name: "/goal".into(),
        available_in_modes: Some(vec!["dev", "agent-builder"]),
        ..Default::default()
    }
}
```

`available_commands_update` 在 mode 切换后过滤发送。

### 9.3 命令参数补全

```rust
pub struct CommandArgSchema {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub hint: Option<String>,  // "file_path" | "model_name" | "mode_name"
}
```

---

## 10. 参考资料

- [ACP Slash Commands 规范](https://agentclientprotocol.com/protocol/slash-commands)
- [ACP Initialization 规范](https://agentclientprotocol.com/protocol/initialization)
- [ACP Schema](https://agentclientprotocol.com/protocol/schema)
- `loom-acp/src/protocol.rs` — Loom ACP 协议文档
- `loom-acp/src/agent.rs:647-713` — 当前 ACP 命令分派
- `telegram-bot/src/pipeline/mod.rs:119-266` — 当前 Telegram 命令分派
- `loom/src/command/parser.rs` — 当前 core 命令解析器
- `loom/src/command/builtins.rs` — 当前 core 命令执行器
- `loom/src/command/command.rs` — 当前 Command enum 定义
- `telegram-bot/src/command/mod.rs` — 当前 Telegram CommandDispatcher
