use anyhow::Result;
use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::context::ContextService;
use tokio::sync::mpsc;

use crate::types::{Message, Part, Role, Task, TaskStatus};

pub async fn run_task(
    task_id: String,
    user_input: String,
    agent: AgentApp,
    event_tx: mpsc::Sender<AgentEvent>,
) -> Result<(Task, ContextService)> {
    let mut ctx = ContextService::new();

    let result = agent
        .handle_user_turn(&mut ctx, &user_input, event_tx)
        .await;

    let (status, final_text) = match result {
        Ok(text) => (TaskStatus::Completed, text),
        Err(e) => {
            let msg = e.to_string();
            (
                TaskStatus::Failed {
                    message: msg.clone(),
                },
                msg,
            )
        }
    };

    let now = chrono::Utc::now();
    let task = Task {
        id: task_id,
        session_id: None,
        status,
        history: vec![Message {
            role: Role::Agent,
            parts: vec![Part::Text { text: final_text }],
        }],
        artifacts: vec![],
        metadata: None,
        created_at: now,
        updated_at: now,
    };

    Ok((task, ctx))
}
