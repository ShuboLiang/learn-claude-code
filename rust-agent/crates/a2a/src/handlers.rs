use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use rust_agent_core::agent::AgentEvent;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::state::{AppState, TaskState};
use crate::task_runner::run_task;
use crate::types::{Message, Part, Role, Task, TaskStatus};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTaskRequest {
    pub id: String,
    pub session_id: Option<String>,
    pub message: Message,
    pub metadata: Option<serde_json::Value>,
}

pub async fn get_agent_card(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(state.agent_card.clone())
}

pub async fn send_task(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SendTaskRequest>,
) -> impl IntoResponse {
    if state.tasks.contains_key(&body.id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": {
                    "code": "task_exists",
                    "message": format!("Task {} already exists", body.id)
                }
            })),
        )
            .into_response();
    }

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "unsupported_part_type",
                        "message": e
                    }
                })),
            )
                .into_response();
        }
    };

    let agent = state.agent.clone();

    let placeholder = Task {
        id: body.id.clone(),
        session_id: body.session_id.clone(),
        status: TaskStatus::Submitted,
        history: vec![],
        artifacts: vec![],
        metadata: body.metadata.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.tasks.insert(
        body.id.clone(),
        TaskState::Running {
            task: placeholder.clone(),
        },
    );

    let (event_tx, _event_rx) = mpsc::channel(64);
    let result = run_task(body.id.clone(), user_input, agent, event_tx).await;

    match result {
        Ok((mut task, ctx)) => {
            task.updated_at = chrono::Utc::now();
            state.contexts.insert(body.id.clone(), ctx);
            state
                .tasks
                .insert(body.id.clone(), TaskState::Completed(task.clone()));
            (StatusCode::OK, Json(task)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let failed = Task {
                id: body.id.clone(),
                session_id: body.session_id,
                status: TaskStatus::Failed {
                    message: msg.clone(),
                },
                history: vec![Message {
                    role: Role::Agent,
                    parts: vec![Part::Text { text: msg.clone() }],
                }],
                artifacts: vec![],
                metadata: body.metadata,
                created_at: placeholder.created_at,
                updated_at: chrono::Utc::now(),
            };
            state.contexts.insert(body.id.clone(), rust_agent_core::context::ContextService::new());
            state.tasks.insert(
                body.id.clone(),
                TaskState::Failed {
                    task: failed.clone(),
                    error: msg,
                },
            );
            (StatusCode::OK, Json(failed)).into_response()
        }
    }
}

use axum::response::sse::Sse;
use tokio_stream::wrappers::ReceiverStream;

use crate::streaming::agent_event_to_a2a;

pub async fn send_task_subscribe(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SendTaskRequest>,
) -> impl IntoResponse {
    if state.tasks.contains_key(&body.id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": {
                    "code": "task_exists",
                    "message": format!("Task {} already exists", body.id)
                }
            })),
        )
            .into_response();
    }

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "unsupported_part_type",
                        "message": e
                    }
                })),
            )
                .into_response();
        }
    };

    let agent = state.agent.clone();
    let task_id = body.id.clone();

    let placeholder = Task {
        id: task_id.clone(),
        session_id: body.session_id.clone(),
        status: TaskStatus::Submitted,
        history: vec![],
        artifacts: vec![],
        metadata: body.metadata.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    state.tasks.insert(
        task_id.clone(),
        TaskState::Running {
            task: placeholder.clone(),
        },
    );

    let (agent_event_tx, mut agent_event_rx) = mpsc::channel::<AgentEvent>(64);
    let (sse_tx, sse_rx) =
        mpsc::channel::<Result<axum::response::sse::Event, std::convert::Infallible>>(64);

    let runner_task_id = task_id.clone();
    let runner_state = Arc::clone(&state);
    let runner_session_id = body.session_id.clone();
    let runner_metadata = body.metadata.clone();
    let runner_created_at = placeholder.created_at;
    tokio::spawn(async move {
        let result = run_task(runner_task_id.clone(), user_input, agent, agent_event_tx).await;
        match result {
            Ok((task, ctx)) => {
                runner_state.contexts.insert(runner_task_id.clone(), ctx);
                runner_state
                    .tasks
                    .insert(runner_task_id, TaskState::Completed(task));
            }
            Err(e) => {
                let msg = e.to_string();
                let failed = Task {
                    id: runner_task_id.clone(),
                    session_id: runner_session_id,
                    status: TaskStatus::Failed {
                        message: msg.clone(),
                    },
                    history: vec![Message {
                        role: Role::Agent,
                        parts: vec![Part::Text { text: msg.clone() }],
                    }],
                    artifacts: vec![],
                    metadata: runner_metadata,
                    created_at: runner_created_at,
                    updated_at: chrono::Utc::now(),
                };
                runner_state.contexts.insert(runner_task_id.clone(), rust_agent_core::context::ContextService::new());
                runner_state.tasks.insert(
                    runner_task_id,
                    TaskState::Failed {
                        task: failed,
                        error: msg,
                    },
                );
            }
        }
    });

    let bridge_task_id = task_id.clone();
    tokio::spawn(async move {
        let mut artifact_counter = 0u32;
        while let Some(agent_event) = agent_event_rx.recv().await {
            let payloads = agent_event_to_a2a(
                &bridge_task_id, agent_event, &mut artifact_counter);
            for payload in payloads {
                let event = payload.into_sse_event();
                if sse_tx.send(Ok(event)).await.is_err() {
                    break;
                }
            }
        }
    });

    let stream = ReceiverStream::new(sse_rx);
    Sse::new(stream).into_response()
}

