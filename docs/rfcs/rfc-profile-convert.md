# RFC: profile-convert — Loom Agent Profile 导出为第三方工具配置

- **作者**: loom contributors
- **日期**: 2025-08-19
- **状态**: Draft

## 1. 摘要

在 `loom` crate 内新增 `profile_convert` 模块，将 `AgentProfile` 转换为 Claude Code (`.claude/agents/{name}.md`)、OpenAI Codex (`config.toml`) 和 Cursor (`.cursor/rules/*.mdc`) 的配置格式。CLI 新增 `export` 子命令调用此模块。当前阶段作为 `loom` 内部模块实现，未来如需独立复用再抽取为 crate。

## 2. 动机

用户在 Loom 中精心配置了 agent（角色、模型、工具约束、行为策略等），切换到 Claude Code / Codex / Cursor 时需要手动重写配置。提供一个一键导出工具，降低迁移成本，也便于 Loom 作为"配置中心"统一管理 agent 定义。

## 3. 目标格式分析

### 3.1 Claude Code — `.claude/agents/{name}.md`

Markdown 文件 + YAML frontmatter，定义 Claude Code 的自定义 sub-agent。放在 `.claude/agents/`（项目级）或 `~/.claude/agents/`（用户级）。

```markdown
---
name: code-reviewer
description: Reviews code for quality and best practices
tools: Read, Glob, Grep
model: sonnet
---

You are a code reviewer. When invoked, analyze the code and provide
specific, actionable feedback on quality, security, and best practices.
```

**支持的语义**（与 Loom `AgentProfile` 高度对齐）:
- `name` — sub-agent 名称
- `description` — 描述（Claude 据此决定何时委派）
- `model` — 模型选择（`sonnet` / `opus` / `haiku` 或完整 model ID）
- `tools` / `disallowedTools` — 工具白名单/黑名单
- `permissionMode` — 权限模式（`default` / `plan` / `auto` / `bypassPermissions`）
- `maxTurns` — 最大迭代轮次
- `mcpServers` — MCP 服务器（内联定义或引用）
- `hooks` — 生命周期钩子
- 正文 — system prompt

**不支持的语义**: `temperature` / `max_tokens`、继承（`extends`）

> **注**: `CLAUDE.md` 是项目级全局指令文件（coding conventions），不是 agent 定义。
> Loom `AgentProfile` 的语义对应物是 `.claude/agents/{name}.md` sub-agent 定义。

### 3.2 OpenAI Codex — `.codex/config.toml`

TOML 配置文件，位于 `.codex/config.toml`（XDG 兼容路径）。

```toml
model = "o3"
model_provider = "openai"
instructions = """
You are a helpful coding assistant.
"""
```

**支持的语义**:
- `instructions` — system prompt
- `model` / `model_provider` — 模型选择

**不支持的语义**: 工具白名单/黑名单、MCP servers、temperature/max_tokens、approval policy

### 3.3 Cursor — `.cursor/rules/*.mdc`

Markdown 文件 + YAML frontmatter，位于 `.cursor/rules/` 目录。

```markdown
---
description: Architect agent configuration
globs:
alwaysApply: true
---

You are a technical architect...
```

**支持的语义**:
- `description` — 规则描述
- `globs` — 文件匹配模式（可选）
- `alwaysApply` — 是否始终应用
- 正文 — 规则内容（system prompt）

**不支持的语义**: model、tools、approval policy

## 4. 详细设计

### 4.1 模块位置

作为 `loom` crate 内部模块实现，不创建独立 crate。原因：

- `AgentProfile` 及其子类型（`RoleConfig`, `ModelConfig`, `ToolsConfig` 等）定义在 `loom::cli_run::profile`，无独立 types crate
- 作为同 crate 模块，可直接访问 `profile` 模块的类型和函数，无需 re-export 桥接
- 避免引入新的 workspace member 和跨 crate 依赖
- 未来如有外部复用需求，再抽取为独立 crate

```
cli ──► loom (profile_convert 模块)
```

> **Import 路径**: 模块内可直接使用 `crate::cli_run::profile::*` 访问所有类型。
> 对外通过 `loom/src/lib.rs` re-export `ExportFormat`、`export`、`export_all` 等公共 API。

### 4.2 模块结构

```
loom/src/profile_convert/
├── mod.rs            # mod 声明 + pub use + ExportFormat 枚举 + export/export_all 函数
├── error.rs          # ConvertError
├── claude_code.rs    # AgentProfile → CLAUDE.md
├── codex.rs          # AgentProfile → config.toml
└── cursor.rs         # AgentProfile → *.mdc
```

