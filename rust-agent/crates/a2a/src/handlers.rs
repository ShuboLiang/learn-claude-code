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

use crate::state::{AppState, TaskState as AppTaskState};
use crate::task_runner::run_task;
use crate::types::{Message, Part, Role, Task, TaskState, TaskStatus};

// ───────────────────────────────────────────
// Request types
// ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<SendMessageConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_output_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_immediately: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_push_notification_config: Option<serde_json::Value>,
}

// ───────────────────────────────────────────
// GET /.well-known/agent.json
// ───────────────────────────────────────────

pub async fn get_agent_card(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(state.agent_card.clone())
}

// ───────────────────────────────────────────
// POST /message:send
// ───────────────────────────────────────────

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    // Multi-turn: if message contains taskId, continue existing task.
    if let Some(ref task_id) = body.message.task_id {
        return send_message_followup(state, task_id.clone(), body).await;
    }

    // New task: server-generated UUIDs.
    let task_id = uuid::Uuid::new_v4().to_string();
    let context_id = uuid::Uuid::new_v4().to_string();

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return a2a_error("ContentTypeNotSupportedError", &e);
        }
    };

    let agent = state.agent.clone();

    let placeholder = Task {
        id: task_id.clone(),
        context_id: Some(context_id.clone()),
        status: TaskStatus {
            state: TaskState::Submitted,
            message: None,
            timestamp: Some(chrono::Utc::now()),
        },
        history: None,
        artifacts: None,
        metadata: body.metadata.clone(),
    };
    state.tasks.insert(
        task_id.clone(),
        AppTaskState::Running {
            task: placeholder.clone(),
        },
    );

    // Transition to Working immediately.
    state.tasks.alter(&task_id, |_, mut ts| {
        if let AppTaskState::Running { ref mut task } = ts {
            task.status.state = TaskState::Working;
            task.status.timestamp = Some(chrono::Utc::now());
        }
        ts
    });

    let return_immediately = body.configuration.as_ref().and_then(|c| c.return_immediately) == Some(true);

    let broadcast_tx = get_or_create_broadcast(&state, &task_id);

    if return_immediately {
        // Non-blocking: spawn background work and return Working task immediately.
        let runner_task_id = task_id.clone();
        let runner_state = Arc::clone(&state);
        let runner_metadata = body.metadata.clone();
        let spawn_context_id = context_id.clone();
        let runner_broadcast_tx = broadcast_tx.clone();
        tokio::spawn(async move {
            let (event_tx, _event_rx) = mpsc::channel(64);
            let result = run_task(runner_task_id.clone(), spawn_context_id.clone(), user_input, agent, event_tx).await;
            match result {
                Ok((mut task, ctx)) => {
                    task.metadata = runner_metadata.clone();
                    runner_state.contexts.insert(runner_task_id.clone(), ctx);
                    runner_state
                        .tasks
                        .insert(runner_task_id.clone(), AppTaskState::Completed(task.clone()));
                    broadcast_status(&runner_broadcast_tx, &runner_task_id, &spawn_context_id, task.status.clone());
                }
                Err(e) => {
                    let msg = e.to_string();
                    let failed = Task {
                        id: runner_task_id.clone(),
                        context_id: Some(spawn_context_id.clone()),
                        status: TaskStatus {
                            state: TaskState::Failed,
                            message: Some(Message {
                                message_id: uuid::Uuid::new_v4().to_string(),
                                context_id: Some(spawn_context_id.clone()),
                                task_id: Some(runner_task_id.clone()),
                                role: Role::Agent,
                                parts: vec![Part::Text { text: msg.clone() }],
                                metadata: None,
                                ..Default::default()
                            }),
                            timestamp: Some(chrono::Utc::now()),
                        },
                        history: None,
                        artifacts: None,
                        metadata: runner_metadata,
                    };
                    runner_state.contexts.insert(runner_task_id.clone(), rust_agent_core::context::ContextService::new());
                    runner_state.tasks.insert(
                        runner_task_id.clone(),
                        AppTaskState::Failed {
                            task: failed.clone(),
                            error: msg,
                        },
                    );
                    broadcast_status(&runner_broadcast_tx, &runner_task_id, &spawn_context_id, failed.status.clone());
                }
            }
        });

        let working_task = Task {
            id: task_id.clone(),
            context_id: Some(context_id),
            status: TaskStatus {
                state: TaskState::Working,
                message: None,
                timestamp: Some(chrono::Utc::now()),
            },
            history: None,
            artifacts: None,
            metadata: body.metadata,
        };
        state.tasks.insert(task_id.clone(), AppTaskState::Running { task: working_task.clone() });
        return (StatusCode::OK, Json(working_task)).into_response();
    }

    // Blocking path.
    let (event_tx, _event_rx) = mpsc::channel(64);
    let result = run_task(task_id.clone(), context_id.clone(), user_input, agent, event_tx).await;

    let broadcast_tx = get_or_create_broadcast(&state, &task_id);

    match result {
        Ok((mut task, ctx)) => {
            task.metadata = body.metadata.clone();
            task.context_id = Some(context_id.clone());
            state.contexts.insert(task_id.clone(), ctx);
            state
                .tasks
                .insert(task_id.clone(), AppTaskState::Completed(task.clone()));
            broadcast_status(&broadcast_tx, &task_id, &context_id, task.status.clone());
            (StatusCode::OK, Json(task)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let failed = Task {
                id: task_id.clone(),
                context_id: Some(context_id.clone()),
                status: TaskStatus {
                    state: TaskState::Failed,
                    message: Some(Message {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        context_id: Some(context_id.clone()),
                        task_id: Some(task_id.clone()),
                        role: Role::Agent,
                        parts: vec![Part::Text { text: msg.clone() }],
                        metadata: None,
                        ..Default::default()
                    }),
                    timestamp: Some(chrono::Utc::now()),
                },
                history: None,
                artifacts: None,
                metadata: body.metadata,
            };
            state.contexts.insert(task_id.clone(), rust_agent_core::context::ContextService::new());
            state.tasks.insert(
                task_id.clone(),
                AppTaskState::Failed {
                    task: failed.clone(),
                    error: msg,
                },
            );
            broadcast_status(&broadcast_tx, &task_id, &context_id, failed.status.clone());
            (StatusCode::OK, Json(failed)).into_response()
        }
    }
}

