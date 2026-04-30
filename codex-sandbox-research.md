# Codex 沙箱技术研究文档

## 1. 概述

OpenAI Codex 的沙箱（Sandbox）是其最核心的安全特性之一。它不是基于提示词或模型自律的软约束，而是 **OS 内核级强制隔离**——操作系统直接阻止违规操作，无论模型生成什么代码都无法绕过。

Codex 沙箱遵循 **纵深防御（Defense-in-Depth）** 原则，在所有命令执行路径上施加限制：

- 文件系统访问：默认只读，可配置写入白名单
- 网络访问：默认关闭，可选限制或代理路由
- 进程能力：降低权限，过滤系统调用
- 受保护路径：`.git`、`.codex` 始终只读

---

## 2. 架构总览

Codex 沙箱采用 **策略驱动 + 平台特定实现** 的分层架构：

```
┌─────────────────────────────────────────────┐
│              策略层 (Policy)                 │
│  sandbox_mode + network_policy + rules      │
├─────────────────────────────────────────────┤
│           执行管道 (Pipeline)                │
│  命令 → 策略匹配 → 平台沙箱包装 → 执行 → 结果分类  │
├─────────────────────────────────────────────┤
│          平台实现 (Platform)                 │
│  macOS Seatbelt │ Linux Landlock+seccomp │ Windows AppContainer │
└─────────────────────────────────────────────┘
```

**核心流程**：

1. 根据策略选择 `SandboxType`
2. 通过 `SandboxManager` 将命令转换为平台特定的沙箱包装
3. 执行并分类结果（包括沙箱拒绝检测）

源码关键路径：
- 核心编排：`codex-rs/core/src/codex.rs`
- 沙箱调度：`codex-rs/core/src/sandboxing/mod.rs`
- 执行入口：`codex-rs/core/src/exec.rs`
- 进程启动：`codex-rs/core/src/spawn.rs`
- Linux 沙箱 crate：`codex-linux-sandbox`

---

## 3. 沙箱模式

Codex 提供三级沙箱严格度：

### 3.1 `read-only`

- 文件系统：全部只读
- 网络：关闭
- 用途：代码审查、问答、不信任目录的默认模式

### 3.2 `workspace-write`（默认推荐）

- 文件系统：工作区内可写，外部只读
- 网络：默认关闭，可配置开启
- 受保护路径（`.git`、`.codex`）即使在工作区内也保持只读
- `--full-auto` 等价于此模式 + `--ask-for-approval on-request`

### 3.3 `external-sandbox`

- Codex 自身不施加沙箱，假定调用方已在外部提供隔离
- 仍会向工具和 MCP 服务器传递网络访问状态
- 适用于已在容器或 VM 中运行的场景

### 3.4 `danger-full-access`

- 无任何沙箱限制
- 等价于 `--dangerously-bypass-approvals-and-sandbox`
- 仅在完全信任环境使用

---

## 4. 平台实现细节

### 4.1 macOS：Apple Seatbelt

使用 macOS 内置的 `sandbox-exec`（`/usr/bin/sandbox-exec`），配合分层策略文件：

- 基础限制 profile
- 网络规则 profile
- 只读平台默认 profile
- 根据 `--sandbox` 模式生成对应策略文本

Seatbelt 提供细粒度的进程能力控制，包括文件系统访问、网络连接和系统调用限制。

### 4.2 Linux：Landlock + seccomp + bubblewrap（最成熟）

自 v0.115.0 起，默认使用 bubblewrap（`bwrap`）实现容器化隔离。

#### 4.2.1 进程硬化

- `PR_SET_NO_NEW_PRIVS`：阻止子进程获取更多权限
- seccomp 网络过滤器：按需阻止网络相关 syscall

#### 4.2.2 文件系统隔离

```
--ro-bind / /                    # 整个文件系统只读挂载
--bind <writable_root> <path>    # 显式可写路径
--ro-bind .git .git              # 受保护路径强制只读覆盖
```

