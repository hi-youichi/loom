# Curator — 技能定期维护

Curator 定期扫描所有技能，维护技能池的健康状态。

## 触发方式

```bash
# 只报告，不修改
loom curator --dry-run

# 实际执行
loom curator
```

## 执行流程

```
loom curator
    │
    ▼
1. 扫描 auto/ + curated/ + evolved/ 所有 SKILL.md
    │
    ▼
2. 读取 curator/state.json 中的 last_used 时间戳
    │
    ▼
3. 生命周期检测
    ├── Auto 技能 60 天未用 → 标记 stale
    ├── Manual 技能 30 天未用 → 标记 stale
    └── Stale 90 天 → 标记 archived
    │
    ▼
4. 重叠检测（Jaccard 相似度 ≥ 0.7）
    │
    ▼
5. 输出 CuratorReport
```

## CuratorReport

| 字段 | 说明 |
|------|------|
| `active` | 当前活跃技能数 |
| `stale` | 被标记为 stale 的技能名列表 |
| `archived` | 被归档的技能名列表 |
| `overlapping` | 重叠技能对（skill_a, skill_b, similarity） |

## 状态持久化

`~/.loom/data/skills/curator/state.json` 记录每个技能的 `last_used` 时间：

```json
{
  "skill_last_used": {
    "debug-rust": "2025-08-19T10:30:00+00:00",
    "deploy-guide": "2025-08-15T08:00:00+00:00"
  }
}
```

## 更新使用时间

`touch_skill(name)` 在技能被使用时调用，更新 `last_used` 为当前时间。

## 配置

```yaml
curator:
  stale_days_auto: 60       # Auto 技能 stale 阈值
  stale_days_manual: 30     # Manual 技能 stale 阈值
  archive_days: 90          # Stale → Archived 天数
  overlap_threshold: 0.7    # 重叠报告阈值
```

## 重叠检测算法

使用 Jaccard 相似度比较技能的 description + triggers 词集：

```
similarity = |A ∩ B| / |A ∪ B|
```

当 similarity ≥ `overlap_threshold`（默认 0.7）时报告重叠，但不会自动合并——需人工决定保留哪个。

## 相关文档

- [技能系统](skills.md) — 技能文件格式和生命周期
- [配置参考](config.md) — curator 配置项
- [命令参考](commands.md) — curator 命令
