# 开发计划：EnvContext

> 关联 RFC: `docs/rfcs/env-context.md`

---

## Phase 1：类型定义与采集逻辑

**目标**：新增 `env_context.rs`，实现所有结构体和 `detect()` 方法。

### 1.1 骨架与基础类型

- 创建 `loom/src/helve/env_context.rs`
- 定义 `EnvContext`、`OsInfo`、`LocaleInfo`、`ShellInfo`、`ProjectInfo`、`RuntimeInfo`
- 实现 `Default`
- 在 `mod.rs` 中注册 `pub mod env_context;`

### 1.2 OsInfo::detect()

- 使用 `std::env::consts::OS` 获取 family
- 使用 `std::env::consts::ARCH` 获取 arch
- macOS: 调用 `sw_vers -productVersion` 获取 version
- Linux: 解析 `/etc/os-release` 获取 version
- Windows: 读取 `OS` 环境变量

### 1.3 LocaleInfo::detect()

- 依次检测 `LANG` → `LC_ALL` → `LANGUAGE` 环境变量
- 解析语言标签（`zh_CN.UTF-8` → `language: "zh_CN"`, `detected: "zh_CN.UTF-8"`）
- `preferred_reply_language` 初始为 `None`

### 1.4 ShellInfo::detect()

- 读取 `SHELL` 环境变量
- 从路径提取 name（`/bin/zsh` → `"zsh"`）
- Windows 回退：检测 `COMSPEC` → `"powershell"` / `"cmd"`
- 均无则返回 `None`

### 1.5 ProjectInfo::detect(working_dir)

采用文件后缀采样策略（类似 GitHub Linguist 简化版）：

- 遍历工作目录前 2 层（含根目录），跳过 `node_modules`/`.git`/`target`/`dist`/`build`/`vendor` 等常见排除目录
- 统计各后缀的文件数量，使用内置后缀→语言映射表：
  - `.rs` → `"rust"`
  - `.ts` / `.tsx` → `"typescript"`
  - `.js` / `.jsx` → `"javascript"`
  - `.py` → `"python"`
  - `.go` → `"go"`
  - `.java` → `"java"`
  - `.rb` → `"ruby"`
  - `.c` / `.h` → `"c"`
  - `.cpp` / `.hpp` / `.cc` → `"cpp"`
  - `.cs` → `"csharp"`
  - `.swift` → `"swift"`
  - `.kt` → `"kotlin"`
  - `.rs` → `"rust"`
  - `.zig` → `"zig"`
  - `.lua` → `"lua"`
  - `.php` → `"php"`
- 仅报告文件数 ≥ 3 的语言（避免偶然文件误报）
- 按文件数降序排列
- 同时检查 `.git/` 目录存在 → `has_git`
- 空目录时 `languages` 为空 `Vec`

### 1.6 RuntimeInfo::detect()

- `agent_name` 硬编码 `"Loom"`
- `is_container`: 检查 `/.dockerenv` 存在 或 `/proc/1/cgroup` 包含 `docker`/`kubepods`

### 1.7 EnvContext::detect()

- 组合调用各子类型的 `detect()`
- `shell` / `project` / `runtime` 包装为 `Option`

### 1.8 单元测试

- `OsInfo::detect()` — 验证 family 非空、arch 非空
- `LocaleInfo::detect()` — 验证 detected 和 language 格式
- `ShellInfo::detect()` — 验证 name 是已知 shell 之一或 None
- `ProjectInfo::detect()` — 临时目录放多个 `.rs` 文件，验证 languages 含 `"rust"`；放单个 `.py` 不满足阈值，验证不报告
- `RuntimeInfo::detect()` — 验证 agent_name 为 `"Loom"`

**验证点**：`cargo test --lib helve::env_context` 全部通过

---

## Phase 2：渲染逻辑

**目标**：实现 `to_prompt_section()` 和 builder 方法。

### 2.1 to_prompt_section()

- 使用 `std::fmt::Write` 渲染
- 必选行：`OS:`、`Locale:`、`Agent:`
- 条件行：
  - `Reply language:` — `preferred_reply_language.is_some()`
  - `Shell:` — `shell.is_some()`
  - `Project languages:` — `project.is_some() && !languages.is_empty()`
  - `Git:` — `project.is_some() && has_git`
  - `Container:` — `runtime.is_some() && is_container`
- `OS:` 行格式：`macos` / `macos (Darwin 24.6.0)` / `macos (Darwin 24.6.0, aarch64)`

### 2.2 Builder 方法

- `with_reply_language(lang)` — 设置 `locale.preferred_reply_language`
- `with_shell(shell)` — 设置 `shell`
- `with_project(project)` — 设置 `project`

