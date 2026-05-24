# RFC: EnvContext — 运行时环境上下文注入

> 状态: Draft
> 模块: `loom/src/helve/env_context.rs`
> 影响: `prompt.rs`, `config.rs`, `cli_run/mod.rs`, `openai_sse/parse.rs`, `lib.rs`, `mod.rs`

## 摘要

将 system prompt 中的环境上下文从 `Option<String>` 提升为结构化的 `EnvContext` 类型。

**现状问题**：`build_env_context()` 硬编码 3 行文本（OS、Locale、Agent），信息维度有限、不可扩展、不可覆盖、不可测试。

**方案**：新增 `env_context.rs` 模块，定义 `EnvContext` 及 5 个子结构体（`OsInfo`/`LocaleInfo`/`ShellInfo`/`ProjectInfo`/`RuntimeInfo`），通过 `detect()` 采集 + `to_prompt_section()` 渲染，builder 模式支持配置覆盖。涉及 7 个文件变更，3 个测试重写。

**预期效果**：环境上下文输出从 3 行扩展到最多 8 行（OS/arch、Locale、Reply language、Shell、Project languages、Git、Agent、Container），每项仅在检测到时输出，agent 可据此适配 shell 命令、回复语言、路径格式等行为。

---

## 一、现状

### 1.1 数据流

环境上下文的注入贯穿以下调用链：

```
cli_run/mod.rs:231          ← 构造 env_context 字符串
    ↓
HelveConfig.env_context     ← Option<String> 透传
    ↓
config.rs:73                ← to_react_build_config() 赋值给 ReactPromptInputs
    ↓
prompt.rs:98-110            ← collect_prefix_sections() 收集为 &str
    ↓
prompt.rs:116-140           ← assemble_react_system_prompt() 拼接到最前部
    ↓
最终 system prompt
```

### 1.2 当前代码

#### `prompt.rs:165-178` — 环境上下文生成

```rust
pub fn build_env_context() -> String {
    let os = std::env::consts::OS;
    let lang = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANGUAGE"))
        .unwrap_or_else(|_| "en_US.UTF-8".to_string());
    format!(
        "ENVIRONMENT:\n\
         - OS: {os}\n\
         - Locale: {lang}\n\
         - Agent: Loom (a Rust-native AI agent framework with ReAct/ToT/GoT/DUP reasoning \
         patterns, tool use, streaming, and session management)"
    )
}
```

输出示例：

```
ENVIRONMENT:
- OS: macos
- Locale: en_US.UTF-8
- Agent: Loom (a Rust-native AI agent framework with ReAct/ToT/GoT/DUP reasoning patterns, tool use, streaming, and session management)
```

#### `prompt.rs:34-53` — ReactPromptInputs 存储

```rust
#[derive(Debug, Clone, Default)]
pub struct ReactPromptInputs {
    pub full_override: Option<String>,
    pub base_prompt_override: Option<String>,
    pub role_setting: Option<String>,
    pub agents_md: Option<String>,
    pub skills_prompt: Option<String>,
    pub env_context: Option<String>,     // ← 裸字符串，无类型约束
    pub working_folder: Option<PathBuf>,
    pub approval_policy: Option<ApprovalPolicy>,
}
```

#### `prompt.rs:98-110` — 拼接逻辑

```rust
fn collect_prefix_sections(inputs: &ReactPromptInputs) -> Vec<&str> {
    [
        inputs.env_context.as_deref(),      // ← 依赖 String 的 as_deref()
        inputs.role_setting.as_deref(),
        inputs.agents_md.as_deref(),
        inputs.skills_prompt.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .collect()
}
```

#### `prompt.rs:116-140` — 最终组装

```rust
pub fn assemble_react_system_prompt(inputs: &ReactPromptInputs) -> String {
    // ... full_override bypass ...
    let base_content = format!(
        "{}{}{}",
        base_prompt,
        build_workdir_section(inputs.working_folder.as_deref()),
        build_approval_section(inputs.approval_policy)
    );
    let prefix_sections = collect_prefix_sections(inputs);
    // prefix_sections 拼接到 base_content 前面
    // 顺序: env_context → role_setting → agents_md → skills_prompt → base_content
}
```

#### `config.rs:21-43` — HelveConfig

