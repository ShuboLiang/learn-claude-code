use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use tokio_stream::StreamExt;

use rust_agent_core::agent::AgentApp;
use rust_agent_core::api::types::ApiMessage;
use rust_agent_core::bots::BotRegistry;
use rust_agent_core::context::ContextService;
use rust_agent_core::mpsc;
use rust_agent_core::skills::SkillLoader;

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
    pub bot_registry: Arc<BotRegistry>,
}

/// 构建所有 API 路由
pub fn routes(store: SessionStore) -> Router {
    // 服务启动时加载 Bot 注册表（含全局技能）
    let global_skills = SkillLoader::load_from_dirs(&[]).unwrap_or_default();
    let bot_registry = BotRegistry::load(global_skills).unwrap_or_default();

    let app_state = AppState {
        store: store.clone(),
        bot_registry: Arc::new(bot_registry),
    };

    Router::new()
        .route("/", get(health_check))
        .route("/sessions", post(create_session))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/sessions/{id}/messages", post(send_message))
        .route("/sessions/{id}/clear", post(clear_session))
        .route(
            "/v1/chat/completions",
            post(openai_compat::chat_completions),
        )
        .route("/bots", get(list_bots))
        .route("/bots/{name}/task", post(bot_task))
        .with_state(app_state)
}

/// GET / — 健康检查
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// POST /sessions — 创建新会话
async fn create_session(State(state): State<AppState>) -> impl IntoResponse {
    let _ = dotenvy::dotenv();
    let agent = match AgentApp::from_env().await {
        Ok(a) => a,
        Err(e) => return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::json!({ "error": { "code": "init_failed", "message": e.to_string() } }),
            ),
        )
            .into_response(),
    };
    let model = agent.model().to_owned();
    let session = state.store.create(Arc::new(agent));
    Json(serde_json::json!({
        "id": session.id,
        "model": model,
        "created_at": session.created_at.to_rfc3339(),
    }))
    .into_response()
}

/// GET /sessions/:id — 查询会话状态
async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get(&id) {
        Some(session) => Json(serde_json::json!({
            "id": session.id,
            "message_count": session.context.len(),
            "created_at": session.created_at.to_rfc3339(),
            "last_active": session.last_active.to_rfc3339(),
        }))
        .into_response(),
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
    if state.store.remove(&id) {
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
    if state.store.clear_context(&id) {
        Json(serde_json::json!({ "status": "cleared" })).into_response()
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

/// POST /sessions/:id/messages — 发送消息（SSE 流式响应）
async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let session = match state.store.get(&id) {
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

    // 在后台启动 agent
    let agent = session.agent.clone();
    let mut ctx = session.context.clone();
    let content = body.content;
    let session_id = id;
    let store = state.store.clone();

    tokio::spawn(async move {
        if let Err(e) = agent
            .handle_user_turn(&mut ctx, &content, event_tx.clone())
            .await
        {
            let _ = event_tx
                .send(rust_agent_core::agent::AgentEvent::Error {
                    code: "agent_error".to_owned(),
                    message: format!("{e:#}"),
                })
                .await;
        }
        store.update(&session_id, ctx);
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
async fn bot_task(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<BotTaskRequest>,
) -> impl IntoResponse {
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

    // 构建 Bot 的 system prompt
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

    // 创建独立上下文并注入系统提示
    let mut ctx = ContextService::new();
    ctx.push(ApiMessage {
        role: "system".to_owned(),
        content: serde_json::Value::String(system_prompt),
    });
    ctx.push_user_text(&body.content);

    let (event_tx, event_rx) = mpsc::channel(64);

    // 在后台启动 agent（使用主 agent 的模型/profile）
    let _store = state.store.clone();
    tokio::spawn(async move {
        let _ = dotenvy::dotenv();
        let agent = match AgentApp::from_env().await {
            Ok(a) => a,
            Err(e) => {
                let _ = event_tx
                    .send(rust_agent_core::agent::AgentEvent::Error {
                        code: "bot_init_failed".to_owned(),
                        message: format!("Bot 初始化失败: {e:#}"),
                    })
                    .await;
                return;
            }
        };

        if let Err(e) = agent
            .handle_user_turn(&mut ctx, &body.content, event_tx.clone())
            .await
        {
            let _ = event_tx
                .send(rust_agent_core::agent::AgentEvent::Error {
                    code: "bot_agent_error".to_owned(),
                    message: format!("{e:#}"),
                })
                .await;
        }
    });

    // 将 AgentEvent 流转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream).into_response()
}
