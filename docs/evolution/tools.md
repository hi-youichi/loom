# ReviewToolExecutor — 后台审查工具白名单

> 源文件: `loom/src/background_review/tools.rs`
> 自 review.md 引用，定义 Review Agent 可使用的全部工具和输入规范

## 概述

`ReviewToolExecutor` 是 Review Agent 的**唯一工具入口**，实现**白名单机制**：只有已注册的 10 个工具可被调用，其他工具名返回错误。所有内存和技能操作通过此结构体代理，确保安全校验始终启用。

## 核心类型

### ReviewToolExecutor

```rust
pub struct ReviewToolExecutor<'a> {
    pub memory: &'a MemoryStore,       // MemoryStore 引用
    pub skills: &'a SkillRegistry,      // SkillRegistry 引用
    pub curator: Option<&'a Curator>,   // 可选 Curator (skill_view 时 touch)
    pub actions: Vec<ReviewAction>,     // 累积的操作记录
}
```

### ReviewAction

每次修改操作成功后推入 `actions`，记录操作类型、目标和摘要：

```rust
pub struct ReviewAction {
    pub kind: String,           // "memory" | "skill" | "skill_file"
    pub target: String,         // 文件名或技能名
    pub summary: String,        // 操作摘要
    pub has_modification: bool, // 默认 true
}
```

## 工具列表

### 记忆工具

| 工具 | 参数 | 功能 | 安全机制 |
|------|------|------|----------|
| `memory_get` | `file`: "USER" / "PROJECT" / "FACTS" | 读取记忆文件 | 仅允许三个已知文件名 |
| `memory_set` | `file`, `action`: "append" / "replace", `content` | 写入记忆文件 | 去重、文件大小上限、替换收缩比例检查 |

**memory_set 安全规则**：
- **去重**: append 时如果内容已存在则跳过 (`"Content already exists"`)
- **大小上限**: `MAX_MEMORY_FILE_SIZE = 64 KB`，超过拒绝
- **替换保护**: replace 时如果新内容不到旧内容的 30% (`REPLACE_SHRINK_RATIO = 0.3`)，拒绝并建议使用 `skill_patch`

### 技能工具

| 工具 | 参数 | 功能 | 安全机制 |
|------|------|------|----------|
| `skills_list` | (无) | 列出所有技能 (name, desc, lifecycle, source, triggers) | — |
| `skill_view` | `name` | 查看技能详情 (含 body) | 自动调用 `curator.touch_skill()` 更新使用时间 |
| `skill_create` | `name`, `description`, `triggers[]`, `body` | 创建新技能 (Source: Auto) | `validate_skill_create()` 检查危险模式/注入 |
| `skill_edit` | `name`, `content` | 全量替换技能 body | 拒绝空内容，拒绝后 `validate_skill_create()` |
| `skill_patch` | `name`, `old_string`, `new_string` | 精确查找替换 | patch 后 `validate_skill_create()`；失败自动回滚 |

**skill_patch 原子性**: 先应用 patch，再验证。验证失败时自动回滚 (`patch(name, new_string, old_string)`)，保证技能文件不会处于非法状态。

### 文件工具

| 工具 | 参数 | 功能 | 安全机制 |
|------|------|------|----------|
| `skill_write_file` | `name`, `path`, `content` | 在技能目录下添加 support file | `validate_skill_path()` 检查路径遍历 |
| `skill_remove_file` | `name`, `path` | 删除技能 support file | — |

**support file 路径示例**: `references/api-docs.md`, `templates/rust-mod.rs`, `scripts/deploy.sh`

## 工具规范导出

```rust
pub fn review_tool_specs() -> Vec<ToolSpec>
```

返回 10 个 `ToolSpec`，包含 JSON Schema 输入定义和描述。用于向 LLM Provider 注册 Review Agent 可用工具。

## 安全层

所有修改操作经过两级校验：

1. **创建时**: `validate_skill_create()` — 检查危险模式 (`rm -rf`, `exec(`, `eval(`) 和注入模式 (`ignore previous instructions`, `jailbreak`)
2. **路径时**: `validate_skill_path()` — 检查路径遍历攻击 (`../`)

校验失败返回 `{"success": false, "error": "Validation failed: ..."}`，阻止保存。

## 并发安全

- `ReviewToolExecutor` 是 `&mut self` 方法调用，单线程执行
- 通过 `MemoryStore` 和 `SkillRegistry` 引用共享底层存储
- Review Agent 运行在独立 `tokio::task::spawn` 中，与主对话隔离
