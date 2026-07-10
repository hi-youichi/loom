# ACP 协议审计：fs/read_text_file

## 协议规范

**`fs/read_text_file`** 是 Client → Agent 请求，用于从代理的工作目录读取文本文件。返回文件的 UTF-8 文本内容。Agent 必须应用其沙箱/路径解析规则以确保 client 只能读取授权目录中的文件。

## 实现状态

**已实现** — 端到端实现正确。

## 实现细节

### 入口点：`stdio_loop.rs`
**文件：** `apps/acp/src/stdio_loop.rs:1-410`（`fs/read_text_file` 分派）

Stdio 循环接收请求并将其分派给 `agent.read_text_file()`：

```rust
// stdio_loop.rs:fs/read_text_file 分派
FsReadTextFile(req) => {
    self.agent.read_text_file(req).await?
}
```

### 处理器：`agent.rs`
**文件：** `apps/acp/src/agent.rs:1437-1509`

`read_text_file` 方法执行文件读取并应用沙箱/路径解析规则。

### 测试覆盖

**文件：** `apps/acp/tests/e2e_mega.rs:286-301`

**文件：** `apps/acp/tests/e2e_mega.rs:303-319` — 路径越界测试

**文件：** `apps/acp/tests/agent_integration.rs:1-300` — 集成测试套件

## 实现方式

1. **Stdio 循环** 接收 `FsReadTextFileRequest`
2. **Agent 处理器** 验证路径在工作目录或白名单中
3. **文件读取** 使用标准 Rust I/O 读取
4. **响应** 序列化为 UTF-8 文本

## 差距与问题

未发现重大差距。实现正确：
- 路径解析已应用
- 越界访问被拒绝（已测试）
- UTF-8 编码已保留

## 验证

**结论：完整实现** — 测试覆盖 happy path 和 path-traversal 攻击（`e2e_mega.rs:286-319`）。

## 总结

`fs/read_text_file` 协议**完整实现**。核心文件读取正确工作；沙箱强制已到位并经过测试。
