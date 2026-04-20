use dashmap::DashMap;
use std::sync::Arc;

use crate::types::{AgentCard, Task};

#[derive(Clone)]
pub struct AppState {
    pub tasks: Arc<DashMap<String, TaskState>>,
    pub agent_card: AgentCard,
}

#[derive(Debug, Clone)]
pub enum TaskState {
    Running { task: Task },
    Completed(Task),
    Failed { task: Task, error: String },
    Canceled,
}
