# ACP 协议审计：fs/write_text_file

## 协议规范

**`fs/write_text_file`** 是 Client → Agent 请求，用于将 UTF-8 文本内容写入代理工作目录中的文件。Agent 必须应用沙箱规则以确保 client 只能写入授权目录中的文件。预期在写入之前进行原子操作（写入临时文件后重命名），以避免在写入过程中部分写入导致损坏。

## 实现状态

**已实现** — 端到端实现正确，但行为语义上存在"非原子性"差距。

## 实现细节

### 入口点：`stdio_loop.rs`
**文件：** `apps/acp/src/stdio_loop.rs`

Stdio 循环分派 `FsWriteTextFileRequest` 给 `agent.write_text_file()`。

### 处理器：`agent.rs`
**文件：** `apps/acp/src/agent.rs:1509-1545`

`write_text_file` 方法：
1. 验证路径在工作目录或白名单中
2. 创建父目录（如有必要）
3. **直接写入**文件（**无临时文件+rename 的原子性保证**）
4. 返回成功响应

### 测试覆盖

**文件：** `apps/acp/tests/e2e_mega.rs:322-340`

## 实现方式

```
FsWriteTextFile (stdio_loop)
  → agent.write_text_file()
    → 路径验证（沙箱）
    → fs::create_dir_all(父目录)
    → fs::write(目标路径, 内容)  ← 非原子
    → 返回 Ok(())
```

## 差距与问题

| 差距 | 严重程度 | 描述 |
|-----|----------|------|
| **非原子性写入** | **中** | 直接 `fs::write` 不使用临时文件+`rename` 模式。电源故障或进程崩溃可能导致部分写入的文件损坏。 |
| **无 fsync 强制** | **低** | 写入后无显式 `sync_data()` / `sync_all()`。如果紧随其后的进程崩溃，文件系统元数据可能不一致。 |
| **路径遍历测试差距** | **低** | 仅测试合法路径；未明确覆盖路径遍历攻击向量（与 `fs/read_text_file` 不同）。 |

## 验证

**结论：已实现** — 核心写入功能工作；测试覆盖 happy path 但未明确覆盖路径遍历或原子性。

## 总结

`fs/write_text_file` 协议**已实现**但具有**非原子性**。核心文件写入工作，沙箱强制已到位，但缺少生产级文件系统持久性保证。

**建议：**
1. **优先修复：** 切换到临时文件+`rename`模式以保证原子性
2. **次要修复：** 在关键写入后添加 `sync_all()` 以强制磁盘同步
3. **测试覆盖：** 添加对路径遍历攻击的显式测试（与 `fs/read_text_file` 对称）

---

## 实现指南

### 当前实现摘要

```rust
// apps/acp/src/agent.rs:1509-1545
pub async fn handle_fs_write_text_file(
    &self,
    req: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, AgentError> {
    // 1. 路径验证（沙箱）
    let safe_path = self.sandbox.validate(&req.path)?;

    // 2. 创建父目录
    if let Some(parent) = safe_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // 3. 直接写入（非原子）
    tokio::fs::write(&safe_path, &req.content).await?;

    Ok(WriteTextFileResponse::default())
}
```

### 差距 1 修复：原子写入（temp + rename）

**问题位置：** `apps/acp/src/agent.rs:1509-1545`

直接 `fs::write` 不使用临时文件 + `rename` 模式，电源故障或进程崩溃可能导致部分写入的文件损坏。

**修复前：**

```rust
pub async fn handle_fs_write_text_file(
    &self,
    req: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, AgentError> {
    let safe_path = self.sandbox.validate(&req.path)?;
    if let Some(parent) = safe_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // ❌ 非原子：崩溃时可能留下半截文件
    tokio::fs::write(&safe_path, &req.content).await?;
    Ok(WriteTextFileResponse::default())
}
```

**修复后：**

```rust
use tokio::io::AsyncWriteExt;
use std::os::unix::fs::OpenOptionsExt;

pub async fn handle_fs_write_text_file(
    &self,
    req: WriteTextFileRequest,
) -> Result<WriteTextFileResponse, AgentError> {
    let safe_path = self.sandbox.validate(&req.path)?;

    // 1. 创建父目录
    if let Some(parent) = safe_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // 2. 在同一文件系统上创建临时文件
    let temp_path = safe_path.with_extension(format!(
        "{}.tmp.{:?}",
        safe_path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));

    // 3. 写入临时文件 + fsync 强制刷盘
    {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_SYNC)  // 每次写都 fsync（Unix）
            .open(&temp_path)
            .await?;

        file.write_all(&req.content).await?;
        file.sync_all().await?;  // ← 强制刷盘到磁盘
    }  // file 在此关闭

    // 4. 原子重命名（POSIX rename(2) 保证原子性）
    tokio::fs::rename(&temp_path, &safe_path).await?;

    // 5. 父目录 fsync（保证目录项也持久化）
    if let Some(parent) = safe_path.parent() {
        let dir = std::fs::File::open(parent)?;
        dir.sync_all()?;
    }

    Ok(WriteTextFileResponse::default())
}
```

**原子性保证机制：**

