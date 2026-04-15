use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio_stream::StreamExt;

use rust_agent_core::agent::AgentApp;
use rust_agent_core::mpsc;

use crate::session::SessionStore;
use crate::sse::agent_event_to_sse;
use crate::openai_compat;

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

/// 构建所有 API 路由
pub fn routes(store: SessionStore) -> Router {
    Router::new()
        .route("/", get(health_check))
        .route("/sessions", post(create_session))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/sessions/{id}/messages", post(send_message))
        .route("/sessions/{id}/clear", post(clear_session))
        .route("/v1/chat/completions", post(openai_compat::chat_completions))
        .with_state(store)
}

/// GET / — 健康检查
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// POST /sessions — 创建新会话
async fn create_session(
    State(store): State<SessionStore>,
) -> impl IntoResponse {
    let _ = dotenvy::dotenv();
    let agent = match AgentApp::from_env().await {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": { "code": "init_failed", "message": e.to_string() } })),
            ).into_response()
        }
    };
    let session = store.create(Arc::new(agent));
    Json(serde_json::json!({
        "id": session.id,
        "created_at": session.created_at.to_rfc3339(),
    })).into_response()
}

/// GET /sessions/:id — 查询会话状态
async fn get_session(
    State(store): State<SessionStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store.get(&id) {
        Some(session) => Json(serde_json::json!({
            "id": session.id,
            "message_count": session.context.len(),
            "created_at": session.created_at.to_rfc3339(),
            "last_active": session.last_active.to_rfc3339(),
        })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在或已过期" }
            })),
        ).into_response(),
    }
}

/// DELETE /sessions/:id — 删除会话
async fn delete_session(
    State(store): State<SessionStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if store.remove(&id) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在" }
            })),
        ).into_response()
    }
}

/// POST /sessions/:id/clear — 清空会话的对话上下文
async fn clear_session(
    State(store): State<SessionStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if store.clear_context(&id) {
        Json(serde_json::json!({ "status": "cleared" })).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在" }
            })),
        ).into_response()
    }
}

/// POST /sessions/:id/messages — 发送消息（SSE 流式响应）
async fn send_message(
    State(store): State<SessionStore>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let session = match store.get(&id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "session_not_found", "message": "会话不存在或已过期" }
                })),
            ).into_response()
        }
    };

    let (event_tx, event_rx) = mpsc::channel(64);

    // 在后台启动 agent
    let agent = session.agent.clone();
    let mut ctx = session.context.clone();
    let content = body.content;
    let session_id = id;
    let store_clone = store.clone();

    tokio::spawn(async move {
        let _ = agent.handle_user_turn(&mut ctx, &content, event_tx).await;
        store_clone.update(&session_id, ctx);
    });

    // 将 AgentEvent 流转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream).into_response()
}
