# 进化子系统命令

所有与进化相关的 CLI 命令。通用命令参考见 [guide/cli.md](../guide/cli.md)。

## 技能管理

| 命令 | 说明 |
|------|------|
| `loom skills list` | 列出所有技能（含来源、生命周期状态） |
| `loom skills show <name>` | 查看技能详情（描述、triggers、正文） |
| `loom skills create <name> [--description <desc>] [--trigger <kw>]...` | 创建技能 |
| `loom skills edit <name>` | 编辑技能（打开 $EDITOR） |
| `loom skills delete <name>` | 删除技能 |
| `loom skills evolve <name> [--source synthetic] [--iterations 5] [--samples N]` | 进化指定技能 |

选项：
- `--source` — 评估数据来源：`synthetic`（默认，合成生成）、`sessiondb`（会话挖掘）
- `--iterations` — GEPA 迭代轮数，默认 5
- `--samples` — 评估样本数量

示例：

```bash
loom skills create debug-rust --description "Debug Rust compiler errors" --trigger "rust" --trigger "cargo build"
loom skills list
loom skills show debug-rust
loom skills edit debug-rust
loom skills delete debug-rust
loom skills evolve debug-rust --iterations 10 --source synthetic
```

## 进化管理

| 命令 | 说明 |
|------|------|
| `loom evolve run` | 运行所有待进化技能（需 LLM 配置） |
| `loom evolve status` | 查看最近进化历史 |
| `loom evolve compare <name>` | 对比 baseline vs evolved 内容 |
| `loom evolve accept <name>` | 接受进化结果，替换当前技能 |
| `loom evolve reject <name>` | 拒绝进化结果，保留原始技能 |
| `loom evolve backups <name>` | 列出技能的备份版本 |
| `loom evolve rollback <name> [--version <ver>]` | 回滚到指定版本 |

示例：

```bash
loom evolve status
loom evolve compare debug-rust
loom evolve accept debug-rust
loom evolve backups debug-rust
loom evolve rollback debug-rust --version 20250615_103000
```

## 技能生命周期（Curator）

| 命令 | 说明 |
|------|------|
| `loom curator [--dry-run]` | 运行 Curator，检测 stale/overlapping 技能 |

选项：
- `--dry-run` — 只报告，不修改

示例：

```bash
loom curator --dry-run
loom curator
```

Curator 检测规则：
- Auto 技能 60 天未用 → 标记 stale
- Manual 技能 30 天未用 → 标记 stale
- Stale 技能 90 天 → 归档
- 相似度 ≥ 0.7 的技能对 → 报告重叠

## 记忆管理

| 命令 | 说明 |
|------|------|
| `loom memory show` | 显示所有记忆文件（USER/PROJECT/FACTS） |
| `loom memory edit <file>` | 编辑记忆文件（打开 $EDITOR） |
| `loom memory search <query>` | 搜索记忆中的关键词 |

file 参数：`USER`、`PROJECT` 或 `FACTS`

示例：

```bash
loom memory show
loom memory edit USER
loom memory search "rust"
```

## JSON 输出

所有命令均支持 `--json` / `-j` 标志输出 JSON 格式：

```bash
loom skills list --json
loom evolve status --json
loom curator --dry-run --json
loom memory search "cargo" --json
```

## 数据目录结构

```
~/.loom/data/
├── memory/                    # 记忆文件
│   ├── USER.md
│   ├── PROJECT.md
│   └── FACTS.md
├── skills/                    # 技能文件
│   ├── auto/                  # 自动生成的技能
│   │   └── <name>/SKILL.md
│   ├── curated/               # 手动创建的技能
│   │   └── <name>/SKILL.md
│   ├── evolved/               # 进化产生的技能
│   │   └── <name>/SKILL.md
│   └── curator/state.json     # Curator 状态
└── evolution/                 # 进化数据
    ├── datasets/              # 评估数据集
    │   └── <skill>/
    │       ├── train.jsonl
    │       └── holdout.jsonl
    ├── runs/                  # 进化运行记录
    │   └── <skill>/
    │       └── <timestamp>/
    │           └── metrics.json
    └── backups/               # 技能备份
        └── <skill>/
            └── <timestamp>.md
```