```rust
#[derive(Clone, Debug, Default)]
pub struct HelveConfig {
    pub working_folder: Option<PathBuf>,
    pub thread_id: Option<String>,
    pub user_id: Option<String>,
    pub approval_policy: Option<ApprovalPolicy>,
    pub role_setting: Option<String>,
    pub agents_md: Option<String>,
    pub system_prompt_override: Option<String>,
    pub skills_prompt: Option<String>,
    pub env_context: Option<String>,       // ← 同样是裸字符串
}
```

#### `config.rs:66-89` — HelveConfig → ReactPromptInputs

```rust
pub fn to_react_build_config(helve: &HelveConfig, base: ReactBuildConfig) -> ReactBuildConfig {
    let prompt_inputs = ReactPromptInputs {
        full_override: helve.system_prompt_override.clone(),
        base_prompt_override: base.system_prompt.clone(),
        role_setting: helve.role_setting.clone(),
        agents_md: helve.agents_md.clone(),
        skills_prompt: helve.skills_prompt.clone(),
        env_context: helve.env_context.clone(),     // ← 直接 clone 透传
        working_folder: helve.working_folder.clone(),
        approval_policy: helve.approval_policy,
    };
    // ...
}
```

#### `cli_run/mod.rs:222-232` — 唯一的构造调用点

```rust
let helve = HelveConfig {
    working_folder: Some(working_folder.clone()),
    thread_id: effective_opts.thread_id.clone(),
    user_id: base.user_id.clone(),
    approval_policy: None,
    role_setting: agent_instructions,
    agents_md: load_agents_md(Some(&working_folder)),
    system_prompt_override: None,
    skills_prompt,
    env_context: Some(crate::helve::build_env_context()),   // ← 唯一调用
};
```

#### `openai_sse/parse.rs:139` — 默认空值

```rust
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// 工作目录中检测到的编程语言（按文件数降序）
    pub languages: Vec<String>,
    /// 是否在 git 仓库中
    pub has_git: bool,
}

/// 文件后缀 → 语言名称的内置映射表。
const EXTENSION_LANGUAGE_MAP: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("ts", "typescript"), ("tsx", "typescript"),
    ("js", "javascript"), ("jsx", "javascript"),
    ("py", "python"),
    ("go", "go"),
    ("java", "java"),
    ("rb", "ruby"),
    ("c", "c"), ("h", "c"),
    ("cpp", "cpp"), ("hpp", "cpp"), ("cc", "cpp"),
    ("cs", "csharp"),
    ("swift", "swift"),
    ("kt", "kotlin"),
    ("zig", "zig"),
    ("lua", "lua"),
    ("php", "php"),
];

/// 采样时跳过的目录。
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "dist", "build", "vendor",
    "__pycache__", ".next", ".nuxt", "bazel-out",
];

/// 报告语言的最小文件数阈值。
const MIN_FILE_THRESHOLD: usize = 3;
```

#### `mod.rs:44-48` — 模块 re-export

```rust
pub use config::{to_react_build_config, HelveConfig};
pub use prompt::{
    assemble_react_system_prompt, assemble_system_prompt, build_env_context,
    tools_requiring_approval, ApprovalPolicy, ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
};
```

#### `lib.rs:195-199` — crate 级 re-export

```rust
pub use helve::{
    assemble_react_system_prompt, assemble_system_prompt, build_env_context,
    to_react_build_config, tools_requiring_approval, ApprovalPolicy, HelveConfig,
    ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
};
```

### 1.3 问题总结

1. **信息维度有限** — 只有 OS 和 LANG，缺少 shell 类型、系统架构、项目语言、容器标志等
2. **不可扩展** — 新增维度需要改 `format!` 字符串，容易出错
3. **概念不突出** — 环境上下文是 `Option<String>`，没有独立的数据模型
4. **不可覆盖** — 用户无法通过配置覆盖检测值（如偏好回复语言）
5. **无法测试** — 采集逻辑和渲染逻辑耦合，无法单独测试

---

## 二、调整方案

### 2.1 新增 `env_context.rs` 模块

文件：`loom/src/helve/env_context.rs`

