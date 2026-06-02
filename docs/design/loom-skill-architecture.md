# Loom Skill Crate 设计文档

## 概述

`loom-skill` 是一个独立的 Rust crate，提供 skill 的核心功能：发现、存储、使用追踪。它不依赖 `loom` 运行时，可以独立测试和复用。

## 架构图

```
┌─────────────────────────────────────────────────────────────┐
│                         loom (运行时)                        │
│  - 依赖 loom-skill                                          │
│  - 提供 SkillTool（工具实现）                                │
│  - 提供 background_review（后台维护）                        │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ 依赖
                              │
┌─────────────────────────────────────────────────────────────┐
│                    loom-skill (核心)                        │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │  discovery  │  │   storage   │  │    usage    │          │
│  │ SkillRegistry│  │SkillStorage │  │SkillUsage   │          │
│  │   (扫描)    │  │ Registry   │  │   Store     │          │
│  │             │  │   (CRUD)   │  │  (统计)     │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│                         │                                   │
│                    ┌─────────┐                              │
│                    │  utils  │                              │
│                    │ frontmatter                            │
│                    │   解析    │                              │
│                    └─────────┘                              │
└─────────────────────────────────────────────────────────────┘
                              ▲
                              │ 可选依赖
                              │
┌─────────────────────────────────────────────────────────────┐
│                     config (外部)                            │
│               env_config::home::loom_home()                  │
└─────────────────────────────────────────────────────────────┘
```

## 模块设计

### 1. utils.rs — 工具函数

提供 frontmatter 解析和平台匹配功能。

#### 核心函数

| 函数 | 说明 |
|------|------|
| `parse_frontmatter(content: &str)` | 解析 YAML frontmatter，返回 `(yaml_mapping, body)` |
| `split_frontmatter(content: &str)` | 分割 YAML 块和正文，返回 `(&yaml, &body)` |
| `parse_skill_frontmatter(content: &str)` | 解析 skill 文件，返回 `(Option<SkillMetadata>, body)` |
| `is_excluded_path(path: &Path)` | 检查路径是否在排除目录列表中 |

#### SkillMetadata 结构

```rust
pub struct SkillMetadata {
    pub name: String,
    pub version: Option<String>,      // 语义版本
    pub description: String,
    pub platforms: Vec<String>,      // 平台要求
    pub tags: Vec<String>,           // 标签
}
```

#### 平台匹配

```rust
impl SkillMetadata {
    pub fn matches_platform(&self, current_platform: &str) -> bool {
        // darwin ↔ macos
        // 空 platforms 匹配所有平台
    }
}
```

### 2. discovery.rs — SkillRegistry

扫描和加载 skill 文件。

#### SkillSource 枚举

```rust
pub enum SkillSource {
    Project,      // .loom/skills
    User,         // ~/.loom/skills
    ProfileDir,   // 配置的额外目录
    Agent,        // Agent 目录
    Data,         // ~/.loom/data/skills
}
```

#### SkillEntry 结构

```rust
pub struct SkillEntry {
    pub metadata: SkillMetadata,     // frontmatter 元数据
    pub base_path: PathBuf,          // skill 目录
    pub skill_file: PathBuf,         // SKILL.md 路径
    pub source: SkillSource,         // 来源
}
```

#### SkillRegistry 方法

```rust
impl SkillRegistry {
    // 从多个位置发现 skills
    pub fn discover(working_folder: &Path, extra_dirs: &[PathBuf]) 
        -> Result<Self, SkillDiscoveryError>
    
    // 添加 agent 特定目录
    pub fn add_agent_skills(&mut self, dir: &Path) -> Result<(), SkillDiscoveryError>
    
    // 应用启用/禁用过滤器
    pub fn apply_filters(&mut self, enabled: Option<&[String]>, disabled: Option<&[String]>)
    
    // 生成 <available_skills> prompt 块
    pub fn available_skills_prompt(&self) -> String
    
    // 加载 skill 内容
    pub fn load_skill(&self, name: &str) -> Result<String, SkillDiscoveryError>
    
    // 列表和查找
    pub fn list(&self) -> &[SkillEntry]
    pub fn find(&self, name: &str) -> Option<&SkillEntry>
    
    // 测试用
    pub fn empty() -> Self
    pub fn from_entries(skills: Vec<SkillEntry>) -> Self
}
```

#### 发现优先级

1. Project `.loom/skills` — 最高优先级
2. ProfileDir 额外目录
3. User `~/.loom/skills`
4. Data `~/.loom/data/skills/` — 递归扫描

### 3. storage.rs — SkillStorageRegistry

持久化存储管理。

#### Lifecycle 枚举

```rust
pub enum Lifecycle {
    Active,    // 活跃
    Stale,     // 久未使用
    Archived,  // 已归档
}
```

#### Source 枚举

```rust
pub enum Source {
    Auto,     // 自动生成（后台 review）
    Manual,   // 手动创建
    Evolved,  // 演化生成
}
```

#### SkillContent 结构

```rust
pub struct SkillContent {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,       // 触发词
    pub lifecycle: Lifecycle,
    pub source: Source,
    pub body: String,                // 正文
    pub raw: String,                 // 原始内容（含 frontmatter）
}
```

#### 目录结构

```
~/.loom/data/skills/
├── auto/           # 自动生成的 skill
│   ├── skill-name-1/
│   │   └── SKILL.md
│   └── skill-name-2/
├── curated/        # 手动创建的 skill
│   └── manual-skill/
│       └── SKILL.md
└── evolved/        # 演化生成的 skill
    └── evolved-skill/
        └── SKILL.md
```

#### SkillStorageRegistry 方法

