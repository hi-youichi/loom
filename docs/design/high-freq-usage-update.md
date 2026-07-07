# Loom ACP 高频 Token 使用量更新方案

## 1. 背景与目标

### 1.1 当前现状
Loom 项目当前在 ACP 协议实现中，token 使用量更新通过 `usage_update` 通知发送，但存在以下限制：
- **更新频率低**：仅在每次 LLM `Usage` 事件时发送一次更新
- **粒度粗糙**：以轮次（prompt turn）级别统计，无法提供细粒度实时反馈
- **用户体验受限**：无法及时了解 token 消耗进度和上下文窗口状态

### 1.2 目标
设计一个高频 token 使用量更新机制，实现：
- ✅ 实时反馈：提供细粒度的 token 使用情况更新
- ✅ 性能优化：避免消息爆炸，平衡实时性和性能
- ✅ 协议兼容：保持 ACP 协议标准格式
- ✅ 可配置性：支持不同使用场景的参数配置
- ✅ 降级机制：确保在异常情况下的稳定性

## 2. 当前实现分析

### 2.1 现有架构
```rust
// apps/acp/src/stream_bridge.rs
pub struct SessionNotifier {
    tx: mpsc::Sender<SessionNotification>,
    session_id: SessionId,
    context_window_size: Option<u64>,
    usage_acc: Option<Arc<Mutex<TurnUsage>>>,
}
```

### 2.2 当前流程
1. **事件处理**：`StreamEvent::Usage` 触发更新
2. **数据提取**：`extract_usage_tokens()` 从事件中提取 `prompt_tokens`
3. **通知发送**：直接发送 `StreamUpdate::UsageUpdate`
4. **累积统计**：`TurnUsage` 累积整个 turn 的 token 使用情况

### 2.3 局限性
- **触发被动**：只能基于 LLM 的 Usage 事件触发
- **信息有限**：只提供 `used` 和 `size`，缺少增量信息
- **无智能触发**：没有基于阈值或时间的触发机制

## 3. 高频更新方案设计

### 3.1 核心架构
```
┌─────────────────────────────────────────────────────────────┐
│                    SessionNotifier                           │
│  ┌──────────────────────────────────────────────────────┐  │
│  │         HighFreqUsageTracker                         │  │
│  │  • 增量跟踪 (pending_tokens)                        │  │
│  │  • 智能触发 (阈值 + 时间 + 百分比)                   │  │
│  │  • 性能优化 (降级 + 节流)                            │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
        ┌───────────────────┼───────────────────┐
        │                   │                   │
        ▼                   ▼                   ▼
  增量触发           时间间隔触发         百分比触发
   (≥50 tokens)      (≥100ms)          (50%,75%,90%...)
```

### 3.2 数据流设计
```
LLM Usage Event
    │
    ▼
extract_usage_delta()
    │
    ▼
HighFreqUsageTracker.update_tokens()
    │
    ├── 检查触发条件
    │   ├── 增量阈值 (min_increment)
    │   ├── 时间间隔 (min_interval_ms)  
    │   └── 百分比阈值 (50%,75%,85%,90%,95%)
    │
    ▼
send_usage_update() ←─┐
    │                 │
    ▼                 │
StreamUpdate::UsageUpdate
    │                 │
    ▼                 │
session/update        │
    │                 │
    └─────────────────┘
         │
         ▼
    客户端处理
```

## 4. 具体实现方案

### 4.1 高频跟踪器核心实现

#### 4.1.1 数据结构定义
```rust
/// 高频token使用量跟踪器
pub struct HighFreqUsageTracker {
    base_used: u64,              // 基准已使用量
    current_used: u64,           // 当前已使用量
    size: u64,                   // 总上下文窗口大小
    last_notified_used: u64,     // 上次通知时的使用量
    pending_tokens: u64,         // 待处理的token增量
    last_notify_time: Instant,   // 上次通知时间
    min_increment: u64,          // 最小增量阈值
    min_interval_ms: u64,        // 最小时间间隔
}

/// 使用量更新信息
#[derive(Clone)]
pub struct UsageUpdateInfo {
    pub used: u64,               // 当前已使用量
    pub size: u64,               // 总窗口大小
    pub increment: u64,          // 增量
    pub timestamp: Instant,      // 时间戳
}
```