```text
【非原子（旧）】

时间线：
  t1: 打开目标文件
  t2: 写入字节 0-512  ← 崩溃可能发生在这里
  t3: 写入字节 513-1024
  t4: 关闭文件

风险：在 t2 和 t3 之间崩溃 → 文件包含半截内容

【原子（新）】

时间线：
  t1: 创建临时文件 foo.txt.tmp
  t2: 写入临时文件 + fsync
  t3: rename(临时, 目标)  ← POSIX 原子
  t4: fsync 父目录

保证：文件总是显示"旧内容"或"新内容"，不会显示半截
```

### 差距 2 修复：显式 fsync

**问题位置：** `apps/acp/src/agent.rs:1509-1545`（与差距 1 合并修复）

**修复后包含：**

```rust
// 1. 文件数据 fsync
file.sync_all().await?;

// 2. 父目录 fsync（关键 — 防止目录项丢失）
let dir = std::fs::File::open(parent)?;
dir.sync_all()?;

// 3. （可选）使用 O_SYNC 标志
.custom_flags(libc::O_SYNC | libc::O_DSYNC)
```

**fsync 各级别说明：**

| 级别 | 调用 | 持久化内容 |
|------|------|----------|
| 0 | 仅 `fs::write` | 内存缓冲（崩溃时丢失） |
| 1 | `file.sync_data()` | 文件数据（不含元数据） |
| 2 | `file.sync_all()` | 文件数据 + 元数据 |
| 3 | + `dir.sync_all()` | 目录项也持久化 |

**推荐：** 级别 2（文件 sync_all）+ 3（目录 sync_all）用于关键文件。

### 差距 3 修复：路径遍历测试

**问题位置：** `apps/acp/tests/e2e_mega.rs:322-340`（与 fs/read_text_file 的 286-319 对称）

**修复后：**

```rust
#[tokio::test]
async fn test_write_text_file_path_traversal_rejected() {
    let client = TestClient::connect().await?;

    // 1. 尝试写入工作目录之外
    let attacks = vec![
        "/etc/passwd",                                    // 绝对路径
        "../../../etc/passwd",                            // 相对路径遍历
        "..\\..\\..\\windows\\system32\\config\\sam",     // Windows 遍历
        "/home/user/.ssh/authorized_keys",                // 敏感文件
        "/proc/self/mem",                                 // procfs
        "valid.txt\x00../../etc/passwd",                 // null byte 注入
        "/dev/sda",                                       // 设备文件
    ];

    for path in attacks {
        let result = client.fs_write_text_file(path, "malicious").await;
        assert!(result.is_err(), "should reject path: {}", path);
        let err = result.unwrap_err();
        assert!(matches!(err, ACPError::PathValidationFailed { .. }),
                "wrong error type for {}: {:?}", path, err);
    }
}

#[tokio::test]
async fn test_write_text_file_symlink_escape_rejected() {
    // 创建指向工作目录外的符号链接
    let target = std::env::temp_dir().join("loom-fs-test-target");
    std::fs::write(&target, "outside").unwrap();
    let link = workdir.join("sneaky_link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let result = client.fs_write_text_file("sneaky_link", "evil").await;
    assert!(result.is_err(), "symlink escape should be rejected");
}

#[tokio::test]
async fn test_write_text_file_atomic_under_concurrent_reads() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    let path = workdir.join("concurrent.txt");
    std::fs::write(&path, "initial").unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let writer_path = path.clone();
    let writer = tokio::spawn(async move {
        barrier.wait().await;
        // 模拟大量写入
        for i in 0..1000 {
            client.fs_write_text_file("concurrent.txt",
                &format!("content-{}", i)).await.unwrap();
        }
    });

    let reader_path = path.clone();
    let reader = tokio::spawn(async move {
        barrier.wait().await;
        let mut never_partial = true;
        for _ in 0..1000 {
            let content = std::fs::read_to_string(&reader_path).unwrap();
            // 读取的内容必须是"initial"或完整的新内容，不能是半截
            if !(content == "initial" || content.starts_with("content-")) {
                never_partial = false;
                break;
            }
        }
        never_partial
    });

    writer.await.unwrap();
    assert!(reader.await.unwrap(), "read saw partial content");
}
```

### 演示：原子写入的时序

```text
【旧实现】fs::write 直接写入：

Application  OS           Disk
    │         │             │
    │──write()─▶             │
    │         │──write──▶   │
    │         │             │ (数据在内核缓冲)
    │         │ ◀──ok──────│
    │ ◀──ok───│             │
    │         │             │
    ⚡ 断电     ⚡ 断电       ⚡ 数据丢失
    │         │             │
    (文件可能半截)

【新实现】temp + rename + fsync：

Application  OS           Disk         FS
    │         │             │          │
    │──open(tmp)─▶          │          │
    │         │──open───▶   │          │
    │──write(tmp)─▶         │          │
    │         │──write──▶   │          │
    │──sync(tmp)─▶          │          │
    │         │──fsync──▶   │          │
    │         │             │──fsync─▶ │
    │         │             │ ◀─ok────│
    │──close(tmp)─▶         │          │
    │──rename(tmp→dst)─▶    │          │
    │         │──rename────▶│          │
    │         │             │──更新───▶│
    │         │             │ ◀─ok────│
    │──sync(dir)─▶          │          │
    │         │             │──fsync─▶ │
    │         │             │ ◀─ok────│
    │ ◀──ok───│             │          │
    │         │             │          │
    ⚡ 断电     ⚡ 断电       ⚡ 断电    ⚡ 断电
    │         │             │          │
    (任何时刻崩溃，文件总是"旧"或"新")
```