// ───────────────────────────────────────────
// Multi-turn follow-up (internal helper)
// ───────────────────────────────────────────

async fn send_message_followup(
    state: Arc<AppState>,
    task_id: String,
    body: SendMessageRequest,
) -> axum::response::Response {
    let existing_entry = match state.tasks.get(&task_id) {
        Some(e) => e,
        None => {
            return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
        }
    };

    let existing_task = match existing_entry.value() {
        AppTaskState::Running { task } => task.clone(),
        AppTaskState::Completed(task) => task.clone(),
        AppTaskState::Failed { task, .. } => task.clone(),
        AppTaskState::Canceled(task) => task.clone(),
    };

    // Reject messages to terminal-state tasks.
    let is_terminal = matches!(
        existing_task.status.state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    );
    if is_terminal {
        return a2a_error(
            "UnsupportedOperationError",
            "Task is already in a terminal state",
        );
    }

    // Validate contextId consistency if both are provided.
    if let (Some(req_ctx), Some(task_ctx)) = (&body.message.context_id, &existing_task.context_id) {
        if req_ctx != task_ctx {
            return a2a_error(
                "InvalidRequestError",
                "contextId does not match the referenced task",
            );
        }
    }

    let inferred_context_id = existing_task.context_id.clone();

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return a2a_error("ContentTypeNotSupportedError", &e);
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

    match result {
        Ok(text) => {
            let mut history = existing_task.history.clone().unwrap_or_default();
            let reply = Message {
                message_id: uuid::Uuid::new_v4().to_string(),
                context_id: inferred_context_id.clone(),
                task_id: Some(task_id.clone()),
                role: Role::Agent,
                parts: vec![Part::Text { text: text.clone() }],
                ..Default::default()
            };
            history.push(reply.clone());
            let task = Task {
                id: task_id.clone(),
                context_id: inferred_context_id.clone(),
                status: TaskStatus {
                    state: TaskState::Completed,
                    message: Some(reply),
                    timestamp: Some(chrono::Utc::now()),
                },
                history: Some(history),
                artifacts: None,
                metadata: body.metadata,
            };
            let broadcast_tx = get_or_create_broadcast(&state, &task_id);
            broadcast_status(&broadcast_tx, &task_id, inferred_context_id.as_deref().unwrap_or(""), task.status.clone());
            state.contexts.insert(task_id.clone(), ctx);
            state.tasks.insert(task_id.clone(), AppTaskState::Completed(task.clone()));
            (StatusCode::OK, Json(task)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let failed = Task {
                id: task_id.clone(),
                context_id: inferred_context_id.clone(),
                status: TaskStatus {
                    state: TaskState::Failed,
                    message: Some(Message {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        context_id: inferred_context_id.clone(),
                        task_id: Some(task_id.clone()),
                        role: Role::Agent,
                        parts: vec![Part::Text { text: msg.clone() }],
                        metadata: None,
                        ..Default::default()
                    }),
                    timestamp: Some(chrono::Utc::now()),
                },
                history: None,
                artifacts: None,
                metadata: body.metadata,
            };
            let broadcast_tx = get_or_create_broadcast(&state, &task_id);
            broadcast_status(&broadcast_tx, &task_id, inferred_context_id.as_deref().unwrap_or(""), failed.status.clone());
            state.contexts.insert(task_id.clone(), ctx);
            state.tasks.insert(
                task_id,
                AppTaskState::Failed {
                    task: failed.clone(),
                    error: msg,
                },
            );
            (StatusCode::OK, Json(failed)).into_response()
        }
    }
}

// ───────────────────────────────────────────
// POST /message:stream  (SSE)
// ───────────────────────────────────────────

use axum::response::sse::Sse;
use tokio_stream::wrappers::ReceiverStream;

use crate::streaming::agent_event_to_stream_response;

pub async fn send_message_stream(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    // Multi-turn streaming: if message.task_id is present, follow-up on existing task.
    if let Some(ref task_id) = body.message.task_id {
        if !state.tasks.contains_key(task_id) {
            return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
        }
        return stream_message_followup(state, task_id.clone(), body).await;
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    let context_id = uuid::Uuid::new_v4().to_string();

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return a2a_error("ContentTypeNotSupportedError", &e);
        }
    };

    let agent = state.agent.clone();
    let task_id_clone = task_id.clone();

    let placeholder = Task {
        id: task_id.clone(),
        context_id: Some(context_id.clone()),
        status: TaskStatus {
            state: TaskState::Submitted,
            message: None,
            timestamp: Some(chrono::Utc::now()),
        },
        history: None,
        artifacts: None,
        metadata: body.metadata.clone(),
    };
    state.tasks.insert(
        task_id.clone(),
        AppTaskState::Running {
            task: placeholder.clone(),
        },
    );

    // Transition shared state to Working so concurrent GET sees correct state.
    state.tasks.alter(&task_id, |_, mut ts| {
        if let AppTaskState::Running { ref mut task } = ts {
            task.status.state = TaskState::Working;
            task.status.timestamp = Some(chrono::Utc::now());
        }
        ts
    });

    let (agent_event_tx, mut agent_event_rx) = mpsc::channel::<AgentEvent>(64);
    let (sse_tx, sse_rx) =
        mpsc::channel::<Result<axum::response::sse::Event, std::convert::Infallible>>(64);

    // Emit initial Task snapshot as first SSE event per spec.
    let init_event = crate::types::StreamResponse {
        task: Some(placeholder.clone()),
        message: None,
        status_update: None,
        artifact_update: None,
    };
    let _ = sse_tx.send(Ok(init_event.into_sse_event())).await;

    let broadcast_tx = get_or_create_broadcast(&state, &task_id);

    let runner_task_id = task_id.clone();
    let runner_context_id = context_id.clone();
    let runner_state = Arc::clone(&state);
    let runner_metadata = body.metadata.clone();
    let runner_sse_tx = sse_tx.clone();
    let runner_broadcast_tx = broadcast_tx.clone();
    tokio::spawn(async move {
        let result = run_task(runner_task_id.clone(), runner_context_id.clone(), user_input, agent, agent_event_tx).await;
        match result {
            Ok((task, ctx)) => {
                runner_state.contexts.insert(runner_task_id.clone(), ctx);
                runner_state
                    .tasks
                    .insert(runner_task_id.clone(), AppTaskState::Completed(task.clone()));
                broadcast_status(&runner_broadcast_tx, &runner_task_id, &runner_context_id, task.status.clone());
            }
            Err(e) => {
                let msg = e.to_string();
                let failed = Task {
                    id: runner_task_id.clone(),
                    context_id: Some(runner_context_id.clone()),
                    status: TaskStatus {
                        state: TaskState::Failed,
                        message: Some(Message {
                            message_id: uuid::Uuid::new_v4().to_string(),
                            context_id: Some(runner_context_id.clone()),
                            task_id: Some(runner_task_id.clone()),
                            role: Role::Agent,
                            parts: vec![Part::Text { text: msg.clone() }],
                            metadata: None,
                            ..Default::default()
                        }),
                        timestamp: Some(chrono::Utc::now()),
                    },
                    history: Some(vec![Message {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        context_id: Some(runner_context_id.clone()),
                        task_id: Some(runner_task_id.clone()),
                        role: Role::Agent,
                        parts: vec![Part::Text { text: msg.clone() }],
                        metadata: None,
                        ..Default::default()
                    }]),
                    artifacts: None,
                    metadata: runner_metadata,
                };
                runner_state.contexts.insert(runner_task_id.clone(), rust_agent_core::context::ContextService::new());
                runner_state.tasks.insert(
                    runner_task_id.clone(),
                    AppTaskState::Failed {
                        task: failed.clone(),
                        error: msg.clone(),
                    },
                );

                let fail_event = crate::types::StreamResponse {
                    status_update: Some(crate::types::TaskStatusUpdateEvent {
                        task_id: runner_task_id.clone(),
                        context_id: runner_context_id.clone(),
                        status: failed.status.clone(),
                        metadata: None,
                    }),
                    task: None,
                    message: None,
                    artifact_update: None,
                };
                let _ = runner_sse_tx.send(Ok(fail_event.into_sse_event())).await;
                broadcast_status(&runner_broadcast_tx, &runner_task_id, &runner_context_id, failed.status.clone());
            }
        }
    });

    let bridge_task_id = task_id_clone.clone();
    let bridge_context_id = context_id.clone();
    let bridge_broadcast_tx = broadcast_tx.clone();
    tokio::spawn(async move {
        while let Some(agent_event) = agent_event_rx.recv().await {
            let responses = agent_event_to_stream_response(
                &bridge_task_id,
                &bridge_context_id,
                agent_event,
            );
            for resp in responses {
                let _ = bridge_broadcast_tx.send(resp.clone());
                let event = resp.into_sse_event();
                if sse_tx.send(Ok(event)).await.is_err() {
                    break;
                }
            }
        }
    });

    let stream = ReceiverStream::new(sse_rx);
    Sse::new(stream).into_response()
}