#### 4.1.2 核心更新逻辑
```rust
impl HighFreqUsageTracker {
    pub fn new(base_used: u64, size: u64) -> Self {
        Self {
            base_used,
            current_used: base_used,
            size,
            last_notified_used: base_used,
            pending_tokens: 0,
            last_notify_time: Instant::now(),
            min_increment: 50,     // 默认增量阈值
            min_interval_ms: 100,  // 默认时间间隔
        }
    }
    
    /// 更新token计数，检查是否需要通知
    pub fn update_tokens(&mut self, delta: u64) -> Option<UsageUpdateInfo> {
        self.pending_tokens += delta;
        self.current_used = self.base_used + self.pending_tokens;
        
        let now = Instant::now();
        let elapsed_ms = now.duration_since(self.last_notify_time).as_millis() as u64;
        
        // 多维度触发条件检查
        let should_notify = 
            self.increment_trigger_met() ||           // 增量触发
            self.interval_trigger_met(elapsed_ms) ||  // 时间触发
            self.percentage_trigger_met();            // 百分比触发
        
        if should_notify && self.current_used != self.last_notified_used {
            let increment = self.current_used - self.last_notified_used;
            self.last_notified_used = self.current_used;
            self.last_notify_time = now;
            
            return Some(UsageUpdateInfo {
                used: self.current_used,
                size: self.size,
                increment,
                timestamp: now,
            });
        }
        
        None
    }
    
    fn increment_trigger_met(&self) -> bool {
        self.pending_tokens >= self.min_increment
    }
    
    fn interval_trigger_met(&self, elapsed_ms: u64) -> bool {
        elapsed_ms >= self.min_interval_ms
    }
    
    fn percentage_trigger_met(&self) -> bool {
        let percentage = (self.current_used as f64 / self.size as f64) * 100.0;
        let thresholds = [50.0, 75.0, 85.0, 90.0, 95.0];
        let last_percentage = (self.last_notified_used as f64 / self.size as f64) * 100.0;
        
        thresholds.iter().any(|&t| last_percentage < t && percentage >= t)
    }
}
```

### 4.2 SessionNotifier 扩展

#### 4.2.1 结构扩展
```rust
pub struct SessionNotifier {
    tx: mpsc::Sender<SessionNotification>,
    session_id: SessionId,
    current_message_id: Mutex<Option<String>>,
    context_window_size: Option<u64>,
    usage_acc: Option<Arc<Mutex<TurnUsage>>>,
    // 新增高频跟踪器
    high_freq_tracker: Arc<Mutex<Option<HighFreqUsageTracker>>>,
}
```

#### 4.2.2 方法扩展
```rust
impl SessionNotifier {
    /// 启用高频更新模式
    pub fn enable_high_freq_tracking(&self, base_used: u64, size: u64) {
        let mut tracker = self.high_freq_tracker.lock().unwrap();
        *tracker = Some(HighFreqUsageTracker::new(base_used, size));
    }
    
    /// 禁用高频更新模式，发送最终更新
    pub async fn disable_high_freq_tracking(&self) {
        let mut tracker = self.high_freq_tracker.lock().unwrap();
        if let Some(tracker) = tracker.take() {
            self.send_usage_update(
                tracker.current_used,
                tracker.size,
                tracker.get_increment(),
            ).await;
        }
    }
    
    async fn send_usage_update(&self, used: u64, size: u64, increment: u64) {
        let meta = self.snapshot_token_usage_meta();
        let mut extended_meta = meta.unwrap_or_default();
        
        // 添加增量信息到_meta
        extended_meta.insert("increment".to_string(), serde_json::json!(increment));
        extended_meta.insert("timestamp".to_string(), 
            serde_json::json!(SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()));
        
        let update = StreamUpdate::UsageUpdate { 
            used, 
            size, 
            meta: Some(extended_meta) 
        };
        
        if let Some(notif) = stream_update_to_session_notification(&self.session_id, &update) {
            if let Err(e) = self.tx.send(notif).await {
                tracing::error!(session_id = %self.session_id, error = %e, 
                    "Failed to send high-freq usage update");
            }
        }
    }
}
```

### 4.3 事件处理增强

