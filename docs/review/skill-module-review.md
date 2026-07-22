# Skill 模块 Code Review

> 审查日期：2025-08-19
> 范围：`agent/skill/`、`agent/tool/tool-basic/src/skill/`、`agent/tool/tool-luft/`、`experimental/curator/`、`agent-core/src/run/config_builder.rs`

---

## 1. 模块概览

| 模块 | 路径 | 职责 |
|------|------|------|
| skill-core | `agent/skill/src/` | 技能发现、加载、存储、安全扫描 |
| skill-tools | `agent/tool/tool-basic/src/skill/` | skill_list / skill_view / skill_manage 三个工具 |
| builtin-skill | `agent/tool/tool-core/src/tool.rs` | `Tool::builtin_skill()` trait 方法 + `BuiltinSkill` 结构 |
| luft-builtin | `agent/tool/tool-luft/src/` | 首个 builtin skill 用例（`luft-workflow-dsl`） |
| curator-registry | `experimental/curator/src/skill_registry.rs` | Curator 独立 SkillRegistry 扩展 |
| config-builder | `agent/agent-core/src/run/config_builder.rs` | 技能注入到 agent 运行时的入口 |

---

## 2. 核心类型

### 2.1 SkillSource（技能来源）

```
agent/skill/src/discovery.rs:35-47
```

```rust
pub enum SkillSource {
    Project,    // .loom/skills/        （项目级）
    ProfileDir, // .loom/agents/<name>/skills/ （Agent profile 级）
    User,       // ~/.loom/skills/      （用户级）
    Agent,      // ~/.loom/agents/<name>/skills/ （Agent 级）
    Data,       // ~/.loom/data/skills/ （data 级，curator 用）
    Builtin,    // include_str! 嵌入    （内置，最低优先级）
}
```

**优先级**：磁盘 skill（Project > ProfileDir > User > Agent > Data）> Builtin。
同名 skill 已存在于磁盘时，`add_builtin` 自动跳过。

`SkillSource::label()` 返回短文本标签（`discovery.rs:49-60`），用于 CLI banner 显示。

### 2.2 SkillEntry（技能条目）

```
agent/skill/src/discovery.rs:25-33
```

```rust
pub struct SkillEntry {
    pub metadata: SkillMetadata,
    pub source: SkillSource,
    pub dir: Option<PathBuf>,          // 磁盘 skill 的目录
    pub embedded_content: Option<String>, // builtin skill 的内嵌内容
}
```

- 磁盘 skill：`dir = Some(...)`，`embedded_content = None`
- builtin skill：`dir = None`，`embedded_content = Some(...)`
- `load_skill_with_dir()` 优先使用 `embedded_content`（`discovery.rs:189-196`）

### 2.3 SkillMetadata（元数据）

```
agent/skill/src/utils.rs:100-130
```

```rust
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub category: Option<String>,
    pub conditions: SkillConditions,  // requires_tools, requires_toolsets
    // ...
}
```

通过 `parse_skill_frontmatter()` 从 SKILL.md 的 YAML frontmatter 解析。

### 2.4 SkillRegistry（注册表）

```
agent/skill/src/discovery.rs:100-170
```

核心方法：

| 方法 | 行号 | 职责 |
|------|------|------|
| `discover(working_folder, extra_dirs)` | ~110 | 扫描 5 个标准路径 |
| `add_builtin(name, desc, content, triggers, requires_tools)` | ~135 | 注入内置技能 |
| `load_skill(name)` | ~175 | 按名加载单个技能（含 embedded fallback） |
| `available_skills_prompt()` | ~200 | 生成 `<available_skills>` 提示块 |
| `list()` | ~155 | 返回所有已发现条目 |
| `apply_toolset_filters(toolset)` | ~250 | 按 toolset 过滤可见技能 |

---

## 3. 技能发现路径

```
agent/skill/src/discovery.rs  discover()
```

扫描顺序（高优先级 → 低）：