### 演示：跨平台实现

```rust
// 跨平台原子写入抽象
pub struct AtomicWriter;

impl AtomicWriter {
    pub async fn write(path: &Path, content: &[u8]) -> Result<(), IoError> {
        #[cfg(unix)]
        Self::write_unix(path, content).await?;

        #[cfg(windows)]
        Self::write_windows(path, content).await?;

        Ok(())
    }

    #[cfg(unix)]
    async fn write_unix(path: &Path, content: &[u8]) -> Result<(), IoError> {
        use std::os::unix::fs::OpenOptionsExt;

        // 1. 临时文件
        let temp = path.with_extension("tmp");
        let mut file = tokio::fs::OpenOptions::new()
            .write(true).create(true).truncate(true)
            .mode(0o644)
            .custom_flags(libc::O_SYNC)
            .open(&temp).await?;

        file.write_all(content).await?;
        file.sync_all().await?;
        drop(file);

        // 2. 原子重命名
        tokio::fs::rename(&temp, path).await?;

        // 3. 父目录 fsync
        if let Some(parent) = path.parent() {
            let dir = std::fs::File::open(parent)?;
            dir.sync_all()?;
        }

        Ok(())
    }

    #[cfg(windows)]
    async fn write_windows(path: &Path, content: &[u8]) -> Result<(), IoError> {
        // Windows: 使用 MoveFileEx with REPLACE_EXISTING
        let temp = path.with_extension("tmp");
        tokio::fs::write(&temp, content).await?;

        // FlushFileBuffers 等价
        let file = tokio::fs::OpenOptions::new().write(true).open(&temp).await?;
        file.sync_all().await?;
        drop(file);

        // 原子替换
        let temp_str = temp.to_str().ok_or(IoError::InvalidPath)?;
        let path_str = path.to_str().ok_or(IoError::InvalidPath)?;
        let result = unsafe {
            windows::Win32::Storage::FileSystem::MoveFileExW(
                windows::core::HSTRING::from(temp_str),
                windows::core::HSTRING::from(path_str),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        result.ok().map_err(IoError::from)?;
        Ok(())
    }
}
```

### 测试场景

在 `apps/acp/tests/e2e_mega.rs:322-340` 扩展：

```rust
// 上述 3 个测试（差距 3a-3c）

#[tokio::test]
async fn test_write_text_file_creates_parent_dirs() {
    let nested = workdir.join("a/b/c/file.txt");
    let result = client.fs_write_text_file(
        "a/b/c/file.txt",
        "content"
    ).await;
    assert!(result.is_ok());
    assert!(nested.exists());
}

#[tokio::test]
async fn test_write_text_file_overwrites_existing() {
    let path = "overwrite.txt";
    client.fs_write_text_file(path, "first").await.unwrap();
    client.fs_write_text_file(path, "second").await.unwrap();
    let content = std::fs::read_to_string(workdir.join(path)).unwrap();
    assert_eq!(content, "second");
}

#[tokio::test]
async fn test_write_text_file_unicode_content() {
    let content = "你好世界 🌍\nこんにちは";
    client.fs_write_text_file("unicode.txt", content).await.unwrap();
    let read_back = std::fs::read_to_string(workdir.join("unicode.txt")).unwrap();
    assert_eq!(read_back, content);
}
```

### 验收清单

**差距 1 — 原子写入：**
- [ ] `agent.rs:1509-1545` 替换 `fs::write` 为 temp + rename 模式
- [ ] 临时文件路径：`{path}.tmp.{pid}`
- [ ] 使用 `tokio::fs::rename` 替换 `std::fs::rename`
- [ ] 添加 `parent.sync_all()` 调用

**差距 2 — fsync：**
- [ ] `file.sync_all().await?` 在 rename 前调用
- [ ] 父目录 `File::open + sync_all` 在 rename 后调用
- [ ] （可选）Unix `O_SYNC` 标志用于更严格的同步
- [ ] Windows 使用 `MOVEFILE_WRITE_THROUGH`

**差距 3 — 路径遍历测试：**
- [ ] 添加 `test_write_text_file_path_traversal_rejected`（7 个攻击向量）
- [ ] 添加 `test_write_text_file_symlink_escape_rejected`
- [ ] 添加 `test_write_text_file_atomic_under_concurrent_reads`
- [ ] 与 `fs/read_text_file` 的路径遍历测试对称

**测试覆盖：**
- [ ] 3 个核心原子性测试（路径遍历、符号链接、并发读）
- [ ] 3 个补充测试（嵌套目录、覆盖、Unicode）
- [ ] 验证：所有测试在修复后通过
