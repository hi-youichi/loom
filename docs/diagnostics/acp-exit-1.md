# loom-acp Exit 1 诊断指南

## Exit Code 含义

loom-acp 是一个 stdio 协议进程（IDE 启动后通过 stdin/stdout JSON-RPC 通信）。进程退出时返回 exit code，IDE 据此判断状态：

| Exit Code | 含义 |
|-----------|------|
| **0** | 正常退出（stdin 关闭、IDE 关闭） |
| **1** | 错误退出（`Result::Err` 从 `main()` 传播） |
| **203** | SIGHUP 触发的 reload 退出（Unix only） |

## Exit 1 的根因分析

Exit 1 出现在 `main()` 的 `run_server()` 返回 `Err(...)` 时。可能原因按启动阶段分类：

### 1. 配置阶段

```
main() → config::load_and_apply_with_report("loom", None)
```

- **LOOM_CONFIG 环境变量指向无效文件**
- **配置文件内容格式错误**（YAML/TOML 语法错误）

### 2. Agent 创建阶段

```
run_stdio_loop() → LoomAcpAgent::with_session_update_tx(tx)
```

- **LLM provider 未配置**（缺少 API key）
- **模型配置找不到**（model_id 不存在）
- **Agent profile 注册失败**

### 3. Tokio Runtime 阶段

```
tokio::runtime::Builder::new_multi_thread().build()?
```

- **系统资源不足**（线程创建失败）

### 4. ACP 连接阶段

```
Agent.builder()...connect_to(ByteStreams::new(stdout, stdin)).await
```

- **stdio 被意外关闭**（IDE crash）
- **协议握手失败**（ACP 版本不兼容）
- **JSON-RPC 序列化错误**

### 5. Panic

```
std::panic::set_hook → eprintln!("loom-acp panic: ...")
```

- **任何未处理的 panic** 会触发 panic hook，然后 exit 1
- panic hook 会打印具体位置：`loom-acp panic: <msg> at <file>:<line>`

## 诊断步骤

### Step 1: 查看日志

```bash
# 默认日志路径
cat ~/.loom/acp/loom-acp.log

# 查看日志路径
loom-acp --show-log-dir

# 增加日志级别
loom-acp --log-level debug
# 或更详细：
loom-acp --log-level trace
```

日志中搜索关键字：
- `loom-acp panic` — panic 错误
- `connect_to failed` — ACP 连接失败
- `run_stdio_loop error` — 主循环错误
- `ERROR` / `WARN` 级别日志

### Step 2: 手动启动 loom-acp

```bash
# 直接运行，观察 stderr 输出
loom-acp --log-level debug 2>acp-debug.log

# 如果 IDE 无法启动，手动测试 JSON-RPC
echo '{"jsonrpc":"2.0","method":"initialize","id":1,"params":{}}' | loom-acp --log-level trace
```

### Step 3: 检查配置

```bash
# 查看当前配置
cat ~/.loom/config.yaml    # 或 LOOM_CONFIG 指向的文件

# 验证 API key 是否设置
echo $OPENAI_API_KEY
echo $ANTHROPIC_API_KEY

# 查看 loom 配置摘要（如果 loom CLI 可用）
loom config show
```

### Step 4: 检查进程状态

```bash
# 查看是否有残留进程
ps aux | grep loom-acp

# 查看 PID 文件
cat ~/.loom/acp/loom-acp.pid

# 查看端口占用（如果用到网络）
lsof -i -P | grep loom
```

### Step 5: 查看 IDE 日志

- **Zed**: 打开 `zed: logs` 面板，搜索 `loom` 或 `acp`
- **JetBrains**: Help → Show Log in Explorer/Finder，搜索 `loom-acp`

## 常见 Exit 1 场景和解决方案

### 场景 A: "No API key configured"

**日志**: `ERROR loom_acp: LLM provider not configured`

**解决**:
```bash
# 设置环境变量
export OPENAI_API_KEY=sk-...

# 或在配置文件中设置
# ~/.loom/config.yaml
llm:
  openai:
    api_key: sk-...
```

### 场景 B: "Connection closed" 误报为 exit 1

**症状**: 进程退出但实际是 IDE 关闭了连接

**区分**: 查看日志最后几行：
- `run_stdio_loop finished (connection closed)` → 正常退出（应 exit 0）
- `run_stdio_loop error` → 真正的错误

### 场景 C: Panic

**日志**: `loom-acp panic: <msg> at <file>:<line>`

**解决**: 这是一个 bug，记录 panic 位置和触发条件，在代码中修复。

### 场景 D: SIGHUP 在非 Unix 平台

**日志**: `loom-acp reload: not supported on this platform`

**解决**: Reload 仅支持 Unix。Windows 上不需要 reload 机制。

### 场景 E: 配置文件损坏

**日志**: `config load error: ...`

**解决**:
```bash
# 备份并重新创建配置
mv ~/.loom/config.yaml ~/.loom/config.yaml.bak
loom-acp --log-level debug  # 测试是否正常
```

## 改进建议：增加 exit code 语义

当前 exit 1 是通用的 `Err` 传播。可以细化 exit code 以便于诊断：

| Exit Code | 建议含义 |
|-----------|---------|
| 0 | 正常退出 |
| 1 | 通用错误 |
| 2 | 配置错误 |
| 3 | LLM/Provider 错误 |
| 203 | SIGHUP reload |

如需实现，在 `main.rs` 中将 `run_server()` 的错误类型改为 enum 并在 `main()` 中 match 转换为具体 exit code。