```
1. .loom/skills/                           → SkillSource::Project
2. .loom/agents/<agent-name>/skills/       → SkillSource::ProfileDir
3. ~/.loom/skills/                          → SkillSource::User
4. ~/.loom/agents/<agent-name>/skills/     → SkillSource::Agent
5. ~/.loom/data/skills/                     → SkillSource::Data
6. Tool::builtin_skill() 注入              → SkillSource::Builtin
```

同名技能：先发现的优先，后续跳过。

---

## 4. 内置技能机制（builtin_skill）

### 4.1 设计

```
docs/design/builtin-skill-from-tool.md
```

Tool trait 新增默认方法：

```rust
// agent/tool/tool-core/src/tool.rs:40
fn builtin_skill(&self) -> Option<BuiltinSkill> { None }
```

`BuiltinSkill` 结构：

```rust
pub struct BuiltinSkill {
    pub name: String,
    pub description: String,
    pub content: String,        // SKILL.md 全文
    pub triggers: Vec<String>,
    pub requires_tools: Vec<String>,
}
```

### 4.2 注入时机

```
agent/agent-core/src/run/config_builder.rs:267-275
```

在 `discover()` 完成后、`apply_filters()` 之前，遍历所有 tool 的 `builtin_skill()`：

```rust
fn inject_builtin_skills(tools: &[Box<dyn Tool>], registry: &mut SkillRegistry) {
    for tool in tools {
        if let Some(skill) = tool.builtin_skill() {
            registry.add_builtin(&skill.name, &skill.description,
                &skill.content, &skill.triggers, &skill.requires_tools);
        }
    }
}
```

### 4.3 首个用例：LuftTool

```
agent/tool/tool-luft/src/tool.rs:469-482
```

- 内嵌 DSL 文档：`include_str!("luft_workflow_dsl.md")`（6654 字节 / ~1663 tokens）
- tool description 引导："For the full DSL reference... load the `luft-workflow-dsl` skill."
- 用户可在 `.loom/skills/luft-workflow-dsl/SKILL.md` 覆盖内置版本

### 4.4 落地状态

| 组件 | 状态 |
|------|------|
| `Tool::builtin_skill()` 默认方法 | ✅ 已实现 |
| `BuiltinSkill` 结构体 | ✅ 已实现 |
| `SkillSource::Builtin` 变体 | ✅ 已实现 |
| `SkillRegistry::add_builtin()` | ✅ 已实现 |
| `inject_builtin_skills()` | ✅ 已实现 |
| LuftTool builtin_skill() | ✅ 已实现 |
| `SkillSource::label()` | ✅ 已实现（本次新增） |
| 单元测试 | ✅ discovery.rs:576-625 |
| 集成测试 | ✅ tool-luft/tests/builtin_skill.rs |
| 端到端验证 | ✅ tool-luft/examples/validate_skill.rs |

---

## 5. 安全扫描

### 5.1 架构

```
agent/skill/src/guard.rs       — 核心扫描引擎
agent/skill/src/security.rs    — agent-created 包装器
```

### 5.2 信任级别

```
guard.rs  resolve_trust_level()
```

| TrustLevel | 来源 | Critical/High 判决 |
|------------|------|---------------------|
| Builtin | 内置 | Safe（直接通过） |
| Trusted | 官方仓库 | Warning |
| AgentCreated | Agent 创建 | Warning |
| Community | 社区 | **Blocked** |

> **设计决策**：Loom 对 AgentCreated 的处理比 Hermes Python 版更严格。
> Hermes: caution → allow；Loom: Critical/High → Warning。

### 5.3 检测项

| 检测 | 位置 | 说明 |
|------|------|------|
| 正则危险模式 | `guard.rs:65-70` | 100+ 条正则规则匹配 shell 注入、路径遍历等 |
| 不可见字符 | `guard.rs:65-70` | 15 种 Unicode 不可见字符 |
| 符号链接转义 | `guard.rs:356-371` | 检测指向技能目录外的 symlink |
| 文件数量限制 | `guard.rs:208-210` | 最大文件数限制 |
| 文件大小限制 | `guard.rs:208-210` | 单文件大小限制 |

### 5.4 安装决策

```
guard.rs:521-605  should_allow_install()
```

