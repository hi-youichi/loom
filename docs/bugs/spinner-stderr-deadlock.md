# Spinner stderr 死锁 Bug

## 现象

使用 `loom goal` 或 `loom` CLI 模式时，进程会卡在 "Thinking..." spinner 无限旋转，最终只能手动 kill。

ACP 模式下不存在此问题。

## 根因

`run_tty_spinner` 在函数开头执行 `stderr.lock()` 获取 stderr 的 Mutex 锁，并在**整个函数生命周期**（包括 `recv_timeout` 等待期间）持续持有该锁：

```rust
fn run_tty_spinner(rx: mpsc::Receiver<SpinnerMsg>, initial_label: &str) {
    let stderr = std::io::stderr();
    let mut stderr_lock = stderr.lock(); // ← 持有锁直到函数返回

    loop {
        match rx.recv_timeout(TICK_INTERVAL) { // ← 等待期间仍持有锁
            // ... write to stderr_lock ...
        }
    }
}
```

当 `on_event` 回调中的任何代码调用 `eprintln!` 时，主线程尝试获取同一个 stderr Mutex 锁，**永远阻塞**。

### 死锁路径

1. `TaskStart("think")` 事件 → `on_event_react` 创建 Spinner → 后台线程启动，锁住 stderr
2. LLM 返回 tool_calls 但无文本内容 → 无 `Messages` 事件产生
3. 因为无 `Messages` 事件，`handle_messages` 中的 `sp.finish_box()` 不会被调用 → Spinner 不停止
4. `Updates` 事件到达 → `on_event_react` 的 Updates 分支执行 `log_tools_used()` → 调用 `eprintln!`
5. 主线程在 stderr Mutex 上阻塞（Spinner 后台线程持有）
6. **死锁**：主线程等 stderr 锁，Spinner 线程在 `recv_timeout` 期间持续持有锁

### ACP 为什么不受影响

ACP 模式使用 `notifier.try_send_event` 作为 `on_event` 回调（轻量 channel send），不创建 Spinner，不写 stderr，因此不会触发此死锁。

## 触发条件

- CLI 或 Goal 模式（使用 `create_stdio_event_callback` + `use_spinner: true`）
- stderr 是 TTY（进入 `run_tty_spinner` 而非 `run_pipe_spinner`）
- LLM 在某个 think turn 中返回 tool_calls 但无文本内容，导致 Spinner 未被 stop
- 随后有 `Updates` 事件触发 `log_tools_used()` 或其他 `eprintln!` 调用

## 修复

将 stderr 锁的获取改为**每次写入时获取、写入后立即释放**，`recv_timeout` 等待期间不持有锁：

```rust
// 修复前：整个生命周期持有锁
let stderr = std::io::stderr();
let mut stderr_lock = stderr.lock();
loop {
    match rx.recv_timeout(TICK_INTERVAL) { // 持有锁等待
        // ... write to stderr_lock ...
    }
}

// 修复后：每次写入前后获取/释放
loop {
    match rx.recv_timeout(TICK_INTERVAL) { // 不持有锁
        Ok(SpinnerMsg::Update(new_label)) => {
            let stderr = std::io::stderr();
            let mut stderr_lock = stderr.lock(); // 获取
            let _ = write!(stderr_lock, ...);
            let _ = stderr_lock.flush();
            // drop → 释放
        }
        // ...
    }
}
```

## 修改文件

- `cli/src/run/spinner.rs`
- `loom/src/stream_display/spinner.rs`

## 日期

2025-05-22