```rust
impl SkillStorageRegistry {
    pub fn new(base_dir: &Path) -> Self
    pub fn base_dir(&self) -> &Path
    
    pub fn list(&self) -> Result<Vec<SkillMeta>, SkillError>  // 列出所有
    pub fn load(&self, name: &str) -> Result<SkillContent, SkillError>  // 加载
    pub fn save(&self, name: &str, content: &SkillContent) -> Result<(), SkillError>  // 保存
    pub fn delete(&self, name: &str) -> Result<(), SkillError>  // 删除
    pub fn patch(&self, name: &str, old: &str, new: &str) -> Result<(), SkillError>  // 文本替换
    
    pub fn write_file(&self, skill_name: &str, path: &str, content: &str) -> Result<...>
    pub fn remove_file(&self, skill_name: &str, path: &str) -> Result<...>
    
    pub fn find_matching(&self, query: &str, threshold: f64) -> Result<Vec<SkillContent>, ...>
}
```

### 4. usage.rs — SkillUsageStore

使用统计和生命周期追踪。

#### SkillUsage 结构

```rust
pub struct SkillUsage {
    pub name: String,
    pub use_count: u64,         // 使用次数
    pub view_count: u64,        // 查看次数
    pub patch_count: u64,       // 修改次数
    pub last_used_at: Option<String>,
    pub last_viewed_at: Option<String>,
    pub last_patched_at: Option<String>,
    pub created_at: String,
    pub created_by: Option<String>,
    pub state: Lifecycle,
    pub pinned: bool,
    pub archived_at: Option<String>,
}
```

#### SkillUsageStore 方法

```rust
impl SkillUsageStore {
    pub fn new(base_dir: &Path) -> Self
    
    // 计数更新
    pub fn bump_use(&self, name: &str)
    pub fn bump_view(&self, name: &str)
    pub fn bump_patch(&self, name: &str)
    
    // 状态更新
    pub fn mark_agent_created(&self, name: &str)
    pub fn set_state(&self, name: &str, state: Lifecycle)
    pub fn set_pinned(&self, name: &str, pinned: bool)
    
    // 查询
    pub fn get(&self, name: &str) -> Option<SkillUsage>
    pub fn agent_created_report(&self) -> Result<Vec<SkillUsageReport>, String>
    
    // 持久化
    pub fn load(&self) -> Result<HashMap<String, SkillUsage>, std::io::Error>
    pub fn save(&self, data: &HashMap<String, SkillUsage>) -> Result<(), std::io::Error>
}
```

#### 存储格式

```json
// ~/.loom/.usage.json
{
  "skill-name": {
    "name": "skill-name",
    "use_count": 42,
    "view_count": 100,
    "patch_count": 3,
    "last_used_at": "2025-01-15T10:30:00Z",
    "created_at": "2025-01-01T00:00:00Z",
    "created_by": "agent",
    "state": "active",
    "pinned": false
  }
}
```

## 与 Hermes 对齐

### Hermes 映射

| Hermes | Loom Skill | 说明 |
|--------|------------|------|
| `skill_utils.py` | `utils.rs` | Frontmatter 解析、平台匹配 |
| `skill_bundles.py` | 待实现 | Skill 组合命令 |
| `skills_tool.py` | `loom/tools/skill.rs` | 工具实现（不在此 crate） |
| `skill_usage.py` | `usage.rs` | 使用追踪 |
| `curator.py` | `loom-curator` crate | 后台维护 |
| `background_review.py` | `loom-curator` | Review prompts |

### 关键差异

1. **Storage Registry vs Discovery Registry** — Loom 有两个独立的 Registry：
   - `discovery.rs` — 扫描发现（运行时）
   - `storage.rs` — 持久化存储（管理）

2. **Skill Source 分类** — Hermes 只有 auto/manual，Loom 新增 evolved

3. **Usage Store** — Hermes 使用 sidecar 文件，Loom 同样

## 使用示例

### 发现 Skill

```rust
use loom_skill::discovery::SkillRegistry;

let registry = SkillRegistry::discover(
    std::path::Path::new("/project"),
    &[],
)?;

println!("{}", registry.available_skills_prompt());

if let Some(entry) = registry.find("code-review") {
    let content = registry.load_skill("code-review")?;
}
```

### 存储 Skill

```rust
use loom_skill::storage::{SkillStorageRegistry, SkillContent, Lifecycle, Source};

let storage = SkillStorageRegistry::new(
    std::path::Path::new("/home/user/.loom/data/skills")
);

let skill = SkillContent {
    name: "rust-debug".to_string(),
    description: "Debug Rust errors".to_string(),
    triggers: vec!["rust".into(), "cargo".into()],
    lifecycle: Lifecycle::Active,
    source: Source::Auto,
    body: "# Debug Steps\n1. Read error...".to_string(),
    raw: String::new(),
};

storage.save("rust-debug", &skill)?;
let loaded = storage.load("rust-debug")?;
```

### 使用统计

```rust
use loom_skill::usage::SkillUsageStore;

let store = SkillUsageStore::new(
    std::path::Path::new("/home/user/.loom")
);

store.bump_use("rust-debug");
store.bump_view("rust-debug");

if let Some(usage) = store.get("rust-debug") {
    println!("Used {} times", usage.use_count);
}

let report = store.agent_created_report()?;
```

## 依赖

```toml
# loom-skill/Cargo.toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
thiserror = { workspace = true }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tokio = { workspace = true }
env_config = { path = "../config", package = "config" }

[dev-dependencies]
tempfile = "3"
```

## 下一步

1. **loom-llm** — 抽取 LLM 客户端抽象和实现
2. **loom-curator** — 抽取 Curator 后台维护逻辑
3. **bundles.rs** — 实现 Skill Bundle 功能（对齐 Hermes）