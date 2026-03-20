//! Session 数据结构和状态管理

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::tui::models::{Message, MessageId};

/// Session ID 类型
pub type SessionId = String;

/// Agent ID 类型
pub type AgentId = String;

/// Session 状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// 活跃中
    Active,
    /// 已暂停
    Paused,
    /// 已完成
    Completed,
    /// 出错
    Error,
}

/// Session 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session 唯一标识
    pub id: SessionId,
    /// Session 名称
    pub name: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,
    /// 关联的 Agent ID
    pub agent_id: Option<AgentId>,
    /// Session 状态
    pub status: SessionStatus,
    /// 消息总数
    pub message_count: usize,
}

impl Session {
    /// 创建新的 Session
    pub fn new(id: SessionId, name: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            created_at: now,
            updated_at: now,
            agent_id: None,
            status: SessionStatus::Active,
            message_count: 0,
        }
    }

    /// 关联 Agent
    pub fn with_agent(mut self, agent_id: AgentId) -> Self {
        self.agent_id = Some(agent_id);
        self
    }
}

/// Session 会话状态（包含消息历史）
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Session 元数据
    pub session: Session,
    /// 消息列表
    pub messages: Vec<Message>,
    /// 消息索引（用于快速查找）
    pub message_index: HashMap<MessageId, usize>,
    /// 滚动偏移量
    pub scroll_offset: usize,
    /// 选中的消息 ID（用于查看详情）
    pub selected_message: Option<MessageId>,
    /// 折叠的消息 ID 集合
    pub collapsed_messages: Vec<MessageId>,
}

impl SessionState {
    /// 创建新的 Session 状态
    pub fn new(session: Session) -> Self {
        Self {
            session,
            messages: Vec::new(),
            message_index: HashMap::new(),
            scroll_offset: 0,
            selected_message: None,
            collapsed_messages: Vec::new(),
        }
    }

    /// 添加消息
    pub fn add_message(&mut self, message: Message) {
        let id = message.id.clone();
        self.messages.push(message);
        self.message_index.insert(id, self.messages.len() - 1);
        self.session.message_count = self.messages.len();
        self.session.updated_at = Utc::now();
    }

    /// 更新消息
    pub fn update_message(&mut self, message_id: MessageId, message: Message) -> bool {
        if let Some(&idx) = self.message_index.get(&message_id) {
            if idx < self.messages.len() {
                self.messages[idx] = message;
                self.session.updated_at = Utc::now();
                return true;
            }
        }
        false
    }

    /// 获取消息
    pub fn get_message(&self, id: &MessageId) -> Option<&Message> {
        self.message_index.get(id).and_then(|&idx| self.messages.get(idx))
    }

    /// 切换消息折叠状态
    pub fn toggle_collapse(&mut self, message_id: MessageId) {
        if let Some(pos) = self.collapsed_messages.iter().position(|id| id == &message_id) {
            self.collapsed_messages.remove(pos);
        } else {
            self.collapsed_messages.push(message_id);
        }
    }

    /// 检查消息是否折叠
    pub fn is_collapsed(&self, message_id: &MessageId) -> bool {
        self.collapsed_messages.contains(message_id)
    }

    /// 向上滚动
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// 向下滚动
    pub fn scroll_down(&mut self, amount: usize, max_height: usize) {
        let max_offset = self.messages.len().saturating_sub(max_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_offset);
    }

    /// 滚动到底部
    pub fn scroll_to_bottom(&mut self, max_height: usize) {
        let max_offset = self.messages.len().saturating_sub(max_height);
        self.scroll_offset = max_offset;
    }

    /// 滚动到顶部
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::models::{Message, MessageRole, MessageContent};

    #[test]
    fn test_session_creation() {
        let session = Session::new("test-session".to_string(), "Test".to_string());
        assert_eq!(session.id, "test-session");
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.message_count, 0);
    }

    #[test]
    fn test_session_state_messages() {
        let session = Session::new("test".to_string(), "Test".to_string());
        let mut state = SessionState::new(session);
        
        let msg = Message::text(MessageRole::User, "Hello".to_string());
        let msg_id = msg.id.clone();
        
        state.add_message(msg);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.session.message_count, 1);
        assert!(state.get_message(&msg_id).is_some());
    }

    #[test]
    fn test_toggle_collapse() {
        let session = Session::new("test".to_string(), "Test".to_string());
        let mut state = SessionState::new(session);
        
        let msg = Message::text(MessageRole::Assistant, "Test".to_string());
        let msg_id = msg.id.clone();
        state.add_message(msg);
        
        assert!(!state.is_collapsed(&msg_id));
        state.toggle_collapse(msg_id.clone());
        assert!(state.is_collapsed(&msg_id));
        state.toggle_collapse(msg_id);
        assert!(!state.is_collapsed(&msg_id));
    }

    #[test]
    fn test_scrolling() {
        let session = Session::new("test".to_string(), "Test".to_string());
        let mut state = SessionState::new(session);
        
        // 添加 20 条消息
        for i in 0..20 {
            let msg = Message::text(MessageRole::User, format!("Message {}", i));
            state.add_message(msg);
        }
        
        // 测试滚动
        state.scroll_down(5, 10);
        assert_eq!(state.scroll_offset, 5);
        
        state.scroll_up(2);
        assert_eq!(state.scroll_offset, 3);
        
        state.scroll_to_bottom(10);
        assert_eq!(state.scroll_offset, 10);
        
        state.scroll_to_top();
        assert_eq!(state.scroll_offset, 0);
    }
}