### 2.3 单元测试

- `to_prompt_section()` 最小输出快照 — 只有 OS/Locale/Agent 三行
- `to_prompt_section()` 完整输出快照 — 所有字段都有值
- `to_prompt_section()` 条件行不出现 — shell=None 时无 Shell 行
- `with_reply_language()` — 设置后输出包含 `Reply language: 中文`
- `with_project()` — 设置后输出包含 `Project languages:` 和 `Git:`

**验证点**：`cargo test --lib helve::env_context` 全部通过

---

## Phase 3：集成到 prompt 系统

**目标**：替换 `Option<String>` 为 `Option<EnvContext>`，修改所有引用点。

### 3.1 ReactPromptInputs 类型变更

- `prompt.rs`: `env_context: Option<String>` → `Option<EnvContext>`
- 添加 `use super::env_context::EnvContext;`

### 3.2 collect_prefix_sections() 适配

- 返回类型 `Vec<&str>` → `Vec<String>`
- `env_context` 分支：`ctx.to_prompt_section()`
- 其余字段：`s.trim().to_string()`
- 验证 `assemble_react_system_prompt` 中 `join("\n\n")` 仍工作

### 3.3 HelveConfig 类型同步

- `config.rs`: `env_context: Option<String>` → `Option<EnvContext>`
- 添加 `use super::env_context::EnvContext;`
- `to_react_build_config()` 中 clone 逻辑不变

### 3.4 cli_run/mod.rs 调用点

- `env_context: Some(crate::helve::build_env_context())`
  → `env_context: Some(EnvContext::detect().with_project(ProjectInfo::detect(&working_dir)))`
- 添加 `use crate::helve::env_context::{EnvContext, ProjectInfo};`

### 3.5 re-export 更新

- `mod.rs`: 添加 `pub use env_context::{EnvContext, OsInfo, ...}`，移除 `build_env_context`
- `lib.rs`: 添加 `EnvContext, OsInfo, ...`，移除 `build_env_context`

### 3.6 删除 build_env_context()

- 删除 `prompt.rs:165-178` 的函数定义
- 确认无其他引用

### 3.7 集成测试

- 重写 `env_context_prepended_before_role_setting` — 构造 `EnvContext`
- 重写 `build_env_context_contains_os_and_agent` — 测试 `EnvContext::detect().to_prompt_section()`
- 重写 `to_react_build_config_env_context_first` — 构造 `EnvContext`
- 回归：`assemble_system_prompt_includes_workdir_and_base` 仍通过
- 回归：`assemble_react_system_prompt_assembles_prefix_and_sections` 仍通过

**验证点**：`cargo test` 全部通过

---

## Phase 4：配置覆盖

**目标**：支持 `.loom/config.yaml` 中的 `env` 配置。

### 4.1 配置结构扩展

- 在 config 解析层新增 `EnvOverride` 结构
- 支持 `env.reply_language` 和 `env.shell` 字段

### 4.2 覆盖逻辑

- `cli_run/mod.rs` 中读取配置后调用 `with_reply_language()` / `with_shell()`

### 4.3 测试

- 有配置时 `to_prompt_section()` 输出 `Reply language` 行
- 无配置时行为不变

**验证点**：`cargo test` 全部通过

---

## Phase 5：编译验证与收尾

**目标**：全量编译、测试、文档更新。

### 5.1 全量编译

- `cargo build` 无 warning
- `cargo clippy` 无新 warning

### 5.2 全量测试

- `cargo test` 全部通过
- `cargo test --all` （workspace 级别）

### 5.3 文档更新

- `helve/mod.rs` 模块文档更新，提及 `env_context` 模块
- `env_context.rs` 顶部模块文档补充使用示例

---

## 风险与依赖

| 风险 | 影响 | 缓解 |
|------|------|------|
| `sw_vers` 在非 macOS 不可用 | OsInfo::version 返回 None | 已有回退 |
| `ProjectInfo::detect` 文件后缀采样深度 2 层，大仓库性能可控 | 遍历耗时 | 跳过 `node_modules`/`target` 等排除目录，限制深度 2 层 |
| `collect_prefix_sections` 返回类型变更影响其他测试 | 测试编译失败 | Phase 3 统一处理 |
| `is_container` 检测在 WSL 环境误判 | 输出错误信息 | WSL 检测作为开放问题 |

## 工时估算

| Phase | 预估 |
|-------|------|
| Phase 1: 类型与采集 | 2-3h |
| Phase 2: 渲染逻辑 | 1-2h |
| Phase 3: 集成 | 2-3h |
| Phase 4: 配置覆盖 | 1h |
| Phase 5: 收尾 | 1h |
| **合计** | **7-10h** |