#### 4.3.1 修改 send_event 方法
```rust
pub async fn send_event(&self, event: &TypedAnyStreamEvent) {
    let mut updates = loom_event_to_updates(event);
    
    // 处理高频更新
    if let Some(size) = self.context_window_size {
        if let Some(usage_delta) = extract_usage_delta(event) {
            let mut tracker = self.high_freq_tracker.lock().unwrap();
            if let Some(tracker) = tracker.as_mut() {
                if let Some(update_info) = tracker.update_tokens(usage_delta) {
                    // 发送高频更新
                    self.send_usage_update(
                        update_info.used,
                        update_info.size,
                        update_info.increment,
                    ).await;
                }
            } else {
                // 降级到原始逻辑
                if let Some(used) = extract_usage_tokens(event) {
                    let meta = self.snapshot_token_usage_meta();
                    updates.push(StreamUpdate::UsageUpdate { used, size, meta });
                }
            }
        }
    }
    
    // 发送其他更新
    for u in updates {
        let u = self.inject_message_id(u);
        if let Some(notif) = stream_update_to_session_notification(&self.session_id, &u) {
            if let Err(e) = self.tx.send(notif).await {
                tracing::error!(session_id = %self.session_id, error = %e, 
                    "Failed to send stream event notification");
            }
        }
    }
}
```

#### 4.3.2 新增增量提取函数
```rust
/// 提取token使用增量
fn extract_usage_delta(ev: &TypedAnyStreamEvent) -> Option<u64> {
    let delta = match ev {
        TypedAnyStreamEvent::React(e) => extract_usage_delta_inner(e),
        TypedAnyStreamEvent::Dup(e) => extract_usage_delta_inner(e),
        TypedAnyStreamEvent::Tot(e) => extract_usage_delta_inner(e),
        TypedAnyStreamEvent::Got(e) => extract_usage_delta_inner(e),
    };
    delta
}

fn extract_usage_delta_inner<S>(ev: &StreamEvent<S>) -> Option<u64>
where
    S: Clone + Send + Sync + std::fmt::Debug + 'static,
{
    match ev {
        StreamEvent::Usage { 
            prompt_tokens, 
            completion_tokens, 
            total_tokens,
            cached_tokens,
            ..
        } => {
            // 返回净增量（总计 - 缓存）
            let total = *total_tokens as u64;
            let cached = cached_tokens.unwrap_or(0) as u64;
            Some(total.saturating_sub(cached))
        },
        _ => None,
    }
}
```

### 4.4 Agent 集成

#### 4.4.1 在 agent.rs 中集成
```rust
pub async fn handle_prompt_request(&self, session_id: &str, request: PromptRequest) -> Result<PromptResponse> {
    // 初始化通知器
    let notifier = self.create_session_notifier(session_id).await?;
    
    // 启用高频跟踪
    if let Some(size) = notifier.context_window_size {
        let base_used = self.get_current_token_usage(session_id)?;
        notifier.enable_high_freq_tracking(base_used, size);
    }
    
    // 执行 prompt
    let result = self.run_agent_with_options(session_id, options, Some(notifier.clone())).await;
    
    // 禁用高频跟踪，发送最终更新
    notifier.disable_high_freq_tracking().await;
    
    // 返回结果...
}
```

## 5. 配置与优化选项

### 5.1 预设配置

#### 5.1.1 激进配置（高实时性）
```rust
let tracker = HighFreqUsageTracker::with_config(
    base_used, 
    size, 
    25,    // 25 tokens 增量
    50     // 50ms 时间间隔
);
```
**适用场景**：需要实时反馈的调试模式、演示环境

#### 5.1.2 标准配置（平衡）
```rust
let tracker = HighFreqUsageTracker::new(base_used, size); // 默认: 50 tokens, 100ms
```
**适用场景**：一般使用场景

#### 5.1.3 保守配置（高性能）
```rust
let tracker = HighFreqUsageTracker::with_config(
    base_used, 
    size, 
    100,   // 100 tokens 增量
    200    // 200ms 时间间隔
);
```
**适用场景**：高负载生产环境、网络带宽受限场景

