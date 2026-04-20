use dashmap::DashMap;
use std::sync::Arc;

use crate::types::{AgentCard, Task};

#[derive(Clone)]
pub struct AppState {
    pub tasks: Arc<DashMap<String, TaskState>>,
    pub contexts: Arc<DashMap<String, rust_agent_core::context::ContextService>>,
    pub agent: rust_agent_core::agent::AgentApp,
    pub agent_card: AgentCard,
}

#[derive(Debug, Clone)]
pub enum TaskState {
    Running { task: Task },
    Completed(Task),
    Failed { task: Task, error: String },
    Canceled,
}
