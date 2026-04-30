use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use tokio_stream::StreamExt;

use rust_agent_core::agent::AgentApp;
use rust_agent_core::bots::BotRegistry;
use rust_agent_core::context::ContextService;
use rust_agent_core::mpsc;

use crate::openai_compat;
use crate::session::SessionStore;
use crate::sse::agent_event_to_sse;

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

/// Bot 任务请求体
#[derive(Deserialize)]
pub struct BotTaskRequest {
    pub content: String,
}

/// 路由共享状态
#[derive(Clone)]
pub struct AppState {
    pub store: SessionStore,
    pub agent: Arc<AgentApp>,
    pub bot_registry: Arc<BotRegistry>,
}

/// 构建所有 API 路由
pub fn routes(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(health_check))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/sessions/{id}/messages", get(get_session_messages).post(send_message))
        .route("/sessions/{id}/clear", post(clear_session))
        .route(
            "/v1/chat/completions",
            post(openai_compat::chat_completions),
        )
        .route("/bots", get(list_bots))
        .route("/bots/{name}/task", post(bot_task))
        .with_state(app_state)
}

/// GET /sessions — 列出所有会话摘要
async fn list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state.store.list().await;
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

/// GET /sessions/:id/messages — 获取会话消息历史
async fn get_session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_messages(&id).await {
        Some(messages) => Json(serde_json::json!({ "messages": messages })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "code": "session_not_found",
                    "message": "会话不存在或已过期"
                }
            })),
        )
            .into_response(),
    }
}

/// GET / — 健康检查
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// POST /sessions — 创建新会话（仅在内存中创建，首次对话时才持久化到磁盘）
async fn create_session(State(state): State<AppState>) -> impl IntoResponse {
    let session_arc = state.store.create().await;
    let session = session_arc.read().await;
    let model = state.agent.model().to_owned();
    let id = session.id.clone();
    let created_at = session.created_at.to_rfc3339();
    drop(session);
    // 不在此处 persist，避免产生空会话文件
    // 文件将在首次 send_message 时（有实际对话内容后）才写入磁盘

    Json(serde_json::json!({
        "id": id,
        "model": model,
        "created_at": created_at,
    }))
    .into_response()
}

/// GET /sessions/:id — 查询会话状态
async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get(&id) {
        Some(session_arc) => {
            let session = session_arc.read().await;
            Json(serde_json::json!({
                "id": session.id,
                "message_count": session.context.len(),
                "created_at": session.created_at.to_rfc3339(),
                "last_active": session.last_active.to_rfc3339(),
            }))
            .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在或已过期" }
            })),
        )
            .into_response(),
    }
}

/// DELETE /sessions/:id — 删除会话
async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.store.remove(&id).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在" }
            })),
        )
            .into_response()
    }
}

/// POST /sessions/:id/clear — 清空会话的对话上下文
async fn clear_session(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get(&id) {
        Some(session_arc) => {
            let mut session = session_arc.write().await;
            session.context = ContextService::new();
            session.last_active = chrono::Utc::now();
            drop(session);
            state.store.persist(&id).await;
            Json(serde_json::json!({ "status": "cleared" })).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在" }
            })),
        )
            .into_response(),
    }
}

/// POST /sessions/:id/messages — 发送消息（SSE 流式响应）
async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let session_arc = match state.store.get(&id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "session_not_found", "message": "会话不存在或已过期" }
                })),
            )
                .into_response();
        }
    };

    let (event_tx, event_rx) = mpsc::channel(64);
    let agent = state.agent.clone();
    let content = body.content;
    let session_id = id;
    let store = state.store.clone();

    tokio::spawn(async move {
        let mut session = session_arc.write().await;
        if let Err(e) = agent
            .handle_user_turn(&mut session.context, &content, event_tx.clone())
            .await
        {
            if e.downcast_ref::<rust_agent_core::api::error::LlmApiError>().is_none() {
                let _ = event_tx
                    .send(rust_agent_core::agent::AgentEvent::Error {
                        code: "agent_error".to_owned(),
                        message: format!("{e:#}"),
                    })
                    .await;
            }
        }
        session.last_active = chrono::Utc::now();
        drop(session);
        store.persist(&session_id).await;
    });

    // 将 AgentEvent 流转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream).into_response()
}

/// GET /bots — 列出所有可用的 Bot
async fn list_bots(State(state): State<AppState>) -> Json<serde_json::Value> {
    let bots = state.bot_registry.list();
    Json(serde_json::json!({
        "bots": bots,
        "total": bots.len()
    }))
}

/// POST /bots/:name/task — 向指定 Bot 委派任务（SSE 流式响应）
///
/// Bot 拥有独立的身份、专属技能和自定义 system prompt。
/// 任务在独立上下文中执行，不污染主会话上下文。
///
/// **安全限制**：
/// - 需要 `SERVER_API_KEY` 环境变量鉴权（Header: `Authorization: Bearer <key>`）
/// - 禁止嵌套 task 工具（`allow_task: false`），防止无限嵌套消耗配额
async fn bot_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<BotTaskRequest>,
) -> impl IntoResponse {
    // ── 鉴权：校验 SERVER_API_KEY ──
    if let Ok(expected_key) = std::env::var("SERVER_API_KEY") {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if provided != expected_key {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": {
                        "code": "unauthorized",
                        "message": "缺少或无效的 API key，请在 Authorization 头中提供 Bearer token"
                    }
                })),
            )
                .into_response();
        }
    } // 未设置 SERVER_API_KEY 则在开发模式下允许所有请求

    let bot = match state.bot_registry.find(&name) {
        Some(b) => b.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": {
                        "code": "bot_not_found",
                        "message": format!("Bot '{}' 不存在", name)
                    }
                })),
            )
                .into_response();
        }
    };

    let (event_tx, event_rx) = mpsc::channel(64);

    let agent = state.agent.clone();
    let user_content = body.content.clone();
    let bot_skills = bot.skills.clone();

    tokio::spawn(async move {
        let bot_agent = agent.as_ref().clone().with_skills(bot_skills);
        let mut ctx = ContextService::new();

        let nickname = if bot.metadata.nickname.is_empty() {
            &bot.metadata.name
        } else {
            &bot.metadata.nickname
        };
        let role = if bot.metadata.role.is_empty() {
            "智能助手"
        } else {
            &bot.metadata.role
        };

        let skills_desc = bot.skills.descriptions_for_system_prompt();

        let system_prompt = format!(
            "你是 {nickname}({role})。\n\
             工作目录：当前项目根目录。\n\n\
             可用技能：\n{skills_desc}\n\n\
             --- 专属指令 ---\n\
             {body}\n\n\
             ---\n\
             仅使用上述专属指令和技能执行用户任务。\
             完成后给出清晰的总结。",
            body = bot.body
        );

        if let Err(e) = bot_agent
            .handle_bot_turn(&mut ctx, &user_content, system_prompt, event_tx.clone())
            .await
        {
            if e.downcast_ref::<rust_agent_core::api::error::LlmApiError>().is_none() {
                let _ = event_tx
                    .send(rust_agent_core::agent::AgentEvent::Error {
                        code: "bot_agent_error".to_owned(),
                        message: format!("{e:#}"),
                    })
                    .await;
            }
        }
    });

    // 将 AgentEvent 流转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream).into_response()
}