### 5.2 自定义配置接口
```rust
impl HighFreqUsageTracker {
    pub fn with_config(
        base_used: u64, 
        size: u64,
        min_increment: u64,
        min_interval_ms: u64,
    ) -> Self {
        Self {
            base_used,
            current_used: base_used,
            size,
            last_notified_used: base_used,
            pending_tokens: 0,
            last_notify_time: Instant::now(),
            min_increment,
            min_interval_ms,
        }
    }
    
    pub fn with_custom_thresholds(
        base_used: u64, 
        size: u64,
        thresholds: Vec<f64>,
    ) -> Self {
        Self {
            // ... 基础初始化
            custom_thresholds: thresholds, // [0.5, 0.75, 0.9]
        }
    }
}
```

### 5.3 性能优化特性

#### 5.3.1 批量发送优化
```rust
pub struct BatchUsageSender {
    pending_updates: Vec<UsageUpdateInfo>,
    batch_size: usize,
    flush_interval: Duration,
}

impl BatchUsageSender {
    pub fn try_flush(&mut self) {
        if self.pending_updates.len() >= self.batch_size {
            self.flush();
        }
    }
    
    pub fn flush(&mut self) {
        // 合并批量更新，发送合并后的通知
        let merged = self.merge_updates(&self.pending_updates);
        self.send_update(merged);
        self.pending_updates.clear();
    }
}
```

#### 5.3.2 自适应频率调整
```rust
impl HighFreqUsageTracker {
    pub fn adjust_frequency_based_on_load(&mut self, system_load: f64) {
        // 根据系统负载动态调整触发阈值
        if system_load > 0.8 {
            self.min_increment = 100;      // 高负载时提高阈值
            self.min_interval_ms = 200;
        } else if system_load < 0.3 {
            self.min_increment = 25;       // 低负载时降低阈值
            self.min_interval_ms = 50;
        }
    }
}
```

## 6. 协议兼容性

### 6.1 ACP 协议格式
保持 ACP 标准的 `usage_update` 格式：
```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_789xyz",
    "update": {
      "sessionUpdate": "usage_update",
      "used": 53000,
      "size": 128000,
      "_meta": {
        "token_usage": {
          "input_tokens": 32000,
          "output_tokens": 21000,
          "total_tokens": 53000,
          "cached_tokens": 5000
        },
        "increment": 150,
        "timestamp": 1691234567890,
        "precision": "incremental",
        "source": "high_freq_tracker"
      }
    }
  }
}
```

### 6.2 扩展字段说明
- `increment`: 本次更新的 token 增量
- `timestamp`: 更新时间戳（毫秒）
- `precision`: 精度标识（"incremental" vs "final"）
- `source`: 数据源标识

### 6.3 降级策略
当高频模式不可用时，自动降级到原始逻辑：
```rust
if let Some(tracker) = tracker.as_mut() {
    // 尝试高频模式
    if let Some(update_info) = tracker.update_tokens(usage_delta) {
        self.send_usage_update(...).await;
    }
} else {
    // 降级到原始模式
    if let Some(used) = extract_usage_tokens(event) {
        updates.push(StreamUpdate::UsageUpdate { used, size, meta });
    }
}
```

## 7. 客户端处理建议

### 7.1 TypeScript 客户端实现
```typescript
class HighFreqUsageHandler {
    private debouncer = new Debouncer(100); // 100ms 防抖
    private lastUsage = { used: 0, size: 0 };
    private uiThrottle = new Throttle(200);  // UI 更新节流
    
    handleUsageUpdate(update: UsageUpdate) {
        // 数据防抖处理
        this.debouncer.debounce(() => {
            const delta = update.used - this.lastUsage.used;
            const percentage = (update.used / update.size) * 100;
            
            // UI 节流更新
            this.uiThrottle.throttle(() => {
                this.updateUI({
                    used: update.used,
                    size: update.size,
                    delta,
                    percentage,
                    timestamp: update._meta.timestamp,
                    increment: update._meta.increment
                });
            });
            
            // 阈值警告（不节流）
            this.checkThresholdWarnings(percentage);
            
            this.lastUsage = update;
        });
    }
    
    checkThresholdWarnings(percentage: number) {
        const warnings = [
            { threshold: 90, message: "接近上下文限制: 90%" },
            { threshold: 95, message: "即将达到上下文限制: 95%" },
            { threshold: 99, message: "警告: 上下文即将耗尽!" }
        ];
        
        warnings.forEach(warning => {
            if (percentage >= warning.threshold && 
                this.lastUsage.used / this.lastUsage.size * 100 < warning.threshold) {
                this.showWarning(warning.message);
            }
        });
    }
}
```

