# Background Review 详细通知方案

**创建时间**：2025-08-19  
**分支**：`acp/review-detailed-notify`  
**Commit**：待定

---

## 1. 背景与问题

当前背景review完成后，通知消息过于简略：
```
Background review saved 6 skills (135.5s).
```

**问题**：
- 用户无法快速了解具体修改了哪些技能/记忆
- 不知道每个技能的变更规模（字符数）
- 需要手动查看SQLite历史记录才能获取详细信息

## 2. 目标

让背景review的完成通知提供**详细的多行信息**：

- ✅ 在chat流中清晰显示所有修改内容
- ✅ 每个技能/记忆显示名称和变更字符数  
- ✅ 保持时间信息
- ✅ 支持分类显示（技能 vs 记忆）
- ✅ 向后兼容现有ACP协议

## 3. 现状分析

**数据源**：
```rust
// experimental/curator/src/review.rs:54-63
pub struct ReviewActionSummary {
    pub kind: String,        // "skill_create", "skill_update", "memory_create"...
    pub target: String,      // 技能名称或记忆ID
    pub summary: String,     // "Skill 'xxx' created (+1234 chars)" 这样的详细信息
    pub succeeded: bool,     // 是否成功
}

pub struct ReviewOutcome {
    pub actions: Vec<ReviewActionSummary>,  // 所有操作的详细列表
    pub memory_count: usize,
    pub skill_count: usize,
    pub duration_ms: u64,
    // ... 其他字段
}
```

**当前通知函数**：
```rust
// apps/acp/src/review_runner.rs:296-326
fn build_summary_line(outcome: &ReviewOutcome) -> String {
    // 当前只生成单行摘要
    format!("Background review saved {} ({:.1}s).", joined, secs)
}
```

## 4. 改进方案：多行详细格式

### 4.1 目标输出格式

**成功场景**：
```
Background review completed (135.5s):
  📝 2 memories saved:
     • Memory "debug-logging" created (345 chars)
     • Memory "api-patterns" updated (+120 chars)
  🔧 6 skills updated:
     • Skill "react-testing" created (+1,234 chars)
     • Skill "rust-async" updated (+567 chars)
     • Skill "typescript-patterns" created (+890 chars)
     • Skill "docker-setup" updated (+234 chars)
     • Skill "ci-cd-pipeline" created (+456 chars)
     • Skill "monitoring" updated (+123 chars)
```

**跳过场景**：
```
Background review skipped (session_too_short).
```

**无操作场景**：
```
Background review: nothing to save (0.8s).
```

### 4.2 设计要点

1. **分类显示**：记忆和技能分区块显示
2. **表情符号**：使用 📝 和 🔧 增强可读性
3. **缩进层级**：2级缩进形成视觉层次
4. **字符数格式化**：使用千位分隔符 (1,234 chars)
5. **ACP协议兼容**：多行文本在 AgentMessageChunk 中正常工作
6. **错误处理**：只有成功的操作才会显示

## 5. 实现细节

### 5.1 核心函数改进

**文件**: `apps/acp/src/review_runner.rs:296-326`

**修改前**:
```rust
fn build_summary_line(outcome: &ReviewOutcome) -> String {
    let secs = outcome.duration_ms as f64 / 1000.0;
    if outcome.skipped {
        let reason = outcome.skip_reason.as_deref().unwrap_or("skipped");
        return format!("Background review skipped ({}).", reason);
    }
    
    let parts: Vec<String> = [
        (outcome.memory_count > 0).then(|| {
            if outcome.memory_count == 1 { "1 memory".to_string() }
            else { format!("{} memories", outcome.memory_count) }
        }),
        (outcome.skill_count > 0).then(|| {
            if outcome.skill_count == 1 { "1 skill".to_string() }
            else { format!("{} skills", outcome.skill_count) }
        }),
    ].into_iter().flatten().collect();
    
    if parts.is_empty() {
        return format!("Background review: nothing to save ({:.1}s).", secs);
    }
    
    let joined = parts.join(" + ");
    format!("Background review saved {} ({:.1}s).", joined, secs)
}
```