- Community + Critical/High → **Block**（不可 `--force` 覆盖）
- 其他级别 + Critical/High → **Warning**（可继续）
- 无 Critical/High → **Safe**

---

## 6. Skill 工具

### 6.1 skill_list

```
agent/tool/tool-basic/src/skill/list.rs
```

- 支持两种模式：SkillRegistry 模式 + 目录扫描模式
- 支持 `category` 过滤器
- 返回 `<available_skills>` 格式列表

### 6.2 skill_view

```
agent/tool/tool-basic/src/skill/view.rs
```

- 按 name 加载技能全文（含 frontmatter + body）
- 支持子文件加载（`file_path` 参数）
- **路径遍历防护**：`view.rs:239-244` 检查 `..` 和绝对路径
- view 计数与 use 计数分离

### 6.3 skill_manage

```
agent/tool/tool-basic/src/skill/manage.rs
```

6 种操作：

| 操作 | 说明 |
|------|------|
| create | 创建新技能（含 frontmatter 验证） |
| patch | 部分替换（old_string → new_string） |
| edit | 全量重写 |
| delete | 删除技能（支持 `absorbed_into` 声明合并意图） |
| write_file | 写入子文件（references/ templates/ scripts/ assets/） |
| remove_file | 删除子文件 |

- 文件大小限制：`MAX_SKILL_FILE_BYTES = 1_048_576`（1 MB）
- 每次操作后 `invalidate_discovery_cache()`

---

## 7. Curator 独立 SkillRegistry

### 7.1 架构

```
experimental/curator/src/skill_registry.rs
```

Curator 维护独立的 SkillRegistry，仅扫描 `~/.loom/data/skills/`（`SkillSource::Data`）。

扩展 trait `SkillRegistryExt` 提供 curating 专用方法。

### 7.2 双 Registry 问题

| | Agent SkillRegistry | Curator SkillRegistry |
|---|---|---|
| 扫描路径 | 5 个标准路径 + Builtin | 仅 `~/.loom/data/skills/` |
| 可见技能 | Project + User + Agent + Data + Builtin | 仅 Data |
| 用途 | 运行时加载 | 后台审查/合并/去重 |

**风险**：Curator 看不到 Project/User/Agent/Builtin 技能，导致：
- 合并提议可能创建重复技能
- 去重操作可能遗漏跨来源冲突
- 无法感知 builtin skill 的存在

---

## 8. 发现的问题

### 8.1 严重

#### P0-1: 双 SkillRegistry 不一致

- **位置**：`experimental/curator/src/skill_registry.rs` vs `agent/skill/src/discovery.rs`
- **影响**：Curator 合并/去重遗漏跨来源技能
- **建议**：统一 Registry 或增加同步机制

#### P0-2: 安全扫描 catch_unwind 静默成功

- **位置**：`agent/skill/src/security.rs:60-85`
- **影响**：扫描 panic 时返回成功，可能隐藏安全问题
- **建议**：改为 `Result` 返回 Err

### 8.2 中等

#### P1-1: build_react_config 返回值膨胀

- **位置**：`agent/agent-core/src/run/config_builder.rs:60`
- **现状**：`(ReactBuildConfig, Option<ResolvedAgent>, Option<Arc<SkillRegistry>>)`
- **建议**：包装为 struct `BuildResult`

#### P1-2: skill_list 双模式代码重复

- **位置**：`agent/tool/tool-basic/src/skill/list.rs:63-163`
- **现状**：registry 模式和目录扫描模式各有一套实现
- **建议**：统一到 registry 模式

#### P1-3: AgentCreated 信任级别策略差异

- **位置**：`agent/skill/src/guard.rs:469-475`
- **现状**：Loom 对 AgentCreated 的 Critical/High 返回 Warning（比 Hermes 严格）
- **结论**：有意设计，无需修改，但需文档化

### 8.3 低

#### P2-1: 硬编码常量

- `SKILLS_SUBDIR = ".loom/skills"`（`tool-basic/src/skill/mod.rs:34`）
- Curator `WAIT_TIMEOUT = 60s`（`experimental/curator/src/workflow.rs:122`）

