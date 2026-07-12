# Tool 内置 Skill 机制

> 允许 Tool 向 Skill Registry 贡献内置 skill，使 agent 能通过 skill 系统（skill_view / nudge）按需加载工具的使用参考文档。

**创建时间**：2025-08-19
**状态**：方案设计

---

## 1. 背景与动机

### 1.1 问题

当前 Tool 只能通过 `ToolSpec.description`（~400 tokens）向 LLM 描述自身用法。对于复杂工具（如 luft），description 不够：

- **太短**：无法覆盖 DSL 语法、required structure、primitives 签名等
- **太长则浪费**：每次工具列表都注入，即使 agent 不需要该工具

Skill 系统支持按需加载（triggers / nudge），是自然的分层机制。但当前 skill 只从磁盘发现（`.loom/skills/`、`~/.loom/data/skills/`），没有 "Tool 自带 skill" 的途径。

### 1.2 目标

- Tool 可选地贡献内置 skill（reference documentation）
- Skill 内容嵌入 Tool crate 二进制（`include_str!`），零磁盘依赖
- Agent 通过现有 skill 机制（`skill_view`、nudge）按需加载
- 用户可在 `.loom/skills/` 覆盖内置版本（用户自定义优先）
- 对现有代码最小侵入

---

## 2. 现状分析

### 2.1 Skill Discovery

`SkillRegistry` (`agent/skill/src/discovery.rs`) 从 5 个磁盘路径扫描：

| Source | 路径 | 说明 |
|---|---|---|
| `Project` | `{working_folder}/.loom/skills/` | 项目级 |
| `ProfileDir` | extra_dirs 参数 | profile 级 |
| `User` | `~/.loom/skills/` | 用户级 |
| `Agent` | agent_skills_dir | agent 自动创建 |
| `Data` | `~/.loom/data/skills/` | 数据级（递归） |

`SkillEntry` 完全磁盘路径驱动：

```rust
pub struct SkillEntry {
    pub metadata: SkillMetadata,
    pub base_path: PathBuf,
    pub skill_file: PathBuf,   // load_skill_with_dir 从此路径 read_to_string
    pub source: SkillSource,
}
```

`load_skill_with_dir` 始终从磁盘读取内容，无法支持内存内嵌 skill。

### 2.2 Tool Trait

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn spec(&self) -> ToolSpec;
    async fn call(&self, args: Value, ctx: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError>;
}
```

没有机制让 Tool 向 skill registry 贡献内容。

### 2.3 Skill Nudge

Agent ReactLoop 中有 skill nudge 机制（`agent/agent-core/src/agent/react/nudge.rs`），按 `iters_since_skill` 计数器定期触发 skill review。内置 skill 可以被此机制自然发现和推荐。

---

## 3. 方案设计

### 3.1 改动概览

```
tool-core:  Tool trait + BuiltinSkill struct
     ↓
tool-luft:  Tool::builtin_skill() 实现 + lua_dsl_reference.md
     ↓
agent init: discover() 后遍历 tools，注入 builtin skills
     ↓
skill:      SkillSource + Builtin, SkillEntry + embedded_content, add_builtin()
```

### 3.2 Skill 层改动（`agent/skill/src/discovery.rs`）

#### SkillSource 新增 Builtin

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    Project,
    ProfileDir,
    User,
    Agent,
    Data,
    Builtin,  // 新增
}
```

#### SkillEntry 新增 embedded_content

```rust
pub struct SkillEntry {
    pub metadata: SkillMetadata,
    pub base_path: PathBuf,
    pub skill_file: PathBuf,
    pub source: SkillSource,
    pub embedded_content: Option<String>,  // 新增
}
```

#### load_skill_with_dir 优先用 embedded_content

```rust
pub fn load_skill_with_dir(&self, name: &str) -> Result<(String, PathBuf), SkillDiscoveryError> {
    let entry = self.skills.iter()
        .find(|e| e.metadata.name == name)
        .ok_or_else(|| SkillDiscoveryError::NotFound(name.to_string()))?;

    let base_path = entry.base_path.clone();

    // 优先使用内嵌内容
    let content = match &entry.embedded_content {
        Some(c) => c.clone(),
        None => std::fs::read_to_string(&entry.skill_file)
            .map_err(|source| SkillDiscoveryError::ReadFailed {
                path: entry.skill_file.clone(),
                source,
            })?,
    };

    let (_, body) = parse_skill_frontmatter(&content);
    // ... 现有的 additional resources 逻辑（对 builtin 跳过）
    let mut out = body;

    if entry.embedded_content.is_none()
        && entry.skill_file.file_name().map(|f| f == SKILL_MD).unwrap_or(false)
    {
        // ... 现有的 additional resources 扫描
    }

    Ok((out, base_path))
}
```

