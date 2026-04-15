use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rust_agent_core::agent::AgentApp;
use rust_agent_core::context::ContextService;

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub context: ContextService,
    pub agent: Arc<AgentApp>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<DashMap<String, Session>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self { sessions: Arc::new(DashMap::new()) }
    }

    pub fn create(&self, agent: Arc<AgentApp>) -> Session {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let session = Session {
            id: id.clone(),
            context: ContextService::new(),
            agent,
            created_at: now,
            last_active: now,
        };
        self.sessions.insert(id, session.clone());
        session
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).map(|r| r.value().clone())
    }

    pub fn update(&self, id: &str, context: ContextService) {
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.context = context;
            session.last_active = Utc::now();
        }
    }

    pub fn remove(&self, id: &str) -> bool {
        self.sessions.remove(id).is_some()
    }
}