async fn stream_message_followup(
    state: Arc<AppState>,
    task_id: String,
    body: SendMessageRequest,
) -> axum::response::Response {
    // Reject messages to terminal-state tasks before opening SSE stream.
    let existing_task = state
        .tasks
        .get(&task_id)
        .and_then(|entry| match entry.value() {
            AppTaskState::Running { task } => Some(task.clone()),
            AppTaskState::Completed(task) => Some(task.clone()),
            AppTaskState::Failed { task, .. } => Some(task.clone()),
            AppTaskState::Canceled(task) => Some(task.clone()),
        });

    let existing_task = match existing_task {
        Some(t) => t,
        None => {
            return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
        }
    };

    let is_terminal = matches!(
        existing_task.status.state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    );
    if is_terminal {
        return a2a_error(
            "UnsupportedOperationError",
            "Task is already in a terminal state",
        );
    }

    let inferred_context_id = existing_task.context_id.clone();

    let user_input = match extract_user_input(&body.message) {
        Ok(text) => text,
        Err(e) => {
            return a2a_error("ContentTypeNotSupportedError", &e);
        }
    };

    let mut ctx = state
        .contexts
        .get(&task_id)
        .map(|entry| entry.value().clone())
        .unwrap_or_default();

    let agent = state.agent.clone();
    let (agent_event_tx, mut agent_event_rx) = mpsc::channel::<AgentEvent>(64);
    let (sse_tx, sse_rx) =
        mpsc::channel::<Result<axum::response::sse::Event, std::convert::Infallible>>(64);

    // Emit initial Task snapshot.
    let init_event = crate::types::StreamResponse {
        task: Some(existing_task),
        message: None,
        status_update: None,
        artifact_update: None,
    };
    let _ = sse_tx.send(Ok(init_event.into_sse_event())).await;

    let broadcast_tx = get_or_create_broadcast(&state, &task_id);

    let runner_task_id = task_id.clone();
    let runner_context_id = inferred_context_id.clone();
    let runner_state = Arc::clone(&state);
    let runner_metadata = body.metadata.clone();
    let runner_sse_tx = sse_tx.clone();
    let runner_broadcast_tx = broadcast_tx.clone();
    tokio::spawn(async move {
        let result = agent.handle_user_turn(&mut ctx, &user_input, agent_event_tx).await;

        let now = chrono::Utc::now();
        match result {
            Ok(text) => {
                let reply = Message {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    context_id: runner_context_id.clone(),
                    task_id: Some(runner_task_id.clone()),
                    role: Role::Agent,
                    parts: vec![Part::Text { text: text.clone() }],
                    metadata: None,
                    ..Default::default()
                };
                let task = Task {
                    id: runner_task_id.clone(),
                    context_id: runner_context_id.clone(),
                    status: TaskStatus {
                        state: TaskState::Completed,
                        message: Some(reply),
                        timestamp: Some(now),
                    },
                    history: Some(vec![Message {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        context_id: runner_context_id.clone(),
                        task_id: Some(runner_task_id.clone()),
                        role: Role::Agent,
                        parts: vec![Part::Text { text: text.clone() }],
                        metadata: None,
                        ..Default::default()
                    }]),
                    artifacts: None,
                    metadata: runner_metadata,
                };
                runner_state.contexts.insert(runner_task_id.clone(), ctx);
                runner_state
                    .tasks
                    .insert(runner_task_id.clone(), AppTaskState::Completed(task.clone()));

                let done_event = crate::types::StreamResponse {
                    status_update: Some(crate::types::TaskStatusUpdateEvent {
                        task_id: runner_task_id.clone(),
                        context_id: runner_context_id.clone().unwrap_or_default(),
                        status: task.status.clone(),
                        metadata: None,
                    }),
                    task: None,
                    message: None,
                    artifact_update: None,
                };
                let _ = runner_sse_tx.send(Ok(done_event.clone().into_sse_event())).await;
                let _ = runner_broadcast_tx.send(done_event);
            }
            Err(e) => {
                let msg = e.to_string();
                let failed = Task {
                    id: runner_task_id.clone(),
                    context_id: runner_context_id.clone(),
                    status: TaskStatus {
                        state: TaskState::Failed,
                        message: Some(Message {
                            message_id: uuid::Uuid::new_v4().to_string(),
                            context_id: runner_context_id.clone(),
                            task_id: Some(runner_task_id.clone()),
                            role: Role::Agent,
                            parts: vec![Part::Text { text: msg.clone() }],
                            metadata: None,
                            ..Default::default()
                        }),
                        timestamp: Some(now),
                    },
                    history: Some(vec![Message {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        context_id: runner_context_id.clone(),
                        task_id: Some(runner_task_id.clone()),
                        role: Role::Agent,
                        parts: vec![Part::Text { text: msg.clone() }],
                        metadata: None,
                        ..Default::default()
                    }]),
                    artifacts: None,
                    metadata: runner_metadata,
                };
                runner_state.contexts.insert(runner_task_id.clone(), ctx);
                runner_state.tasks.insert(
                    runner_task_id.clone(),
                    AppTaskState::Failed {
                        task: failed.clone(),
                        error: msg.clone(),
                    },
                );

                let fail_event = crate::types::StreamResponse {
                    status_update: Some(crate::types::TaskStatusUpdateEvent {
                        task_id: runner_task_id.clone(),
                        context_id: runner_context_id.clone().unwrap_or_default(),
                        status: failed.status.clone(),
                        metadata: None,
                    }),
                    task: None,
                    message: None,
                    artifact_update: None,
                };
                let _ = runner_sse_tx.send(Ok(fail_event.clone().into_sse_event())).await;
                let _ = runner_broadcast_tx.send(fail_event);
            }
        }
    });

    let bridge_task_id = task_id.clone();
    let bridge_context_id = inferred_context_id.clone().unwrap_or_default();
    let bridge_broadcast_tx = broadcast_tx.clone();
    tokio::spawn(async move {
        while let Some(agent_event) = agent_event_rx.recv().await {
            let responses = agent_event_to_stream_response(
                &bridge_task_id,
                &bridge_context_id,
                agent_event,
            );
            for resp in responses {
                let _ = bridge_broadcast_tx.send(resp.clone());
                let event = resp.into_sse_event();
                if sse_tx.send(Ok(event)).await.is_err() {
                    break;
                }
            }
        }
    });

    let stream = ReceiverStream::new(sse_rx);
    Sse::new(stream).into_response()
}