### 7.2 性能优化建议
1. **防抖/节流**：避免 UI 频繁重绘
2. **虚拟滚动**：大量历史更新时使用虚拟列表
3. **差异更新**：只更新变化的 UI 部分
4. **后台处理**：大数据量的统计计算放在后台线程

### 7.3 用户体验优化
```typescript
class UsageVisualizer {
    private sparklineData: number[] = [];
    private maxDataPoints = 100;
    
    updateVisualization(update: UsageUpdate) {
        const percentage = update.used / update.size * 100;
        
        // 更新火花线数据
        this.sparklineData.push(percentage);
        if (this.sparklineData.length > this.maxDataPoints) {
            this.sparklineData.shift();
        }
        
        // 渲染进度条
        this.renderProgressBar(percentage, update._meta.increment);
        
        // 渲染趋势图
        this.renderSparkline(this.sparklineData);
        
        // 显示预估剩余时间
        this.estimateRemainingTime(update);
    }
}
```

## 8. 测试与验证方案

### 8.1 单元测试
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_increment_trigger() {
        let mut tracker = HighFreqUsageTracker::new(1000, 10000);
        
        // 未达到阈值
        assert!(tracker.update_tokens(20).is_none());
        
        // 达到阈值
        assert!(tracker.update_tokens(30).is_some());
    }
    
    #[test]
    fn test_interval_trigger() {
        let mut tracker = HighFreqUsageTracker::new(1000, 10000);
        
        // 短时间内多次更新
        tracker.update_tokens(10);
        std::thread::sleep(Duration::from_millis(50));
        assert!(tracker.update_tokens(10).is_none());
        
        // 超过时间间隔
        std::thread::sleep(Duration::from_millis(60));
        assert!(tracker.update_tokens(10).is_some());
    }
    
    #[test]
    fn test_percentage_trigger() {
        let mut tracker = HighFreqUsageTracker::new(4000, 10000);
        
        // 达到 50% 阈值
        assert!(tracker.update_tokens(1000).is_some()); // 5000/10000 = 50%
    }
}
```

### 8.2 集成测试
```rust
#[tokio::test]
async fn test_high_freq_integration() {
    let (tx, mut rx) = mpsc::channel(100);
    let notifier = SessionNotifier::new(tx, "test_session".into());
    notifier.enable_high_freq_tracking(1000, 10000);
    
    // 模拟 LLM 使用事件
    let events = create_mock_usage_events(vec![20, 30, 50, 40, 60]);
    for event in events {
        notifier.send_event(&event).await;
    }
    
    // 验证高频更新
    let updates = collect_updates(&mut rx).await;
    assert!(updates.len() > 1); // 应该有多次更新
    
    // 验证增量准确性
    let total_increment: u64 = updates.iter()
        .map(|u| u._meta.increment.as_u64().unwrap())
        .sum();
    assert_eq!(total_increment, 200); // 20+30+50+40+60
}
```

### 8.3 性能测试
```rust
#[tokio::test]
async fn benchmark_high_freq_overhead() {
    let (tx, _) = mpsc::channel(1000);
    let notifier = SessionNotifier::new(tx, "test".into());
    notifier.enable_high_freq_tracking(1000, 10000);
    
    let start = Instant::now();
    
    // 模拟大量高频事件
    for i in 0..1000 {
        let event = create_mock_usage_event(10);
        notifier.send_event(&event).await;
    }
    
    let duration = start.elapsed();
    println!("High frequency overhead: {:?}", duration);
    
    // 确保开销在可接受范围内
    assert!(duration.as_millis() < 500); // 小于 500ms
}
```

### 8.4 协议兼容性测试
```rust
#[tokio::test]
async fn test_acp_compatibility() {
    let (tx, mut rx) = mpsc::channel(100);
    let notifier = SessionNotifier::new(tx, "test".into());
    notifier.enable_high_freq_tracking(1000, 10000);
    
    // 触发更新
    notifier.send_event(&create_mock_usage_event(100)).await;
    
    let notification = rx.recv().await.unwrap();
    let update = notification.params.update;
    
    // 验证 ACP 格式
    assert_eq!(update.sessionUpdate, "usage_update");
    assert!(update.used.is_u64());
    assert!(update.size.is_u64());
    assert!(update._meta.is_object());
}
```

## 9. 部署与监控

### 9.1 渐进式部署
1. **Phase 1**: 在开发环境启用高频模式
2. **Phase 2**: 在测试环境验证性能影响
3. **Phase 3**: 生产环境灰度发布（10% 用户）
4. **Phase 4**: 全量部署，持续监控

### 9.2 监控指标
```rust
pub struct HighFreqMetrics {
    total_updates_sent: AtomicU64,
    total_updates_dropped: AtomicU64,
    average_update_frequency: AtomicU64, // 每秒更新数
    peak_pending_updates: AtomicU32,
    system_load_avg: AtomicF64,
}