```rust
/// 运行时环境上下文，注入到 system prompt 最前部。
///
/// 帮助 agent 感知运行环境，从而适配 shell 命令、回复语言、路径格式等行为。
#[derive(Debug, Clone)]
pub struct EnvContext {
    pub os: OsInfo,
    pub locale: LocaleInfo,
    pub shell: Option<ShellInfo>,
    pub project: Option<ProjectInfo>,
    pub runtime: Option<RuntimeInfo>,
}

impl Default for EnvContext {
    fn default() -> Self {
        Self {
            os: OsInfo::default(),
            locale: LocaleInfo::default(),
            shell: None,
            project: None,
            runtime: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OsInfo {
    pub family: String,           // "macos" | "linux" | "windows"
    pub version: Option<String>,  // e.g. "Darwin 24.6.0"
    pub arch: String,             // "aarch64" | "x86_64"
}

#[derive(Debug, Clone)]
pub struct LocaleInfo {
    /// 检测到的系统 locale (e.g. "zh_CN.UTF-8")
    pub detected: String,
    /// 提取的语言标签 (e.g. "zh_CN")
    pub language: String,
    /// 用户显式设置的偏好回复语言 (覆盖 language)
    pub preferred_reply_language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShellInfo {
    pub name: String,      // "zsh" | "bash" | "fish" | "powershell"
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// 工作目录中检测到的编程语言
    pub languages: Vec<String>,
    /// 是否在 git 仓库中
    pub has_git: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeInfo {
    pub agent_name: String,   // "Loom"
    pub is_container: bool,
}
```

### 2.2 核心方法

```rust
impl EnvContext {
    pub fn detect() -> Self;

    /// 渲染为 prompt 文本段（使用 std::fmt::Write 硬编码渲染）。
    pub fn to_prompt_section(&self) -> String;

    /// Builder: 覆盖偏好回复语言。
    pub fn with_reply_language(mut self, lang: impl Into<String>) -> Self;

    /// Builder: 覆盖 shell 信息。
    pub fn with_shell(mut self, shell: ShellInfo) -> Self;

    /// Builder: 设置项目信息。
    pub fn with_project(mut self, project: ProjectInfo) -> Self;
}

impl OsInfo       { pub fn detect() -> Self; }
impl LocaleInfo   { pub fn detect() -> Self; }
impl ShellInfo    { pub fn detect() -> Self; }
impl ProjectInfo  { pub fn detect(working_dir: &Path) -> Self; }
impl RuntimeInfo  { pub fn detect() -> Self; }
```

### 2.3 采集策略

| 维度 | 采集方式 | 失败回退 |
|------|---------|---------|
| OS family | `std::env::consts::OS` | 必定成功 |
| OS version | macOS: `sw_vers`, Linux: `/etc/os-release`, Windows: env | `None` |
| Arch | `std::env::consts::ARCH` | 必定成功 |
| Locale | `LANG` / `LC_ALL` / `LANGUAGE` env vars | `"en_US.UTF-8"` |
| Shell | `SHELL` env / parent process / `$0` | `None` |
| Project languages | 文件后缀采样：遍历工作目录前 2 层，统计后缀数量，映射表 `.rs`→rust / `.ts/.tsx`→typescript / `.py`→python / `.go`→go / `.java`→java / `.rb`→ruby 等，仅报告文件数 ≥ 3 的语言，跳过 `node_modules`/`.git`/`target`/`dist` 等排除目录 | `None` |
| Git | 检查 `.git/` 目录 | `false` |
| Container | 检查 `/.dockerenv` 或 `/proc/1/cgroup` | `false` |

### 2.4 用户配置覆盖

在 `.loom/config.yaml` 中支持：

```yaml
env:
  reply_language: "中文"
  shell: "powershell"
```

`EnvContext::detect()` 读取后通过 builder 方法覆盖。

### 2.5 各文件具体调整

#### `prompt.rs` — 类型变更 + 逻辑调整

**ReactPromptInputs.env_context 类型变更：**

```rust
// before
pub env_context: Option<String>,

// after
pub env_context: Option<EnvContext>,
```

**collect_prefix_sections() 类型适配：**

`env_context` 不再是 `String`，无法直接 `as_deref()`。返回类型从 `Vec<&str>` 改为 `Vec<String>`：