#### SkillRegistry 新增 add_builtin 方法

```rust
impl SkillRegistry {
    /// 注册一个内置 skill。如果同名 skill 已存在（来自磁盘），则跳过（用户优先）。
    pub fn add_builtin(
        &mut self,
        name: &str,
        description: &str,
        content: &str,
        triggers: Vec<String>,
        requires_tools: Vec<String>,
    ) {
        if self.skills.iter().any(|e| e.metadata.name == name) {
            return;
        }

        let metadata = SkillMetadata {
            name: name.to_string(),
            description: description.to_string(),
            triggers,
            metadata: if requires_tools.is_empty() {
                None
            } else {
                Some(SkillMetadataBlock {
                    conditions: SkillConditions {
                        requires_tools,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            },
            ..Default::default()
        };

        self.skills.push(SkillEntry {
            metadata,
            base_path: PathBuf::new(),
            skill_file: PathBuf::new(),
            source: SkillSource::Builtin,
            embedded_content: Some(content.to_string()),
        });
    }
}
```

### 3.3 Tool 层改动（`agent/tool/tool-core/`）

#### BuiltinSkill 结构

```rust
/// Tool 可选地向 skill registry 贡献的内置 skill。
pub struct BuiltinSkill {
    pub name: String,
    pub description: String,
    /// SKILL.md 全文（含 frontmatter）
    pub content: String,
    pub triggers: Vec<String>,
    /// 依赖的工具名；skill registry 会按 toolset filter 过滤
    pub requires_tools: Vec<String>,
}
```

#### Tool trait 新增默认方法

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn spec(&self) -> ToolSpec;
    async fn call(&self, args: Value, ctx: Option<&ToolCallContext>) -> Result<ToolCallContent, ToolSourceError>;

    /// Tool 可选地向 skill registry 贡献内置 skill。
    /// 默认返回 None。覆盖此方法以提供工具的详细使用文档。
    fn builtin_skill(&self) -> Option<BuiltinSkill> { None }
}
```

### 3.4 Agent 初始化改动

在 `SkillRegistry::discover()` 完成后，遍历所有已注册 tool，注入 builtin skill：

```rust
// 伪代码 — 实际位置取决于 agent 初始化流程
for tool in &tools {
    if let Some(skill) = tool.builtin_skill() {
        registry.add_builtin(
            &skill.name,
            &skill.description,
            &skill.content,
            skill.triggers,
            skill.requires_tools,
        );
    }
}
```

注入时机在 `discover()` 之后、`apply_filters()` 之前，确保：
1. 磁盘发现的用户 skill 优先（`add_builtin` 内部去重）
2. `apply_toolset_filters` 能正确过滤（`requires_tools` 生效）

### 3.5 优先级规则

```
磁盘发现（Project > ProfileDir > User > Agent > Data）
    ↓ 去重（先到先得）
Builtin skill（仅当磁盘无同名时注入）
    ↓
apply_filters / apply_toolset_filters
```

用户可以在 `.loom/skills/luft-workflow-dsl/SKILL.md` 覆盖内置版本。

---

## 4. LuftTool 实现（首个用例）

### 4.1 文件组织

```
agent/tool/tool-luft/src/
├── tool.rs                      ← Tool::builtin_skill() 实现
├── luft_workflow_dsl.md         ← 新增：DSL 参考文档（SKILL.md 格式）
└── ...
```

### 4.2 builtin_skill 实现

```rust
const LUFT_WORKFLOW_DSL: &str = include_str!("luft_workflow_dsl.md");

impl Tool for LuftTool {
    // ... name / spec / call 不变