> **说明**: 不需要 `loader` 模块。`crate::cli_run::profile::resolve_profile(name)` 已是公共 API
> (`loom/src/cli_run/profile.rs:433`)，直接调用即可。该函数已处理内置 agent、项目级、用户级
> profile 的查找，以及 `role.file` → `role.content` 的文件内容解析、`extends` 继承合并等。

### 4.3 核心 API

```rust
// loom/src/profile_convert/mod.rs

use crate::cli_run::profile::AgentProfile;
use std::path::PathBuf;

use crate::cli_run::profile::resolve_profile;

/// 目标导出格式（不依赖 clap，CLI 层自行桥接）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    ClaudeCode,
    Codex,
    Cursor,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 3] = [
        ExportFormat::ClaudeCode,
        ExportFormat::Codex,
        ExportFormat::Cursor,
    ];
}

impl std::str::FromStr for ExportFormat {
    type Err = ConvertError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "cursor" => Ok(Self::Cursor),
            _ => Err(ConvertError::UnknownFormat(s.to_string())),
        }
    }
}

/// 导出结果
pub struct ExportOutput {
    /// 目标相对路径（如 ".claude/agents/architect.md"、".codex/config.toml"、".cursor/rules/architect.mdc"）
    pub path: PathBuf,
    /// 文件内容
    pub content: String,
}

/// 将指定 agent 导出为目标格式
pub fn export(name: &str, format: ExportFormat) -> Result<ExportOutput, ConvertError>;

/// 将指定 agent 导出为所有支持的格式
pub fn export_all(name: &str) -> Result<Vec<ExportOutput>, ConvertError>;
```

### 4.4 字段映射表

| Loom `AgentProfile` | Claude Code (`.claude/agents/`) | Codex | Cursor |
|---|---|---|---|
| `name` | frontmatter `name` + 文件名 | `# comments` | frontmatter `description` + 文件名 |
| `description` | frontmatter `description` | `# comments` | frontmatter `description` 附加 |
| `role.content` | 正文（system prompt） | `instructions` | .mdc 正文主体 |
| `model.name` | frontmatter `model` | `model` | HTML 注释保留 |
| `model.tier` | frontmatter `model`（映射为 sonnet/opus/haiku） | `model`（映射） | HTML 注释保留 |
| `model.provider` | 不支持（Claude 专有） | `model_provider` | HTML 注释保留 |
| `behavior.approval_policy` | frontmatter `permissionMode`（映射） | 正文约束说明 | 正文约束说明 |
| `behavior.max_iterations` | frontmatter `maxTurns` | `# comments` | 正文约束说明 |
| `tools.builtin.disabled` | frontmatter `disallowedTools` | `# comments` | 正文约束说明 |
| `tools.builtin.enabled` | frontmatter `tools`（白名单） | `# comments` | 正文约束说明 |
| `tools.mcp.servers` | frontmatter `mcpServers` | 丢弃（不兼容） | 丢弃（不兼容） |
| `skills` | 正文附加说明 | `instructions` 附加 | 正文附加说明 |
| `environment` | 丢弃 | 丢弃 | 丢弃 |

### 4.5 前置条件与 profile 加载

转换器**必须**通过 `crate::cli_run::profile::resolve_profile(name)` 加载 profile，不可直接构造 `AgentProfile`。原因：

- `RoleConfig.file` 字段指定外部文件路径，`resolve_profile` 内部会将文件内容读入 `role.content`（`profile.rs:342-348`）
- `extends` 继承链需要递归解析并合并（`profile.rs:351-356`）
- 内置 agent（dev/ask/explore/orchestrator/agent-builder）通过 `include_str!` 嵌入，`resolve_profile` 会优先匹配
- `source_dir` 在加载时设置，供 skills 解析使用

### 4.6 无映射字段处理策略

对于目标格式不支持的字段（如 model 对 Cursor、provider 对 Claude Code），统一采用 **HTML 注释保留** 策略：

- **保留在注释中**：以 `<!-- loom: ... -->` 格式嵌入，方便人类参考和未来回导
- **语义约束转文字**：如 `tools.builtin.disabled: [bash]` 转为 "Do NOT use bash/shell commands"
- **不丢弃关键语义**：用户能从导出文件中看到原始 Loom 配置的完整意图

### 4.7 各格式转换器设计