**修改后**:
```rust
fn build_summary_line(outcome: &ReviewOutcome) -> String {
    let secs = outcome.duration_ms as f64 / 1000.0;
    
    // 跳过场景保持简洁
    if outcome.skipped {
        let reason = outcome.skip_reason.as_deref().unwrap_or("skipped");
        return format!("Background review skipped ({}).", reason);
    }
    
    // 主标题行
    let mut lines = vec![format!("Background review completed ({:.1}s):", secs)];
    
    // 按类型分组操作
    let memory_actions: Vec<_> = outcome.actions.iter()
        .filter(|a| a.kind.contains("memory") && a.succeeded)
        .collect();
    let skill_actions: Vec<_> = outcome.actions.iter()
        .filter(|a| a.kind.contains("skill") && a.succeeded)
        .collect();
    
    // 记忆操作详情
    if !memory_actions.is_empty() {
        lines.push(format!("  📝 {} memories saved:", memory_actions.len()));
        for action in memory_actions {
            let action_summary = format_action_summary(action);
            lines.push(format!("     • {}", action_summary));
        }
    }
    
    // 技能操作详情
    if !skill_actions.is_empty() {
        lines.push(format!("  🔧 {} skills updated:", skill_actions.len()));
        for action in skill_actions {
            let action_summary = format_action_summary(action);
            lines.push(format!("     • {}", action_summary));
        }
    }
    
    // 无操作场景
    if memory_actions.is_empty() && skill_actions.is_empty() {
        lines.push("  • nothing to save".to_string());
    }
    
    lines.join("\n")
}

/// 格式化单个操作的详细信息
fn format_action_summary(action: &ReviewActionSummary) -> String {
    // summary 已经包含了创建/更新信息和字符数
    // 例如: "Skill 'react-testing' created (+1,234 chars)"
    action.summary.clone()
}
```

### 5.2 测试更新

**文件**: `apps/acp/src/review_runner.rs:418-472`

需要更新现有测试以适应新格式：

```rust
#[test]
fn summary_line_for_reviewed_with_both_kinds() {
    let outcome = ReviewOutcome {
        actions: vec![
            ReviewActionSummary {
                kind: "memory_create".to_string(),
                target: "debug-logging".to_string(),
                summary: "Memory 'debug-logging' created (345 chars)".to_string(),
                succeeded: true,
            },
            ReviewActionSummary {
                kind: "skill_update".to_string(),
                target: "react-testing".to_string(),
                summary: "Skill 'react-testing' updated (+567 chars)".to_string(),
                succeeded: true,
            },
        ],
        memory_count: 1,
        skill_count: 1,
        duration_ms: 1200,
        skipped: false,
        skip_reason: None,
        tokens: Default::default(),
        // ... 其他字段
    };
    let s = build_summary_line(&outcome);
    assert!(s.contains("Background review completed (1.2s):"));
    assert!(s.contains("📝 1 memories saved:"));
    assert!(s.contains("Memory 'debug-logging' created (345 chars)"));
    assert!(s.contains("🔧 1 skills updated:"));
    assert!(s.contains("Skill 'react-testing' updated (+567 chars)"));
}
```

## 6. 协议兼容性分析

| 维度 | 当前实现 | 改进后 |
|------|---------|---------|
| ACP协议 | AgentMessageChunk (单行) | AgentMessageChunk (多行) |
| 消息ID | UUID | UUID (不变) |
| Client支持 | 已支持多行消息 | 完全兼容 |
| 渲染方式 | 单行气泡 | 多行气泡 |

**ACP协议兼容性**：✅ 完全兼容
- `AgentMessageChunk.content` 支持多行文本
- 消息ID机制不变
- Zed/其他IDE客户端已支持多行消息显示

## 7. 风险评估

| 风险项 | 影响 | 缓解措施 |
|--------|------|---------|
| 超长消息 | 可能导致UI拥挤 | 限制最多显示10个操作，超出显示"+N more" |
| 字符编码 | 中文字符可能影响排版 | 使用固定宽度测试，确保各IDE正常显示 |
| 性能 | 复杂格式化可能影响性能 | 格式化操作简单，性能影响可忽略 |
| 向后兼容 | 现有客户端期望单行 | ACP协议已支持多行，无需改动 |

## 8. 未来演进

1. **可配置格式**：允许用户选择简洁/详细模式
2. **HTML格式**：支持富文本、链接等
3. **交互式UI**：点击技能名称跳转到技能详情
4. **操作分类**：按创建/更新/删除进一步分类

## 9. 验证标准

- [ ] 多行格式在Zed中正确渲染
- [ ] 表情符号显示正常
- [ ] 字符数格式化正确
- [ ] 空操作场景显示正确
- [ ] 跳过场景保持简洁
- [ ] 所有现有测试通过
- [ ] Clippy无警告
- [ ] 格式检查通过

## 10. 文件影响清单

| 文件 | 改动类型 | 行数估计 |
|------|---------|---------|
| `apps/acp/src/review_runner.rs` | 函数实现+测试更新 | +50/-15 |
| `docs/design/acp-review-detailed-notify.md` | 新建设计文档 | +250 |

---

## 附录：ReviewActionSummary.summary 字段格式

基于curator实现，summary字段遵循以下格式：

**创建操作**:
- `"Memory '{name}' created ({chars} chars)"`
- `"Skill '{name}' created (+{chars} chars)"`

**更新操作**:
- `"Memory '{name}' updated (+{chars} chars)"`
- `"Skill '{name}' updated (+{chars} chars)"`

**删除操作**:
- `"Memory '{name}' deleted (-{chars} chars)"`
- `"Skill '{name}' deleted (-{chars} chars)"`

字符数格式已由curator负责，无需额外格式化。