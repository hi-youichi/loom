//! Session 相关数据结构
//!
//! 定义 Session、Message 等核心数据类型

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use super::ToolCallStatus;

/// Session ID
pub type SessionId = String;

/// Agent ID
pub type AgentId = String;

/// Session 消息 ID (不同于 Message 模块的 MessageId)
pub type SessionMessageId = uuid::Uuid;

/// 时间戳
pub type Timestamp = DateTime<Utc>;

/// Session 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub agent_id: Option<AgentId>,
    pub message_count: usize,
}

/// Session 消息类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionMessageType {
    /// 用户消息
    User,
    /// 助手消息
    Assistant,
    /// 思考过程
    Think,
    /// 工具调用
    Tool,
    /// 系统消息
    System,
}

impl SessionMessageType {
    /// 获取消息类型的显示图标
    pub fn icon(&self) -> &'static str {
        match self {
            SessionMessageType::User => "👤",
            SessionMessageType::Assistant => "🤖",
            SessionMessageType::Think => "💭",
            SessionMessageType::Tool => "🔧",
            SessionMessageType::System => "ℹ️",
        }
    }

    /// 获取消息类型的显示名称
    pub fn display_name(&self) -> &'static str {
        match self {
            SessionMessageType::User => "User",
            SessionMessageType::Assistant => "Assistant",
            SessionMessageType::Think => "Think",
            SessionMessageType::Tool => "Tool",
            SessionMessageType::System => "System",
        }
    }

    /// 是否默认折叠
    pub fn default_collapsed(&self) -> bool {
        matches!(self, SessionMessageType::Think | SessionMessageType::Tool)
    }
}

/// Session 工具调用详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToolCall {
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub result: Option<String>,
    pub duration_ms: Option<u64>,
    pub status: ToolCallStatus,
}

/// Session 工具调用状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionToolCallStatus {
    Pending,
    Running,
    Success,
    Error,
}

/// Session 消息内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessageContent {
    /// 文本内容
    pub text: String,
    /// 工具调用（可选）
    pub tool_call: Option<SessionToolCall>,
}

/// Session 消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: SessionMessageId,
    pub session_id: SessionId,
    pub msg_type: SessionMessageType,
    pub content: SessionMessageContent,
    pub timestamp: Timestamp,
    /// 是否折叠
    pub collapsed: bool,
    /// 元数据
    pub metadata: Option<serde_json::Value>,
}

impl SessionMessage {
    /// 创建新消息
    pub fn new(
        session_id: SessionId,
        msg_type: SessionMessageType,
        text: String,
    ) -> Self {
        let collapsed = msg_type.default_collapsed();
        Self {
            id: SessionMessageId::new_v4(),
            session_id,
            msg_type,
            content: SessionMessageContent {
                text,
                tool_call: None,
            },
            timestamp: Utc::now(),
            collapsed,
            metadata: None,
        }
    }

    /// 创建带工具调用的消息
    pub fn with_tool_call(
        session_id: SessionId,
        text: String,
        tool_call: SessionToolCall,
    ) -> Self {
        Self {
            id: SessionMessageId::new_v4(),
            session_id,
            msg_type: SessionMessageType::Tool,
            content: SessionMessageContent {
                text,
                tool_call: Some(tool_call),
            },
            timestamp: Utc::now(),
            collapsed: true,
            metadata: None,
        }
    }

    /// 切换折叠状态
    pub fn toggle_collapse(&mut self) {
        self.collapsed = !self.collapsed;
    }

    /// 获取显示内容（考虑折叠状态）
    pub fn display_text(&self) -> String {
        if self.collapsed {
            // 折叠时只显示第一行
            self.content.text.lines().next().unwrap_or("").to_string()
        } else {
            self.content.text.clone()
        }
    }

    /// 获取内容行数（考虑折叠）
    pub fn line_count(&self, max_width: usize) -> usize {
        let text = self.display_text();
        if max_width == 0 {
            return text.lines().count();
        }
        
        text.lines()
            .map(|line| {
                let width = unicode_width::UnicodeWidthStr::width(line);
                (width + max_width - 1) / max_width
            })
            .sum()
    }
}

/// Session 状态
#[derive(Debug, Clone, Default)]
pub struct SessionState {
    /// 当前活动的 Session
    pub active_session: Option<SessionId>,
    /// 所有 Session 的消息
    pub messages: Vec<SessionMessage>,
    /// Session 元数据
    pub sessions: std::collections::HashMap<SessionId, Session>,
    /// 滚动偏移
    pub scroll_offset: usize,
    /// 选中的消息索引
    pub selected_message: Option<usize>,
}

impl SessionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建新 Session
    pub fn create_session(&mut self, name: String, agent_id: Option<AgentId>) -> SessionId {
        let session_id = format!("session-{}", uuid::Uuid::new_v4());
        let now = Utc::now();
        
        let session = Session {
            id: session_id.clone(),
            name,
            created_at: now,
            updated_at: now,
            agent_id,
            message_count: 0,
        };
        
        self.sessions.insert(session_id.clone(), session);
        session_id
    }

    /// 添加消息
    pub fn add_message(&mut self, message: SessionMessage) {
        let session_id = message.session_id.clone();
        self.messages.push(message);
        
        // 更新 Session 计数
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.message_count += 1;
            session.updated_at = Utc::now();
        }
    }

    /// 获取当前 Session 的消息
    pub fn current_messages(&self) -> Vec<&SessionMessage> {
        if let Some(session_id) = &self.active_session {
            self.messages
                .iter()
                .filter(|msg| &msg.session_id == session_id)
                .collect()
        } else {
            vec![]
        }
    }

    /// 切换消息折叠状态
    pub fn toggle_message_collapse(&mut self, message_id: SessionMessageId) {
        if let Some(message) = self.messages.iter_mut().find(|m| m.id == message_id) {
            message.toggle_collapse();
        }
    }
}