- 符号链接阻断：对符号链接或缺失组件挂载 `/dev/null`
- 即使可写根目录下的子路径，`.git` 和 `.codex` 也被重新标记为只读

#### 4.2.3 命名空间隔离

```
--unshare-user    # 私有用户命名空间
--unshare-pid     # 私有 PID 命名空间
--unshare-net     # 私有网络命名空间（网络受限时）
--proc /proc      # 挂载独立的 /proc
```

#### 4.2.4 受管代理模式（Managed Proxy）

当需要受控网络访问时：

1. 使用 `--unshare-net` 创建独立网络命名空间
2. 启动内部 TCP→UDS→TCP 桥接
3. 工具流量仅能到达配置的代理端点
4. seccomp 阻止用户命令创建新的 `AF_UNIX` 和 `socketpair`，防止通过 Unix socket 逃逸

#### 4.2.5 Linux 沙箱辅助二进制

`codex-linux-sandbox` 是一个独立的库 crate，暴露 `run_main()` 用于 arg0 路由。当检测到 arg0 为 `codex-...-sandbox` 时，执行沙箱逻辑而非正常 CLI。

辅助二进制在当前线程上应用策略：
- **Landlock**：全局可读，仅 `/dev/null` 和可写根目录可写
- **Seccomp**：阻止 `connect`、`bind`、`sendto` 等网络 syscall，仅允许 `AF_UNIX`

### 4.3 Windows：AppContainer + Restricted Token

- 在 AppContainer profile 派生的 restricted token 中启动命令
- 仅授予特定文件系统能力（通过 capability SID）
- 禁用出站网络：覆盖代理相关环境变量 + 注入常见网络工具的 stub 可执行文件
- **局限**：无法阻止在 Everyone SID 已有写权限的目录中的文件操作（如 world-writable 文件夹）

---

## 5. 云端沙箱

### 5.1 Codex Cloud

每个任务在 OpenAI 管理的隔离容器中运行，采用两阶段运行模型：

1. **Setup 阶段**：可访问网络安装依赖，Secrets 仅此阶段可见
2. **Agent 阶段**：默认离线执行，Secrets 已被清除

### 5.2 Docker Sandboxes（MicroVM 方案）

Docker 推出了基于 Firecracker 的 microVM 隔离方案：

- **独立 Linux 内核**：Agent 的 syscall 不会到达宿主机内核
- **独立 Docker daemon**：可在沙箱内构建和运行容器
- **隔离文件系统**：工作区通过文件系统透传双向同步，宿主机其余部分不可见
- **网络代理**：所有出站流量通过宿主机 HTTP/HTTPS 代理，强制网络策略

Firecracker VMM 仅约 83,000 行 Rust 代码（对比 Linux 内核约 4,000 万行 C），攻击面极小。VM 逃逸需要 hypervisor CVE，这类漏洞在黑市赏金高达 $250K–$500K。

---

## 6. 策略配置

### 6.1 config.toml 配置

```toml
# ~/.codex/config.toml
sandbox_mode = "workspace-write"
approval_policy = "on-request"

[sandbox_workspace_write]
writable_roots = [
    "/Users/user/projects/my-app/src",
    "/Users/user/projects/my-app/tests",
]

# 命令前缀规则（原 execpolicy，现 rules）
[[rules]]
prefix = "npm test"
allow = true

[[rules]]
prefix = "python -m pytest"
allow = true

[[rules]]
prefix = "docker"
allow = false
```

### 6.2 命令行参数

```bash
# 推荐的本地自动化预设
codex --sandbox workspace-write --ask-for-approval on-request

# 完全自动化（沙箱内）
codex --full-auto

# 无沙箱无审批（危险）
codex --sandbox danger-full-access --ask-for-approval never
```

### 6.3 Rules 系统（原 execpolicy）

按命令前缀粒度控制执行策略，支持全局、profile、项目三级配置：

