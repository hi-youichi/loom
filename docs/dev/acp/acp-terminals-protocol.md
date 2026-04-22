# ACP Terminals 协议文档

> 原文: https://agentclientprotocol.com/protocol/terminals

Terminal 方法允许 Agent 在 Client 环境中执行 shell 命令，支持构建过程、脚本执行、命令行工具交互，并提供实时输出流和进程控制。

---

## 1. 检查支持

使用终端方法前，Agent **必须** 通过 `initialize` 响应中的 `clientCapabilities` 字段确认 Client 支持终端能力：

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "protocolVersion": 1,
    "clientCapabilities": {
      "terminal": true
    }
  }
}
```

若 `terminal` 为 `false` 或不存在，Agent **不得** 调用任何终端方法。

---

## 2. 终端生命周期

```
Create → Monitor → Wait → Kill(可选) → Release
```

1. **Create**: Agent 调用 `terminal/create` 启动命令
2. **Monitor**: Agent 调用 `terminal/output` 获取当前输出
3. **Wait**: Agent 调用 `terminal/wait_for_exit` 等待命令完成
4. **Kill**（可选）: Agent 调用 `terminal/kill` 终止命令
5. **Release**: Agent 调用 `terminal/release` 释放资源

---

## 3. 执行命令 — `terminal/create`

在新的终端中启动命令。

### 请求

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "terminal/create",
  "params": {
    "sessionId": "sess_abc123def456",
    "command": "npm",
    "args": ["test", "--coverage"],
    "env": [
      { "name": "NODE_ENV", "value": "test" }
    ],
    "cwd": "/home/user/project",
    "outputByteLimit": 1048576
  }
}
```

### 参数说明

| 参数 | 类型 | 说明 |
|------|------|------|
| `sessionId` | string | 会话 ID |
| `command` | string | 要执行的命令 |
| `args` | string[] | 命令参数列表（可选） |
| `env` | `{name: string, value: string}[]` | 环境变量（可选） |
| `cwd` | string | 工作目录（可选） |
| `outputByteLimit` | integer | 输出字节上限（可选） |

### 响应

Client 立即返回 Terminal ID，不等待命令完成：

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "terminalId": "term_xyz789"
  }
}
```

---

## 4. 获取输出 — `terminal/output`

获取终端的当前输出，不等待命令完成。

### 请求

```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "terminal/output",
  "params": {
    "sessionId": "sess_abc123def456",
    "terminalId": "term_xyz789"
  }
}
```

### 响应

Client 返回当前输出和退出状态（如果命令已结束）：

```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "result": {
    "output": "... current terminal output ...",
    "exitStatus": null,
    "truncated": false
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `output` | string | 当前累积的终端输出 |
| `exitStatus` | integer \| null | 退出码，未结束时为 null |
| `truncated` | boolean | 输出是否因超出 `outputByteLimit` 被截断 |

---

## 5. 等待退出 — `terminal/wait_for_exit`

阻塞等待命令完成后返回。

### 请求

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "terminal/wait_for_exit",
  "params": {
    "sessionId": "sess_abc123def456",
    "terminalId": "term_xyz789"
  }
}
```

### 响应

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "exitStatus": 0
  }
}
```

### 构建超时

结合 `terminal/kill` 可实现超时控制：Agent 启动命令后，在指定时间内若未收到 `wait_for_exit` 响应，调用 `terminal/kill` 终止命令。

---

## 6. 终止命令 — `terminal/kill`

终止正在运行的命令，但**不释放**终端资源。

### 请求

```json
{
  "jsonrpc": "2.0",
  "id": 8,
  "method": "terminal/kill",
  "params": {
    "sessionId": "sess_abc123def456",
    "terminalId": "term_xyz789"
  }
}
```

> kill 后仍可调用 `terminal/output` 获取已有输出，但不能再调用 `terminal/wait_for_exit`。

---

## 7. 释放终端 — `terminal/release`

若命令仍在运行则先终止，然后释放所有资源。

### 请求

```json
{
  "jsonrpc": "2.0",
  "id": 9,
  "method": "terminal/release",
  "params": {
    "sessionId": "sess_abc123def456",
    "terminalId": "term_xyz789"
  }
}
```

> Release 后 terminalId 失效，不能再用于任何终端方法。Agent 不再需要终端时**必须**调用此方法释放资源。

---

## 8. 最佳实践

- **始终 release**: 使用完毕后调用 `terminal/release` 释放资源
- **捕获输出**: `waitForExit()` 后获取最终输出以得到完整结果
- **超时控制**: 对长时间运行的命令设置超时，超时后 `kill`
- **检查能力**: 使用前检查 `clientCapabilities.terminal` 是否为 `true`
- **处理截断**: 关注 `truncated` 字段，输出可能因 `outputByteLimit` 被截断

---

## 9. 方法汇总

| 方法 | 用途 | 是否阻塞 |
|------|------|----------|
| `terminal/create` | 创建终端并执行命令 | 否（立即返回 terminalId） |
| `terminal/output` | 获取当前输出和退出状态 | 否 |
| `terminal/wait_for_exit` | 等待命令执行完成 | 是 |
| `terminal/kill` | 终止命令（不释放资源） | 否 |
| `terminal/release` | 终止命令并释放所有资源 | 否 |