pub async fn send_task_followup(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(body): Json<SendTaskRequest>,
) -> impl IntoResponse {
    if body.id != task_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": "task_id_mismatch",
                    "message": "URL 中的 taskId 与 body 中的 id 不一致"
                }
            })),
        )
            .into_response();
    }

    if !state.tasks.contains_key(&task_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "code": "task_not_found",
                    "message": format!("Task {} not found", task_id)
                }
            })),
        )
            .into_response();
    }

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "code": "unsupported_part_type",
                        "message": e
                    }
                })),
            )
                .into_response();
        }
    };

    let mut ctx = state
        .contexts
        .get(&task_id)
        .map(|entry| entry.value().clone())
        .unwrap_or_default();

    let agent = state.agent.clone();
    let (event_tx, _event_rx) = mpsc::channel(64);
    let result = agent.handle_user_turn(&mut ctx, &user_input, event_tx).await;

    let now = chrono::Utc::now();
    match result {
        Ok(text) => {
            let task = Task {
                id: task_id.clone(),
                session_id: body.session_id,
                status: TaskStatus::Completed,
                history: vec![Message {
                    role: Role::Agent,
                    parts: vec![Part::Text { text: text.clone() }],
                }],
                artifacts: vec![],
                metadata: body.metadata,
                created_at: now,
                updated_at: now,
            };
            state.contexts.insert(task_id.clone(), ctx);
            state.tasks.insert(task_id, TaskState::Completed(task.clone()));
            (StatusCode::OK, Json(task)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let failed = Task {
                id: task_id.clone(),
                session_id: body.session_id,
                status: TaskStatus::Failed {
                    message: msg.clone(),
                },
                history: vec![Message {
                    role: Role::Agent,
                    parts: vec![Part::Text { text: msg.clone() }],
                }],
                artifacts: vec![],
                metadata: body.metadata,
                created_at: now,
                updated_at: now,
            };
            state.contexts.insert(task_id.clone(), ctx);
            state.tasks.insert(
                task_id,
                TaskState::Failed {
                    task: failed.clone(),
                    error: msg,
                },
            );
            (StatusCode::OK, Json(failed)).into_response()
        }
    }
}

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.tasks.get(&task_id) {
        Some(entry) => {
            let task = match entry.value() {
                TaskState::Running { task } => task.clone(),
                TaskState::Completed(task) => task.clone(),
                TaskState::Failed { task, .. } => task.clone(),
                TaskState::Canceled => Task {
                    id: task_id,
                    session_id: None,
                    status: TaskStatus::Canceled,
                    history: vec![],
                    artifacts: vec![],
                    metadata: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                },
            };
            (StatusCode::OK, Json(task)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "code": "task_not_found",
                    "message": format!("Task {} not found", task_id)
                }
            })),
        )
            .into_response(),
    }
}

pub async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if state.tasks.contains_key(&task_id) {
        state.tasks.insert(task_id, TaskState::Canceled);
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "code": "task_not_found",
                    "message": format!("Task {} not found", task_id)
                }
            })),
        )
            .into_response()
    }
}

fn extract_user_input(message: &Message) -> Result<String, String> {
    let mut texts = Vec::new();
    for part in &message.parts {
        match part {
            Part::Text { text } => texts.push(text.clone()),
            Part::File { file } => {
                if let Some(uri) = &file.uri {
                    texts.push(format!("[File: {}]", uri));
                } else {
                    return Err("File parts with bytes are not supported in MVP".to_string());
                }
            }
            Part::Data { .. } => {
                return Err("Data parts are not supported in MVP".to_string());
            }
        }
    }
    if texts.is_empty() {
        return Err("Message contains no usable parts".to_string());
    }
    Ok(texts.join("\n"))
}
