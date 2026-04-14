use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rust_agent_core::agent::AgentApp;
use rust_agent_core::api::types::ApiMessage;

/// 会话数据，持有独立的 Agent 实例和消息历史
#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub messages: Vec<ApiMessage>,
    pub agent: Arc<AgentApp>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

/// 线程安全的会话存储（内存 DashMap，后续可替换为 trait + SQLite）
#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<DashMap<String, Session>>,
}

impl SessionStore {
    /// 创建空的会话存储
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    /// 创建新会话，返回包含唯一 ID 的 Session
    pub fn create(&self, agent: Arc<AgentApp>) -> Session {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let session = Session {
            id: id.clone(),
            messages: Vec::new(),
            agent,
            created_at: now,
            last_active: now,
        };
        self.sessions.insert(id, session.clone());
        session
    }

    /// 获取会话
    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).map(|r| r.value().clone())
    }

    /// 更新会话的消息列表和最后活跃时间
    pub fn update(&self, id: &str, messages: Vec<ApiMessage>) {
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.messages = messages;
            session.last_active = Utc::now();
        }
    }

    /// 删除会话
    pub fn remove(&self, id: &str) -> bool {
        self.sessions.remove(id).is_some()
    }
}