// ───────────────────────────────────────────
// GET /tasks/{taskId}
// ───────────────────────────────────────────

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    match state.tasks.get(&task_id) {
        Some(entry) => {
            let mut task = match entry.value() {
                AppTaskState::Running { task } => task.clone(),
                AppTaskState::Completed(task) => task.clone(),
                AppTaskState::Failed { task, .. } => task.clone(),
                AppTaskState::Canceled(task) => task.clone(),
            };
            // Apply historyLength limit if provided.
            if let Some(hl_str) = params.get("historyLength") {
                if let Ok(hl) = hl_str.parse::<usize>() {
                    if let Some(ref mut history) = task.history {
                        if history.len() > hl {
                            let start = history.len() - hl;
                            *history = history.split_off(start);
                        }
                    }
                }
            }
            (StatusCode::OK, Json(task)).into_response()
        }
        None => a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id)),
    }
}

// ───────────────────────────────────────────
// POST /tasks/{taskId}:cancel
// ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelTaskRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

pub async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(body): Json<CancelTaskRequest>,
) -> impl IntoResponse {
    let entry = match state.tasks.get(&task_id) {
        Some(e) => e,
        None => {
            return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
        }
    };

    let existing_task = match entry.value() {
        AppTaskState::Running { task } => task.clone(),
        AppTaskState::Completed(task) => task.clone(),
        AppTaskState::Failed { task, .. } => task.clone(),
        AppTaskState::Canceled(task) => task.clone(),
    };

    let is_terminal = matches!(
        existing_task.status.state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    );
    if is_terminal {
        return a2a_error(
            "TaskNotCancelableError",
            &format!("Task {} is already in a terminal state", task_id),
        );
    }

    let mut canceled = existing_task;
    canceled.status = TaskStatus {
        state: TaskState::Canceled,
        message: None,
        timestamp: Some(chrono::Utc::now()),
    };
    if let Some(meta) = body.metadata {
        match &mut canceled.metadata {
            Some(existing) if existing.is_object() && meta.is_object() => {
                if let (Some(existing_obj), Some(new_obj)) = (existing.as_object_mut(), meta.as_object()) {
                    for (k, v) in new_obj {
                        existing_obj.insert(k.clone(), v.clone());
                    }
                } else {
                    canceled.metadata = Some(meta);
                }
            }
            _ => {
                canceled.metadata = Some(meta);
            }
        }
    }
    let broadcast_tx = get_or_create_broadcast(&state, &task_id);
    broadcast_status(&broadcast_tx, &task_id, canceled.context_id.as_deref().unwrap_or(""), canceled.status.clone());
    state.tasks.insert(task_id, AppTaskState::Canceled(canceled.clone()));
    (StatusCode::OK, Json(canceled)).into_response()
}