#### P2-2: 频繁缓存失效

- **位置**：`manage.rs:313,385,542`
- 每次技能操作都 `invalidate_discovery_cache()`
- 可优化为批量操作后统一失效

---

## 9. 测试覆盖

| 模块 | 测试数 | 状态 |
|------|--------|------|
| `skill --lib` | 172 | 7 pre-existing failures（path/usage/validation） |
| `cli --lib`（panel_format） | 16 | ✅ 全绿（含 7 个新增） |
| `agent --lib` | 364 | ✅ 全绿 |
| `tool-luft` builtin_skill | 4 | ✅ 全绿 |
| `tool-luft` validate_skill example | 1 | ✅ 通过 |

预存在失败（与本次改动无关）：
- `storage::excluded_path_tests`（2 个）— dotfile/pycache 路径排除
- `usage::tests`（3 个）— 使用计数逻辑
- `validation::tests`（1 个）— 路径验证
- 这些在 memory 中已记录为「15+ 预存在失败」的一部分

---

## 10. 文件索引

| 文件 | 行数 | 关键内容 |
|------|------|----------|
| `agent/skill/src/discovery.rs` | ~625 | SkillRegistry, SkillSource, SkillEntry, add_builtin, discover |
| `agent/skill/src/guard.rs` | ~605 | scan_skill, scan_file, determine_verdict, should_allow_install |
| `agent/skill/src/security.rs` | ~100 | security_scan_skill, catch_unwind 包装 |
| `agent/skill/src/utils.rs` | ~250 | SkillMetadata, SkillConditions, parse_frontmatter |
| `agent/skill/src/storage.rs` | — | 使用计数、存储 |
| `agent/tool/tool-basic/src/skill/list.rs` | ~165 | skill_list 工具 |
| `agent/tool/tool-basic/src/skill/view.rs` | ~260 | skill_view 工具 |
| `agent/tool/tool-basic/src/skill/manage.rs` | ~875 | skill_manage 工具 |
| `agent/tool/tool-basic/src/skill/mod.rs` | ~120 | SkillContext, 工厂函数 |
| `agent/tool/tool-luft/src/tool.rs` | ~490 | LuftTool, builtin_skill() |
| `agent/agent-core/src/run/config_builder.rs` | ~630 | build_react_config, inject_builtin_skills |

---

## 附录 A：技能发现完整流程

```
用户执行 `loom -m "..."`

  ┌─ run_flow.rs
  │    args.verbose (u8) → RunOptions.verbose_level
  │    args.verbose >= 1 → RunOptions.verbose = true
  │
  └─ run/agent.rs
       │
       ├─ build_react_config(&opts)
       │    │
       │    ├─ SkillRegistry::discover(working_folder, extra_dirs)
       │    │    ├─ 扫描 .loom/skills/             → Project
       │    │    ├─ 扫描 .loom/agents/<name>/skills/ → ProfileDir
       │    │    ├─ 扫描 ~/.loom/skills/           → User
       │    │    ├─ 扫描 ~/.loom/agents/<name>/skills/ → Agent
       │    │    └─ 扫描 ~/.loom/data/skills/      → Data
       │    │
       │    ├─ inject_builtin_skills(tools, &mut registry)
       │    │    └─ Tool::builtin_skill() → add_builtin()
       │    │       （同名磁盘技能存在时跳过）
       │    │
       │    ├─ apply_toolset_filters(toolset)
       │    │    └─ 按 requires_tools 过滤
       │    │
       │    └─ return (config, resolved_agent, Some(Arc<SkillRegistry>))
       │
       ├─ Banner 打印（按 verbose_level）
       │    ├─ Level 0: _AGENT / _MODEL / _TOOLS（一行）
       │    ├─ Level 1: +_SKILLS（一行）
       │    └─ Level 2: _TOOLS 多行 + _SKILLS 多行（含 source/desc）
       │
       └─ 运行 agent loop
            └─ skill_list / skill_view / skill_manage 通过 SkillRegistry 操作
```