#### 4.7.1 `claude_code.rs` → `.claude/agents/{name}.md`

```rust
// loom/src/profile_convert/claude_code.rs
use crate::cli_run::profile::AgentProfile;

pub fn convert(profile: &AgentProfile) -> ExportOutput;
```

输出结构：

```markdown
---
name: {name}
description: {description or name}
model: {resolved_model: sonnet | opus | haiku}
tools: {whitelisted tools}
disallowedTools: {blacklisted tools}
maxTurns: {max_iterations}
permissionMode: {mapped approval_policy}
---

{role.content}

## Constraints

<!-- 从 tools.builtin.disabled 和 behavior 转化的补充约束 -->
- {constraint_1}
- {constraint_2}
```

**`model.tier` 映射**（当 `model.name` 为空时）:

| Loom `ModelTier` | Claude Code `model` 值 |
|---|---|
| `None` | 不设置 `model` 字段 |
| `Light` | `haiku` |
| `Standard` | `sonnet` |
| `Strong` | `opus` |

**`approval_policy` → `permissionMode` 映射**:

| Loom `approval_policy` | Claude Code `permissionMode` |
|---|---|
| `auto-approve` | `auto` |
| `suggest` | `default` |
| `strict` | `plan` |
| 其他/未设置 | 不设置（使用 Claude Code 默认） |

**`tools.builtin` 映射**:

| Loom `tools.builtin.enabled` | Claude Code `tools` 字段 |
|---|---|
| 非空列表 | 映射为 Claude Code 工具名（如 `Read`, `Glob`, `Grep`, `Bash`, `Edit`, `Write`） |
| 空/None | 不设置（继承所有默认工具） |

| Loom `tools.builtin.disabled` | Claude Code `disallowedTools` 字段 |
|---|---|
| 非空列表 | 映射为对应工具名 |
| 空/None | 不设置 |

> **注**: Claude Code sub-agent 的 `tools` 和 `disallowedTools` 字段控制工具访问权限。
> 如果两者都设置了，先应用 `disallowedTools` 移除，再从结果中取 `tools` 白名单交集。

#### 4.7.2 `codex.rs` → `.codex/config.toml`

```rust
// loom/src/profile_convert/codex.rs
use crate::cli_run::profile::AgentProfile;

pub fn convert(profile: &AgentProfile) -> ExportOutput;
```

输出结构：

```toml
# Exported from Loom agent: {name}
# Description: {description}

model = "{resolved_model_name}"
model_provider = "{provider}"

# loom-behavior: max_iterations={max_iterations}

instructions = """
{role.content}

# Constraints
- {constraint_1}
- {constraint_2}
"""
```

**`model.tier` 映射**（当 `model.name` 为空时）:

| Loom `ModelTier` | Codex `model` 值 |
|---|---|
| `None` | 不设置 `model` 字段 |
| `Light` | `"o3-mini"` |
| `Standard` | `"o3"` |
| `Strong` | `"o3"` |

**`model_provider` 映射**（`model.provider` → Codex `model_provider`）:

| Loom `model.provider` | Codex `model_provider` | 备注 |
|---|---|---|
| `"openai"` | `"openai"` | 直接映射 |
| 其他非空值 | 原值透传 | 如 `"anthropic"` 等，由 Codex 自行处理 |
| `None` | 不设置 | 使用 Codex 默认 provider |

> Loom 的 `provider_type`（`"openai"` / `"bigmodel"`）与 Codex 的 provider 体系不完全对齐。
> 映射策略为：优先使用 `model.provider`，忽略 `model.provider_type`。

#### 4.7.3 `cursor.rs` → `.cursor/rules/{name}.mdc`

```rust
// loom/src/profile_convert/cursor.rs
use crate::cli_run::profile::AgentProfile;

pub fn convert(profile: &AgentProfile) -> ExportOutput;
```

输出结构：

```markdown
---
description: "{description or name} — exported from Loom"
alwaysApply: true
---

<!-- loom-model: {model} -->
<!-- loom-tools-disabled: {list} -->

{role.content}

## Constraints

- {constraint_1}
- {constraint_2}
```

> **说明**: `globs` 字段省略（Loom profile 无文件匹配语义），仅保留 `alwaysApply: true`。

### 4.8 CLI 集成

在 `cli/src/args.rs` 的 `Command` 枚举中新增 `Export` 子命令：

```rust
/// Export Loom agent profile to third-party tool formats
Export(ExportArgs),
```

