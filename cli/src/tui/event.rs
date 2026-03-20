use std::time::Duration;

use crossterm::event::{Event, KeyEvent, poll, read};
use tokio::sync::mpsc;

/// TUI 事件类型
///
/// 表示终端用户界面中可能发生的各种事件
#[derive(Debug)]
pub enum TuiEvent {
    /// 新代理启动
    AgentStarted {
        id: String,
        name: String,
        task: String,
    },
    /// 代理进度更新
    AgentProgress {
        id: String,
        node: String,
        message: String,
    },
    /// 代理完成
    AgentCompleted {
        id: String,
        result: String,
    },
    /// 代理错误
    AgentError {
        id: String,
        error: String,
    },
    /// 终端 tick 事件，用于 UI 刷新
    Tick,
    /// 用户退出
    Quit,
    /// 键盘事件
    Key(KeyEvent),
    /// 用户输入文本（Enter 提交）
    InputSubmitted(String),
    /// 用户消息已加入会话
    UserMessageAdded {
        content: String,
    },
    /// 助手消息开始
    AssistantMessageStarted {
        agent_id: String,
        message_id: String,
    },
    /// 助手消息增量
    AssistantMessageChunk {
        agent_id: String,
        message_id: String,
        chunk: String,
    },
    /// 助手消息结束
    AssistantMessageCompleted {
        agent_id: String,
        message_id: String,
    },
    /// 思考消息开始
    ThinkingStarted {
        agent_id: String,
        message_id: String,
    },
    /// 思考消息增量
    ThinkingChunk {
        agent_id: String,
        message_id: String,
        chunk: String,
    },
    /// 思考消息结束
    ThinkingCompleted {
        agent_id: String,
        message_id: String,
    },
    /// 工具调用开始
    ToolCallStarted {
        agent_id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    /// 工具输出增量
    ToolCallOutput {
        agent_id: String,
        call_id: String,
        content: String,
    },
    /// 工具调用结束
    ToolCallCompleted {
        agent_id: String,
        call_id: String,
        result: String,
        is_error: bool,
    },
}

/// 事件通道
///
/// 用于在系统各部分之间传递 TUI 事件
pub struct EventChannel {
    sender: mpsc::UnboundedSender<TuiEvent>,
}

impl EventChannel {
    /// 创建新的事件通道
    ///
    /// # Returns
    /// 返回 (EventChannel, receiver) 元组，其中 receiver 用于接收事件
    pub fn new() -> (Self, mpsc::UnboundedReceiver<TuiEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }

    /// 获取发送器的引用
    ///
    /// # Returns
    /// 返回发送器的引用，用于发送事件
    pub fn sender(&self) -> &mpsc::UnboundedSender<TuiEvent> {
        &self.sender
    }
}

/// 事件处理器
///
/// 从 crossterm 读取事件并转换为 TuiEvent
pub struct EventHandler {
    receiver: mpsc::UnboundedReceiver<TuiEvent>,
}

impl EventHandler {
    /// 创建新的事件处理器，启动后台事件轮询任务
    ///
    /// # Arguments
    /// * `tick_rate` - Tick 事件的生成间隔
    ///
    /// # Returns
    /// 返回 EventHandler 实例，后台任务自动开始轮询事件
    pub fn new(tick_rate: Duration) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self::spawn_event_loop(sender, tick_rate);
        Self { receiver }
    }

    /// 使用外部 sender 创建事件处理器
    ///
    /// # Arguments
    /// * `tick_rate` - Tick 事件的生成间隔
    /// * `sender` - 外部事件发送器
    pub fn with_sender(tick_rate: Duration, sender: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self::spawn_event_loop(sender, tick_rate);
        Self { 
            receiver: mpsc::unbounded_channel().1  // dummy receiver, not used
        }
    }

    /// 启动事件轮询循环
    fn spawn_event_loop(sender: mpsc::UnboundedSender<TuiEvent>, tick_rate: Duration) {
        tokio::spawn(async move {
            loop {
                // crossterm::event::poll 是同步阻塞调用
                // 在异步任务中直接调用可能会阻塞 tokio 运行时
                // 但对于简单的 TUI 应用这是可接受的
                match poll(tick_rate) {
                    Ok(true) => {
                        // 有事件可读，读取并处理
                        if let Ok(event) = read() {
                            match event {
                                Event::Key(key_event) => {
                                    // 在 Windows 上，crossterm 会报告 KeyPress 和 KeyRelease 两个事件
                                    // 只处理 Press 事件，避免重复
                                    if key_event.kind == crossterm::event::KeyEventKind::Press {
                                        // 如果发送失败（接收端已关闭），退出循环
                                        if sender.send(TuiEvent::Key(key_event)).is_err() {
                                            break;
                                        }
                                    }
                                }
                                Event::Resize(_, _) => {
                                    // 可选：处理终端大小变化
                                    // 目前忽略
                                }
                                _ => {
                                    // 其他事件类型忽略
                                }
                            }
                        }
                    }
                    Ok(false) => {
                        // poll 超时，发送 Tick 事件
                        if sender.send(TuiEvent::Tick).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        // poll 错误，继续轮询
                        continue;
                    }
                }
            }
        });
    }