```rust
// before (prompt.rs:98-110)
fn collect_prefix_sections(inputs: &ReactPromptInputs) -> Vec<&str> {
    [
        inputs.env_context.as_deref(),
        inputs.role_setting.as_deref(),
        inputs.agents_md.as_deref(),
        inputs.skills_prompt.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .collect()
}

// after
fn collect_prefix_sections(inputs: &ReactPromptInputs) -> Vec<String> {
    let mut sections = Vec::new();
    if let Some(ctx) = &inputs.env_context {
        let s = ctx.to_prompt_section();
        if !s.trim().is_empty() { sections.push(s); }
    }
    if let Some(s) = &inputs.role_setting {
        let s = s.trim().to_string();
        if !s.is_empty() { sections.push(s); }
    }
    if let Some(s) = &inputs.agents_md {
        let s = s.trim().to_string();
        if !s.is_empty() { sections.push(s); }
    }
    if let Some(s) = &inputs.skills_prompt {
        let s = s.trim().to_string();
        if !s.is_empty() { sections.push(s); }
    }
    sections
}
```

`assemble_react_system_prompt` 中的 `prefix_sections.join("\n\n")` 不受影响（`String` 也能 `join`）。

**删除 `build_env_context()`：**

`prompt.rs:165-178` 整个函数删除。

**测试重写：**

- `env_context_prepended_before_role_setting` (:236) — 构造 `EnvContext` 替代字符串
- `build_env_context_contains_os_and_agent` (:252) — 改为测试 `EnvContext::detect().to_prompt_section()`

#### `config.rs` — HelveConfig 类型同步

```rust
// before (config.rs:42)
pub env_context: Option<String>,

// after
pub env_context: Option<EnvContext>,
```

`to_react_build_config()` 中 `env_context: helve.env_context.clone()` 无需改逻辑，只是类型变了。

**测试重写：**

- `to_react_build_config_env_context_first` (:218) — 构造 `EnvContext` 替代 `Some("ENV".to_string())`

#### `cli_run/mod.rs` — 调用点

```rust
// before (:231)
env_context: Some(crate::helve::build_env_context()),

// after
let mut ctx = EnvContext::detect()
    .with_project(ProjectInfo::detect(&working_dir));
if let Some(lang) = config.reply_language() {
    ctx = ctx.with_reply_language(lang);
}
// ...
env_context: Some(ctx),
```

#### `mod.rs` — 模块声明和 re-export

```rust
// before (mod.rs:41-48)
mod config;
mod prompt;

pub use config::{to_react_build_config, HelveConfig};
pub use prompt::{
    assemble_react_system_prompt, assemble_system_prompt, build_env_context,
    tools_requiring_approval, ApprovalPolicy, ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
};

// after
pub mod env_context;
mod config;
mod prompt;

pub use config::{to_react_build_config, HelveConfig};
pub use env_context::{EnvContext, OsInfo, LocaleInfo, ShellInfo, ProjectInfo, RuntimeInfo};
pub use prompt::{
    assemble_react_system_prompt, assemble_system_prompt,
    tools_requiring_approval, ApprovalPolicy, ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
};
```

#### `lib.rs` — crate 级 re-export

```rust
// before (:195-199)
pub use helve::{
    assemble_react_system_prompt, assemble_system_prompt, build_env_context,
    to_react_build_config, tools_requiring_approval, ApprovalPolicy, HelveConfig,
    ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
};

// after
pub use helve::{
    assemble_react_system_prompt, assemble_system_prompt,
    to_react_build_config, tools_requiring_approval, ApprovalPolicy, HelveConfig,
    ReactPromptInputs, APPROVAL_REQUIRED_EVENT_TYPE,
    EnvContext, OsInfo, LocaleInfo, ShellInfo, ProjectInfo, RuntimeInfo,
};
```

#### `openai_sse/parse.rs` — 自动适配

`parse.rs:139` 的 `env_context: None` 类型自动适配，无需改逻辑。

---

## 三、调整后的结果

### 3.1 新的数据流

```
cli_run/mod.rs:231
    ↓ EnvContext::detect().with_project(...)
EnvContext (结构化)
    ↓
HelveConfig.env_context     ← Option<EnvContext>
    ↓
config.rs:73                ← clone 透传
    ↓
ReactPromptInputs.env_context
    ↓
prompt.rs collect_prefix_sections()
    ↓ ctx.to_prompt_section() 渲染为 String
    ↓
assemble_react_system_prompt() 拼接到最前部
    ↓
最终 system prompt
```

### 3.2 渲染输出示例

完整输出（所有字段都检测到时）：