// ───────────────────────────────────────────
// POST /tasks (List Tasks)
// ───────────────────────────────────────────

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(body): axum::extract::Query<crate::types::ListTasksRequest>,
) -> impl IntoResponse {
    let mut tasks: Vec<Task> = state
        .tasks
        .iter()
        .filter_map(|entry| {
            let mut task = match entry.value() {
                AppTaskState::Running { task } => task.clone(),
                AppTaskState::Completed(task) => task.clone(),
                AppTaskState::Failed { task, .. } => task.clone(),
                AppTaskState::Canceled(task) => task.clone(),
            };

            if let Some(ref ctx) = body.context_id {
                if task.context_id.as_ref() != Some(ctx) {
                    return None;
                }
            }
            if let Some(ref st) = body.status {
                if &task.status.state != st {
                    return None;
                }
            }
            if let Some(ref after) = body.status_timestamp_after {
                if task.status.timestamp.as_ref().map(|t| t < after).unwrap_or(true) {
                    return None;
                }
            }

            // Omit artifacts if requested.
            if body.include_artifacts == Some(false) {
                task.artifacts = None;
            }

            // Apply historyLength limit if provided.
            if let Some(hl) = body.history_length {
                let hl = hl as usize;
                if let Some(ref mut history) = task.history {
                    if history.len() > hl {
                        let start = history.len() - hl;
                        *history = history.split_off(start);
                    }
                }
            }

            Some(task)
        })
        .collect();

    tasks.sort_by(|a, b| b.status.timestamp.cmp(&a.status.timestamp));

    let total_size = tasks.len() as u32;
    let page_size = body.page_size.unwrap_or(20).max(1);
    let next_page_token = if tasks.len() > page_size as usize {
        tasks.truncate(page_size as usize);
        "1".to_string()
    } else {
        "".to_string()
    };

    let resp = crate::types::ListTasksResponse {
        tasks,
        page_size,
        total_size,
        next_page_token,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

// ───────────────────────────────────────────
// POST /tasks/{taskId}/subscribe
// ───────────────────────────────────────────

pub async fn subscribe_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    // Reject if server does not advertise streaming capability.
    if !state.agent_card.capabilities.streaming {
        return a2a_error(
            "UnsupportedOperationError",
            "Streaming is not supported by this agent",
        );
    }

    let task = match state.tasks.get(&task_id) {
        Some(entry) => match entry.value() {
            AppTaskState::Running { task } => task.clone(),
            AppTaskState::Completed(task) => task.clone(),
            AppTaskState::Failed { task, .. } => task.clone(),
            AppTaskState::Canceled(task) => task.clone(),
        },
        None => {
            return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
        }
    };

    // Terminal-state tasks must return HTTP error, not open SSE stream.
    let is_terminal = matches!(
        task.status.state,
        TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
    );
    if is_terminal {
        return a2a_error(
            "UnsupportedOperationError",
            "Task is already in a terminal state",
        );
    }

    let (sse_tx, sse_rx) =
        mpsc::channel::<Result<axum::response::sse::Event, std::convert::Infallible>>(64);

    let broadcast_tx = get_or_create_broadcast(&state, &task_id);
    let mut broadcast_rx = broadcast_tx.subscribe();

    tokio::spawn(async move {
        let init = crate::types::StreamResponse {
            task: Some(task.clone()),
            message: None,
            status_update: None,
            artifact_update: None,
        };
        let _ = sse_tx.send(Ok(init.into_sse_event())).await;

        while let Ok(resp) = broadcast_rx.recv().await {
            let is_terminal = resp.status_update.as_ref().map(|u| {
                matches!(
                    u.status.state,
                    TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
                )
            }).unwrap_or(false);
            let _ = sse_tx.send(Ok(resp.into_sse_event())).await;
            if is_terminal {
                break;
            }
        }
    });

    let stream = ReceiverStream::new(sse_rx);
    Sse::new(stream).into_response()
}

// ───────────────────────────────────────────
// GET /extendedAgentCard
// ───────────────────────────────────────────

pub async fn get_extended_agent_card(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !state.extended_agent_card_enabled {
        return a2a_error(
            "UnsupportedOperationError",
            "Extended agent card is not supported",
        );
    }

    let mut extended = state.agent_card.clone();
    extended.provider = Some(crate::types::AgentProvider {
        organization: "rust-agent-project".to_string(),
        url: state.agent_card.url.clone(),
    });
    extended.documentation_url = Some(format!("{}/docs", state.agent_card.url));
    (StatusCode::OK, Json(extended)).into_response()
}

// ───────────────────────────────────────────
// Push Notification stubs
// ───────────────────────────────────────────

pub async fn create_push_config(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if !state.tasks.contains_key(&task_id) {
        return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
    }
    a2a_error("PushNotificationNotSupportedError", "Push notifications are not supported")
}

pub async fn list_push_configs(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if !state.tasks.contains_key(&task_id) {
        return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
    }
    a2a_error("PushNotificationNotSupportedError", "Push notifications are not supported")
}

pub async fn get_push_config(
    State(state): State<Arc<AppState>>,
    Path((task_id, _config_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !state.tasks.contains_key(&task_id) {
        return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
    }
    a2a_error("PushNotificationNotSupportedError", "Push notifications are not supported")
}

pub async fn delete_push_config(
    State(state): State<Arc<AppState>>,
    Path((task_id, _config_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !state.tasks.contains_key(&task_id) {
        return a2a_error("TaskNotFoundError", &format!("Task {} not found", task_id));
    }
    a2a_error("PushNotificationNotSupportedError", "Push notifications are not supported")
}

// ───────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────

fn extract_user_input(message: &Message) -> Result<String, String> {
    let mut texts = Vec::new();
    for part in &message.parts {
        match part {
            Part::Text { text } => texts.push(text.clone()),
            Part::File { url, .. } => {
                if let Some(url) = url {
                    texts.push(format!("[File: {}]", url));
                } else {
                    return Err("File parts without URL are not supported in MVP".to_string());
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

fn a2a_error(code: &str, message: &str) -> axum::response::Response {
    use crate::errors::{A2AError, A2AErrorResponse};
    A2AErrorResponse {
        error: A2AError::new(code, message),
    }
    .into_response()
}

fn get_or_create_broadcast(
    state: &AppState,
    task_id: &str,
) -> tokio::sync::broadcast::Sender<crate::types::StreamResponse> {
    state
        .task_broadcasts
        .get(task_id)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| {
            let (tx, _rx) = tokio::sync::broadcast::channel(64);
            state
                .task_broadcasts
                .insert(task_id.to_string(), tx.clone());
            tx
        })
}

fn broadcast_status(
    tx: &tokio::sync::broadcast::Sender<crate::types::StreamResponse>,
    task_id: &str,
    context_id: &str,
    status: TaskStatus,
) {
    let _ = tx.send(crate::types::StreamResponse {
        status_update: Some(crate::types::TaskStatusUpdateEvent {
            task_id: task_id.to_string(),
            context_id: context_id.to_string(),
            status,
            metadata: None,
        }),
        task: None,
        message: None,
        artifact_update: None,
    });
}
