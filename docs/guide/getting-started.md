# 快速开始

## 安装

```bash
# 从源码编译（需要 Rust 1.75+）
cargo install --git https://github.com/loom/loom

# 或下载预编译二进制
curl -fsSL https://loom.dev/install.sh | bash
```

## 初始化

```bash
$ loom init

? 选择底层 CLI:
  ❯ Loom
    Codex (OpenAI)

✓ Created ~/.loom/data/memory/
✓ Created ~/.loom/data/skills/auto/
✓ Created ~/.loom/data/sessions/
✓ Created ~/.loom/config.yaml
✓ Backend: loom (context file: CLAUDE.md)
```

## 第一次会话

```bash
$ loom chat
[loom] Injected memory context (0 facts, 0 skills)
[loom] Starting loom chat...
> 帮我配置这个 Rust 项目的 CI，用 GitHub Actions，跑测试+clippy+fmt

... (Loom 完成任务，12 个工具调用) ...

[loom] Session saved (47 messages)
[loom] Background review...
  → Created skill: rust-ci-setup
  → Saved preference: "用户偏好 GitHub Actions，用 rust-toolchain.toml"
```

## 第二次会话（自动加载记忆和技能）

```bash
$ loom chat
[loom] Injected: 1 fact, 1 skill (rust-ci-setup)
[loom] Starting loom chat...
> 给另一个项目也配上 CI
  (直接使用 rust-ci-setup 技能，无需重复指导)
```

## 切换底层 CLI

```bash
$ loom config set cli.backend codex
[loom] Backend switched to codex (context file: AGENTS.md)

$ loom chat
[loom] Starting codex --quiet...
```

## 常用命令

```bash
loom skills list                   # 查看所有技能
loom memory show                   # 查看记忆内容
loom sessions search "clippy"      # 搜索历史对话
loom curator run                   # 手动触发技能维护
```

## 数据在哪

所有数据在 `~/.loom/data/`，纯文件系统：

```
~/.loom/data/
├── memory/          # 记忆文件（Markdown）
├── skills/auto/     # 自动创建的技能
├── sessions/        # 会话记录 + SQLite 搜索索引
├── evolution/       # GEPA 优化记录
└── config.yaml      # 全局配置
```

可以直接用编辑器打开任何文件查看或手动修改。

## 下一步

- [配置参考](config.md) — 自定义模型、阈值、调度
- [Backend 指南](backends.md) — Loom vs Codex 详细对比
- [技能系统](../evolution/skills.md) — 理解技能如何工作