```
ENVIRONMENT:
- OS: macos (Darwin 24.6.0, aarch64)
- Locale: zh_CN.UTF-8
- Reply language: 中文
- Shell: zsh (/bin/zsh)
- Project languages: rust, typescript
- Git: yes
- Agent: Loom
- Container: docker
```

最小输出（仅必填字段）：

```
ENVIRONMENT:
- OS: macos
- Locale: en_US.UTF-8
- Agent: Loom
```

渲染规则：

- 仅在 `preferred_reply_language` 设置时输出 `Reply language` 行
- 仅在 `is_container: true` 时输出 `Container` 行
- `Shell` / `Project languages` / `Git` 仅在检测到时输出（对应 `Option` 字段）
- `OS version` 和 `arch` 仅在有值时附加在 `OS:` 行括号中

### 3.3 文件变更清单

| 文件 | 变更 |
|------|------|
| `loom/src/helve/env_context.rs` | **新增** — 核心类型、采集逻辑、渲染逻辑 |
| `loom/src/helve/mod.rs` | 增加 `pub mod env_context;`，re-export 新类型，移除 `build_env_context` |
| `loom/src/helve/prompt.rs` | `env_context` 类型改为 `Option<EnvContext>`，`collect_prefix_sections()` 返回 `Vec<String>`，删除 `build_env_context()`，重写 2 个测试 |
| `loom/src/helve/config.rs` | `HelveConfig.env_context` 类型同步改为 `Option<EnvContext>`，重写 1 个测试 |
| `loom/src/cli_run/mod.rs` | 调用 `EnvContext::detect()` 替代 `build_env_context()` |
| `loom/src/openai_sse/parse.rs` | `env_context: None` 类型自动适配，无需改逻辑 |
| `loom/src/lib.rs` | 更新 re-export，移除 `build_env_context`，新增 `EnvContext` 等类型 |

### 3.4 测试计划

**新增测试（env_context.rs）：**

- `OsInfo::detect()` — 各平台基础测试
- `LocaleInfo::detect()` — 模拟不同 LANG 环境变量
- `ShellInfo::detect()` — 模拟 SHELL 环境变量
- `ProjectInfo::detect()` — 临时目录构造不同项目结构（多语言混合、排除目录、阈值过滤）
- `EnvContext::to_prompt_section()` — 快照测试，验证输出格式
- `with_reply_language()` — 验证覆盖后输出包含 `Reply language` 行

**重写测试：**

- `prompt.rs::env_context_prepended_before_role_setting` — 构造 `EnvContext` 替代字符串
- `prompt.rs::build_env_context_contains_os_and_agent` — 改为测试 `EnvContext::detect().to_prompt_section()`
- `config.rs::to_react_build_config_env_context_first` — 构造 `EnvContext` 替代 `String`

**回归测试：**

- `assemble_react_system_prompt` — `env_context: None` 时输出不变
- `assemble_system_prompt_includes_workdir_and_base` — 无 env_context 时行为不变

### 3.5 迁移步骤

`ReactPromptInputs` 和 `HelveConfig` 是内部类型，无外部消费者。直接改类型，一次到位。

1. 新增 `env_context.rs`，实现所有类型和采集逻辑
2. 修改 `ReactPromptInputs.env_context` 类型为 `Option<EnvContext>`
3. 修改 `HelveConfig.env_context` 类型为 `Option<EnvContext>`
4. 调整 `collect_prefix_sections()` 返回类型为 `Vec<String>`
5. 更新 `cli_run/mod.rs` 调用 `EnvContext::detect()`
6. 更新 `mod.rs` / `lib.rs` re-export
7. 删除 `build_env_context()`
8. 重写受影响的测试，补充新测试

---

## 四、开放问题

1. **采样阈值调优** — `MIN_FILE_THRESHOLD = 3` 是否合适？小项目（如纯 Python 脚本项目）可能只有 1-2 个文件，是否需要按语言调整阈值？
2. **locale 到自然语言的映射** — `zh_CN` → "中文" 的映射表放在哪里？硬编码还是可配置？
3. **容器检测精度** — 当前方案只检测 Docker，是否需要支持 Podman/K8s 等场景？
4. **模板引擎统一** — 现有 prompt 系统（react.yaml, helve.yaml）都是纯 YAML 值，没有模板占位符。引入模板引擎是独立议题，不应在此 RFC 中绑定
5. **渲染方式** — 第一版使用 `std::fmt::Write` 硬编码渲染。模板化作为 future work
