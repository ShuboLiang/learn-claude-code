use anyhow::Result;
use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::context::ContextService;
use tokio::sync::mpsc;

use crate::types::{Message, Part, Role, Task, TaskState, TaskStatus};

pub async fn run_task(
    task_id: String,
    context_id: String,
    user_input: String,
    agent: AgentApp,
    event_tx: mpsc::Sender<AgentEvent>,
) -> Result<(Task, ContextService)> {
    let mut ctx = ContextService::new();

    let result = agent
        .handle_user_turn(&mut ctx, &user_input, event_tx)
        .await;

    let (state, final_text) = match result {
        Ok(text) => (TaskState::Completed, text),
        Err(e) => {
            let msg = e.to_string();
            (TaskState::Failed, msg.clone())
        }
    };

    let final_message = Some(Message {
        message_id: uuid::Uuid::new_v4().to_string(),
        context_id: Some(context_id.clone()),
        task_id: Some(task_id.clone()),
        role: Role::Agent,
        parts: vec![Part::Text { text: final_text.clone() }],
        metadata: None,
        ..Default::default()
    });

    let task = Task {
        id: task_id.clone(),
        context_id: Some(context_id.clone()),
        status: TaskStatus {
            state,
            message: final_message,
            timestamp: Some(chrono::Utc::now()),
        },
        history: Some(vec![Message {
            message_id: uuid::Uuid::new_v4().to_string(),
            context_id: Some(context_id.clone()),
            task_id: Some(task_id.clone()),
            role: Role::Agent,
            parts: vec![Part::Text { text: final_text }],
            metadata: None,
            ..Default::default()
        }]),
        artifacts: None,
        metadata: None,
    };

    Ok((task, ctx))
}