- 配置路径：`~/.codex/rules/default.rules`（原 `*.codexpolicy`）
- 可指定命令免沙箱执行或完全阻止
- 格式为命令前缀匹配 + allow/deny 规则

---

## 7. 网络访问控制

网络策略与文件系统策略分离管理（v0.72.0+ 引入 split policy）：

- **默认**：网络完全关闭（显著降低 prompt injection 和数据泄露风险）
- **受管代理模式**：通过内部桥接提供受控网络访问
- **环境变量标记**：`CODEX_SANDBOX_NETWORK_DISABLED=1` 通知工具和 MCP 服务器网络状态
- **Web 工具**：默认使用 OpenAI 维护的网页搜索缓存，无需开放完整网络

---

## 8. 执行策略与审批

沙箱模式定义技术边界，审批策略定义何时需要人工确认：

| 审批策略 | 行为 |
|---------|------|
| `never` | 从不请求审批 |
| `on-request` | 超出沙箱时请求 |
| `on-fail` | 沙箱拒绝后请求 |

安全边界交互：
- 沙箱内操作：自动执行，无需审批
- 需超出沙箱的操作：触发审批流程
- 用户可逐次批准或永久放行

---

## 9. 安全设计原则

1. **默认最小权限**：新目录默认 `read-only`，网络默认关闭
2. **内核级强制**：不是"建议"而是操作系统直接拒绝
3. **纵深防御**：策略 + OS 沙箱 + 审批流程三层保护
4. **策略分离**：文件系统策略和网络策略独立管理
5. **受保护路径不可覆盖**：`.git`、`.codex` 在任何模式下都强制只读
6. **无 secrets 泄露**：云环境 secrets 在 agent 阶段前清除

---

## 10. 局限性与注意事项

- **Windows**：无法阻止 Everyone SID 有写权限目录的操作
- **Linux 容器环境**：宿主/容器配置不暴露 Landlock/seccomp 时沙箱可能失效，需用 `--sandbox danger-full-access` 并依赖外部隔离
- **旧版 Linux 内核**：可能不支持 Landlock 所需特性
- **配置错误风险**：过于宽松的 profile 会使内核级保证形同虚设
- **writable_roots 过宽**：如果将整个 home 目录设为可写，保护大幅降低

---

## 11. 与其他方案对比

| 维度 | Codex CLI | Claude Code | Cursor |
|------|-----------|-------------|--------|
| 沙箱层级 | OS 内核级 | 应用层 hooks | 编辑器权限模型 |
| macOS | Seatbelt | 无 | 无 |
| Linux | Landlock + seccomp | 无 | 无 |
| 网络隔离 | 命名空间级 | 无 | 无 |
| 内核隔离 | 是 | 否 | 否 |
| 配置粒度 | 路径 + 命令前缀 | hooks + 提示 | 编辑器设置 |

Codex 是目前 AI 编码代理中沙箱方案最成熟的产品，特别适合在共享机器、不可信 PR 或 CI 环境中运行。

---

## 参考资料

- [Codex Sandboxing Architecture (Mintlify)](https://www.mintlify.com/openai/codex/architecture/sandboxing)
- [Sandbox - OpenAI Developers](https://developers.openai.com/codex/concepts/sandboxing)
- [Agent Approvals & Security - OpenAI Developers](https://developers.openai.com/codex/agent-approvals-security)
- [Codex GitHub - sandbox.md](https://github.com/openai/codex/blob/main/docs/sandbox.md)
- [GPT-5.2-Codex Agent Sandbox](https://deploymentsafety.openai.com/gpt-5-2-codex/agent-sandbox)
- [Docker Sandboxes for Codex CLI](https://codex.danielvaughan.com/2026/04/13/docker-sandboxes-codex-cli-microvm-isolation/)
- [Codex CLI vs Claude Code vs Cursor Architecture](https://aicatchup.com/comparisons/codex-cli-vs-claude-code-vs-cursor-architecture)