    fn builtin_skill(&self) -> Option<BuiltinSkill> {
        Some(BuiltinSkill {
            name: "luft-workflow-dsl".to_string(),
            description: "Lua DSL reference for writing Luft multi-agent workflows".to_string(),
            content: LUFT_WORKFLOW_DSL.to_string(),
            triggers: vec![
                "luft".to_string(),
                "workflow".to_string(),
                "multi-agent".to_string(),
                "lua script".to_string(),
            ],
            requires_tools: vec!["luft".to_string()],
        })
    }
}
```

### 4.3 Skill 内容设计（`luft_workflow_dsl.md`）

**定位**：参考文档，不是 system prompt。agent 在需要写 luft workflow 脚本时通过 `skill_view("luft-workflow-dsl")` 加载。

**内容大纲**（~6KB / ~1.5K tokens）：

| 章节 | 内容 | 行数估计 |
|---|---|---|
| Execution Model | sandbox 约束、orchestrator-only 语义 | ~8 行 |
| Required Structure | meta 表、function main()、report() 三件套 | ~15 行 |
| Primitives | 每个 primitive 签名 + 行为 + 使用场景 | ~60 行 |
| - agent(opts) | opts 全字段、result 结构、schema 规则 | ~20 行 |
| - parallel / pipeline | 签名 + 语义 + 区别 | ~15 行 |
| - phase / workflow / report / log / budget / json | 签名 | ~15 行 |
| Globals | args、ctx.run_id | ~3 行 |
| Rules | 9 条浓缩规则 | ~15 行 |
| Example | 1 个精简完整示例（analyze → report） | ~25 行 |

**vs luft 原始 `lua_dsl_reference.md`（27KB）的差异**：

| 删除的内容 | 原因 |
|---|---|
| Planner 语气（"You are the orchestration planner..."） | 不是 system prompt |
| Architecture Header（44 dashes + 箭头图规则） | planner 专用，手动写脚本时不需要 |
| Task Decomposition 完整方法论 | 浓缩为 1 条 rule |
| Adversarial Verification 完整示例（90 行） | pattern 名提及即可 |
| 3 个完整 refactoring 示例 | 保留 1 个精简版 |
| Agent Prompt Quality BAD/GOOD 对比 | 浓缩为 agent() 规则 |
| Mock generation 指令 | planner 专用 |

### 4.4 Tool description 同步

tool description 保持上一轮已修复的精简签名版，末尾增加一句引导：

```
... json.decode(string).
For full DSL reference and examples, load skill 'luft-workflow-dsl'.
```

---

## 5. 影响评估

### 5.1 改动范围

| 层 | 文件 | 代码量 | 说明 |
|---|---|---|---|
| skill | `agent/skill/src/discovery.rs` | ~40 行 | SkillSource + Builtin, SkillEntry + embedded_content, add_builtin, load 逻辑 |
| tool-core | `Tool` trait 定义文件 | ~15 行 | BuiltinSkill struct + trait 默认方法 |
| agent init | 注册 tools 后的初始化代码 | ~8 行 | 遍历 tools 注入 builtin skill |
| tool-luft | `tool.rs` + `luft_workflow_dsl.md` | ~15 行代码 + 1 md | 首个 builtin_skill 实现 |

### 5.2 兼容性

- **零破坏**：`Tool::builtin_skill()` 默认返回 `None`，所有现有 tool 不受影响
- **零磁盘依赖**：内置 skill 通过 `include_str!` 嵌入二进制
- **用户可覆盖**：磁盘 skill 优先于 builtin
- **现有测试不受影响**：`embedded_content` 默认 `None`，`add_builtin` 是新增方法

### 5.3 Token 预算

| 场景 | 注入内容 | Token 开销 |
|---|---|---|
| Agent 看到 tool 列表 | tool description (~400 tokens) | 固定 |
| Agent 加载 luft skill | `luft_workflow_dsl.md` (~1.5K tokens) | 按需 |
| Agent 不使用 luft | 零额外开销 | — |

### 5.4 后续扩展

其他复杂 tool 可按同样模式贡献 builtin skill：

| Tool | 潜在 skill | 场景 |
|---|---|---|
| `git` | git-workflow | 复杂 git 操作流程（rebase、cherry-pick） |
| `websearch` | search-strategies | 高级搜索语法、domain filter 技巧 |
| `agent` | delegation-patterns | 子 agent 委派模式 |

---

## 6. 实施步骤

1. **skill 层**：`discovery.rs` — SkillSource + Builtin, SkillEntry + embedded_content, add_builtin, load 逻辑调整
2. **tool-core 层**：定义 `BuiltinSkill` struct，`Tool` trait 加 `builtin_skill()` 默认方法
3. **agent 初始化**：找到 tool 注册后的 skill registry 注入点，加遍历逻辑
4. **tool-luft**：编写 `luft_workflow_dsl.md`，实现 `builtin_skill()`
5. **tool description**：末尾加 skill 引导语
6. **测试**：discovery.rs 单元测试（builtin 注入、去重、load embedded）；tool-luft 集成验证