    /// 等待下一个事件（异步）
    ///
    /// # Returns
    /// * `Some(TuiEvent)` - 收到事件
    /// * `None` - 发送端已关闭
    pub async fn next(&mut self) -> Option<TuiEvent>{
        self.receiver.recv().await
    }

    /// 等待下一个事件（同步，阻塞）
    ///
    /// # Returns
    /// * `Ok(TuiEvent)` - 收到事件
    /// * `Err(...)` - 发送端已关闭或错误
    pub fn next_event(&mut self) -> Result<TuiEvent, Box<dyn std::error::Error + Send + Sync>> {
        // 使用 tokio::runtime::Runtime 来在同步上下文中运行异步代码
        // 或者直接使用 recv() 方法（如果 receiver 支持）
        // 这里我们使用 try_recv() 来非阻塞地检查是否有事件
        self.receiver.try_recv().map_err(|e| format!("Event receive error: {}", e).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_event_channel_send_receive() {
        let (channel, mut receiver) = EventChannel::new();

        let event = TuiEvent::AgentStarted {
            id: "test-agent".to_string(),
            name: "Test Agent".to_string(),
            task: "Test task".to_string(),
        };

        channel.sender().send(event).unwrap();

        let received = receiver.recv().await.unwrap();

        // 由于 TuiEvent 不再实现 Clone，我们需要手动比较字段
        match received {
            TuiEvent::AgentStarted { id, name, task } => {
                assert_eq!(id, "test-agent");
                assert_eq!(name, "Test Agent");
                assert_eq!(task, "Test task");
            }
            _ => panic!("Expected AgentStarted event"),
        }
    }

    #[tokio::test]
    async fn test_key_event_mapping() {
        let (channel, mut receiver) = EventChannel::new();

        // 创建一个简单的键盘事件
        let key_event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let event = TuiEvent::Key(key_event);

        channel.sender().send(event).unwrap();

        let received = receiver.recv().await.unwrap();

        match received {
            TuiEvent::Key(ke) => {
                assert_eq!(ke.code, KeyCode::Char('q'));
                assert_eq!(ke.modifiers, KeyModifiers::NONE);
            }
            _ => panic!("Expected Key event"),
        }
    }

    #[tokio::test]
    async fn test_event_handler_creation() {
        // 测试 EventHandler 能够成功创建
        // 注意：不实际等待事件，因为 poll 在非终端环境可能阻塞
        let tick_rate = Duration::from_millis(100);
        let handler = EventHandler::new(tick_rate);

        // 验证 handler 能够成功创建
        // 通过 drop 来确保资源正确释放
        drop(handler);
    }

    #[tokio::test]
    async fn test_multiple_events() {
        let (channel, mut receiver) = EventChannel::new();

        // 发送多个事件
        channel.sender().send(TuiEvent::Tick).unwrap();
        channel.sender().send(TuiEvent::Quit).unwrap();
        channel
            .sender()
            .send(TuiEvent::AgentCompleted {
                id: "agent-1".to_string(),
                result: "Success".to_string(),
            })
            .unwrap();

        // 验证按顺序接收
        let event1 = receiver.recv().await.unwrap();
        assert!(matches!(event1, TuiEvent::Tick));

        let event2 = receiver.recv().await.unwrap();
        assert!(matches!(event2, TuiEvent::Quit));

        let event3 = receiver.recv().await.unwrap();
        match event3 {
            TuiEvent::AgentCompleted { id, result } => {
                assert_eq!(id, "agent-1");
                assert_eq!(result, "Success");
            }
            _ => panic!("Expected AgentCompleted event"),
        }
    }

    #[tokio::test]
    async fn test_agent_progress_event() {
        let (channel, mut receiver) = EventChannel::new();

        let event = TuiEvent::AgentProgress {
            id: "agent-2".to_string(),
            node: "processing".to_string(),
            message: "Working on task".to_string(),
        };

        channel.sender().send(event).unwrap();

        let received = receiver.recv().await.unwrap();

        match received {
            TuiEvent::AgentProgress { id, node, message } => {
                assert_eq!(id, "agent-2");
                assert_eq!(node, "processing");
                assert_eq!(message, "Working on task");
            }
            _ => panic!("Expected AgentProgress event"),
        }
    }

    #[tokio::test]
    async fn test_agent_error_event() {
        let (channel, mut receiver) = EventChannel::new();

        let event = TuiEvent::AgentError {
            id: "agent-3".to_string(),
            error: "Something went wrong".to_string(),
        };

        channel.sender().send(event).unwrap();

        let received = receiver.recv().await.unwrap();

        match received {
            TuiEvent::AgentError { id, error } => {
                assert_eq!(id, "agent-3");
                assert_eq!(error, "Something went wrong");
            }
            _ => panic!("Expected AgentError event"),
        }
    }

    #[tokio::test]
    async fn test_channel_sender_reference() {
        // 测试 sender() 方法返回的引用可以正常使用
        let (channel, mut receiver) = EventChannel::new();

        // 使用引用发送事件
        channel.sender().send(TuiEvent::Tick).unwrap();

        let received = receiver.recv().await.unwrap();
        assert!(matches!(received, TuiEvent::Tick));
    }
}