impl HighFreqMetrics {
    pub fn record_update_sent(&self) {
        self.total_updates_sent.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn get_update_frequency(&self) -> f64 {
        // 计算每秒更新频率
        // 用于动态调整触发阈值
    }
}
```

### 9.3 运维配置
```rust
// 配置文件支持
#[derive(Deserialize)]
pub struct HighFreqConfig {
    pub enabled: bool,
    pub min_increment: Option<u64>,
    pub min_interval_ms: Option<u64>,
    pub adaptive: bool,
    pub thresholds: Vec<f64>,
}

// 环境变量支持
impl HighFreqConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("LOOM_HIGH_FREQ_ENABLED")
                .map(|v| v.parse().unwrap_or(false))
                .unwrap_or(true),
            min_increment: std::env::var("LOOM_HIGH_FREQ_MIN_INCREMENT")
                .ok()
                .and_then(|v| v.parse().ok()),
            min_interval_ms: std::env::var("LOOM_HIGH_FREQ_MIN_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok()),
            adaptive: std::env::var("LOOM_HIGH_FREQ_ADAPTIVE")
                .map(|v| v.parse().unwrap_or(false))
                .unwrap_or(true),
            thresholds: vec![0.5, 0.75, 0.85, 0.9, 0.95],
        }
    }
}
```

## 10. 风险与缓解

### 10.1 潜在风险
| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|----------|
| 消息爆炸 | 客户端崩溃 | 中 | 增量阈值 + 时间间隔 + 降级机制 |
| 性能下降 | Agent 响应变慢 | 中 | 批量发送 + 自适应频率 |
| 网络拥堵 | 延迟增加 | 低 | 客户端节流 + 批量处理 |
| 数据不一致 | 统计错误 | 低 | 最终一致性 + TurnUsage 校验 |

### 10.2 监控告警
```rust
impl HighFreqUsageTracker {
    pub fn check_health(&self) -> HealthStatus {
        let frequency = self.calculate_update_frequency();
        let system_load = self.get_system_load();
        
        if frequency > 100.0 { // 每秒超过 100 次更新
            return HealthStatus::Warning("High update frequency detected".to_string());
        }
        
        if system_load > 0.9 {
            return HealthStatus::Critical("System overload, consider reducing frequency".to_string());
        }
        
        HealthStatus::Healthy
    }
}
```

## 11. 总结

本方案通过智能触发机制、性能优化和协议兼容性设计，实现了 Loom ACP 的高频 token 使用量更新功能：

### 11.1 关键特性
- ✅ **细粒度更新**：从轮次级别提升到 token 级别
- ✅ **智能触发**：多维度触发条件避免消息爆炸  
- ✅ **性能优化**：批量发送、自适应调整、降级机制
- ✅ **协议兼容**：保持 ACP 标准格式，通过 `_meta` 扩展
- ✅ **可配置性**：支持多种预设和自定义配置

### 11.2 实施路径
1. **第一阶段**：实现核心跟踪器逻辑（预计 2-3 天）
2. **第二阶段**：集成到 SessionNotifier（预计 1-2 天）
3. **第三阶段**：Agent 集成和测试（预计 2-3 天）
4. **第四阶段**：性能优化和监控（预计 1-2 天）

### 11.3 预期效果
- 更好的用户体验：实时反馈 token 使用进度
- 更准确的成本控制：精确的用量监控
- 更强的可靠性：多维度触发保证数据及时性

通过本方案，Loom ACP 将具备业界领先的实时 token 使用监控能力，为用户提供更好的开发体验。