```rust
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ExportArgs {
    /// Agent profile name to export (required unless --all)
    #[arg(value_name = "AGENT")]
    pub(crate) agent: Option<String>,

    /// Export all available agents
    #[arg(long)]
    pub(crate) all: bool,

    /// Output format: claude-code, codex, cursor, all
    #[arg(short, long, value_name = "FORMAT", default_value = "all")]
    pub(crate) format: String,

    /// Output directory (default: current directory)
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub(crate) output: PathBuf,

    /// Dry run: print to stdout instead of writing files
    #[arg(long)]
    pub(crate) dry_run: bool,
}
```

用法示例：

```bash
# 导出 architect agent 为所有格式
loom export architect

# 仅导出 Claude Code 格式
loom export architect --format claude-code

# 导出到指定目录
loom export architect --format codex --output ~/my-project

# 预览（不写文件，输出到 stdout）
loom export architect --dry-run

# 导出所有 agent
loom export --all --format claude-code
```

### 4.9 `loom/src/lib.rs` 注册

```rust
// loom/src/lib.rs 新增
pub mod profile_convert;
pub use profile_convert::{export, export_all, ConvertError, ExportFormat, ExportOutput};
```

无需修改 workspace `Cargo.toml` 或 `cli/Cargo.toml`，`cli` 已依赖 `loom`。

## 5. 替代方案

### 方案 B: 独立 `profile-convert` crate

将转换逻辑放在 `crates/profile-convert/` 独立 crate 中，通过 `loom` 依赖获取 `AgentProfile` 类型。

**优点**: 解耦，可独立发布和复用；`cli` 不需要引入 `loom` 的额外功能
**缺点**: 需要在 `loom` 中 re-export 更多子类型，或创建单独的 `profile-types` crate；增加 workspace 复杂度

**结论**: 当前阶段不需要。作为 `loom` 内部模块实现，未来如有外部复用需求再抽取。

### 方案 C: CLI 内嵌实现（不新建模块）

直接在 `cli` crate 中实现转换逻辑。

**优点**: 最简单，零新模块
**缺点**: 违反 SRP（CLI 负责参数解析+调度，不应包含业务逻辑）；无法独立测试和复用；`serve` 等其他 crate 也可能需要导出功能

**结论**: 不采纳。放在 `loom` 中可同时服务于 `cli` 和 `serve`。

## 6. 测试计划

### 6.1 单元测试

每个转换器（`claude_code.rs`, `codex.rs`, `cursor.rs`）独立测试：

- **最小 profile 测试**: 仅 `name`，验证输出不为空
- **完整 profile 测试**: 所有字段填充，验证映射完整
- **边界测试**: `role.content` 为空、`model` 未设置、`tools` 未设置
- **特殊字符测试**: 内容含 TOML 特殊字符、Markdown 语法、YAML frontmatter 语法

### 6.2 集成测试

- 使用 `resolve_profile("dev")` 加载内置 dev agent，导出为所有格式，验证输出结构合法
- 使用 `.loom/agents/architect` 项目级 agent，验证导出

### 6.3 快照测试

为内置 agents（dev, explore, ask）的导出结果维护 snapshot，防止意外变更。

## 7. 实施计划

### Phase 1: 骨架 + Claude Code

1. 创建 `loom/src/profile_convert/` 目录结构
2. 实现 `mod.rs`（`ExportFormat` 枚举、`export`/`export_all` 函数骨架）、`error.rs`
3. 实现 `claude_code.rs`
4. 在 `loom/src/lib.rs` 注册模块并 re-export
5. 单元测试

### Phase 2: Codex + Cursor

1. 实现 `codex.rs`
2. 实现 `cursor.rs`
3. 单元测试

### Phase 3: CLI 集成

1. 修改 `cli/src/args.rs` 添加 `Export` 子命令
2. 实现 `cli/src/subcommands/export.rs`（调度 `loom::profile_convert`）
3. 在 `cli/src/main.rs` 中路由 `Export` 命令
4. 集成测试

### Phase 4: 打磨

1. 快照测试
2. 错误处理优化（agent 不存在、写入权限等）
3. 文档更新

## 8. 开放问题

1. **批量导出 `--all`**: 是否支持一次导出所有 agent？建议 Phase 3 实现
2. **输出路径策略**: 当前设计为 `--output` 指定根目录，文件写入相对路径。是否需要支持自定义文件名？
3. **回导（Round-trip）**: 未来是否支持从 Claude Code / Codex / Cursor 配置反向导入为 Loom profile？建议作为独立 RFC
