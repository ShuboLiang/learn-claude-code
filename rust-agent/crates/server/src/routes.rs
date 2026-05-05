use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    response::sse::KeepAlive,
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
        .route(
            "/sessions/{id}/messages",
            get(get_session_messages).post(send_message),
        )
        .route("/sessions/{id}/clear", post(clear_session))
        .route(
            "/v1/chat/completions",
            post(openai_compat::chat_completions),
        )
        .route("/browse", get(browse_directory))
        .route("/watch", get(watch_files))
        .route("/file", get(read_file).put(write_file))
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

#[derive(Deserialize)]
struct CreateSessionRequest {
    #[serde(default)]
    working_dir: Option<String>,
}

/// POST /sessions — 创建新会话（仅在内存中创建，首次对话时才持久化到磁盘）
async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let working_dir = body
        .working_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let session_arc = state.store.create(working_dir).await;
    let session = session_arc.read().await;
    let model = state.agent.model().to_owned();
    let id = session.id.clone();
    let created_at = session.created_at.to_rfc3339();
    let wd = session.working_dir.display().to_string();
    drop(session);
    // 不在此处 persist，避免产生空会话文件
    // 文件将在首次 send_message 时（有实际对话内容后）才写入磁盘

    Json(serde_json::json!({
        "id": id,
        "model": model,
        "created_at": created_at,
        "working_dir": wd,
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
        tracing::info!("[send_message] 等待获取 session 写锁...");
        let mut session = session_arc.write().await;
        let cwd = session.working_dir.clone();
        tracing::info!("[send_message] 获取写锁成功，开始调用 handle_user_turn");
        if let Err(e) = agent
            .handle_user_turn(&mut session.context, &content, Some(&cwd), event_tx.clone())
            .await
        {
            tracing::error!("[send_message] handle_user_turn 失败: {e:#}");
            let _ = event_tx
                .send(rust_agent_core::agent::AgentEvent::Error {
                    code: "agent_error".to_owned(),
                    message: format!("{e:#}"),
                })
                .await;
        }
        tracing::info!("[send_message] handle_user_turn 完成");
        session.last_active = chrono::Utc::now();
        drop(session);
        store.persist(&session_id).await;
        // 无论成功还是失败，都发送 Done 事件，让客户端知道 SSE 流已结束
        let _ = event_tx
            .send(rust_agent_core::agent::AgentEvent::Done)
            .await;
        tracing::info!("[send_message] Done 事件已发送");
    });

    // 将 AgentEvent 流转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[derive(Deserialize)]
struct BrowseQuery {
    path: Option<String>,
}

/// GET /browse?path=... — 浏览目录，返回子目录和文件列表
async fn browse_directory(Query(q): Query<BrowseQuery>) -> impl IntoResponse {
    let current = match &q.path {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            // 空路径：Windows 返回驱动器列表，Unix 返回根目录
            if cfg!(windows) {
                return Json(serde_json::json!({
                    "path": "",
                    "parent": serde_json::Value::Null,
                    "entries": list_windows_drives(),
                }))
                .into_response();
            } else {
                PathBuf::from("/")
            }
        }
    };

    if !current.is_dir() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "not_found", "message": "目录不存在" }
            })),
        )
            .into_response();
    }

    let parent = current.parent().map(|p| p.display().to_string());
    let mut dirs: Vec<serde_json::Value> = Vec::new();
    let mut files: Vec<serde_json::Value> = Vec::new();

    // 重型目录，列出内容会很慢，直接跳过
    const SKIP_DIRS: &[&str] = &["node_modules", "target", ".git", "__pycache__", "dist", ".next"];

    if let Ok(read) = std::fs::read_dir(&current) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // 跳过隐藏文件和系统目录
            if name.starts_with('.') || name.starts_with('$') {
                continue;
            }
            let ft = entry.file_type().ok();
            let is_dir = ft.map(|t| t.is_dir()).unwrap_or(false);

            if is_dir {
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                dirs.push(serde_json::json!({
                    "name": name,
                    "path": entry.path().display().to_string(),
                    "kind": "directory",
                }));
            } else {
                let metadata = entry.metadata().ok();
                let size = metadata.as_ref().map(|m| m.len());
                let modified = metadata
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        let dt: chrono::DateTime<chrono::Utc> = t.into();
                        dt.to_rfc3339()
                    });
                let mut entry_json = serde_json::json!({
                    "name": name,
                    "path": entry.path().display().to_string(),
                    "kind": "file",
                });
                if let Some(s) = size {
                    entry_json["size"] = serde_json::json!(s);
                }
                if let Some(ref m) = modified {
                    entry_json["modified"] = serde_json::json!(m);
                }
                files.push(entry_json);
            }
        }
    }

    dirs.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    files.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });

    dirs.extend(files);

    Json(serde_json::json!({
        "path": current.display().to_string(),
        "parent": parent,
        "entries": dirs,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct WatchQuery {
    session_id: String,
}

/// GET /watch?session_id=... — 实时监听工作目录文件变更（SSE）
async fn watch_files(
    State(state): State<AppState>,
    Query(q): Query<WatchQuery>,
) -> impl IntoResponse {
    let session_arc = match state.store.get(&q.session_id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "session_not_found", "message": "会话不存在" }
                })),
            )
                .into_response();
        }
    };

    let working_dir = session_arc.read().await.working_dir.clone();

    let (tx, rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);

    // spawn_blocking: notify watcher 在同步线程运行
    let tx_closed = tx.clone();
    tokio::task::spawn_blocking(move || {
        use notify::{RecursiveMode, Watcher};
        let debounce = Mutex::new(HashMap::<PathBuf, Instant>::new());
        let tx = tx.clone();

        let mut watcher = match notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| {
                let Ok(event) = res else { return };
                // 忽略 .git 目录内的变更
                let path_str = event.paths.first().map(|p| p.display().to_string()).unwrap_or_default();
                if path_str.contains("/.git/") || path_str.contains("\\.git\\") {
                    return;
                }
                let kind = match event.kind {
                    notify::EventKind::Create(_) => "file_created",
                    notify::EventKind::Modify(_) => "file_modified",
                    notify::EventKind::Remove(_) => "file_removed",
                    _ => return,
                };
                let file_kind = if event.paths.first().map(|p| p.is_dir()).unwrap_or(false) {
                    "directory"
                } else {
                    "file"
                };

                // 200ms 去抖动
                let now = Instant::now();
                let path_key = event.paths.first().cloned().unwrap_or_default();
                {
                    let mut db = debounce.lock().unwrap();
                    if let Some(last) = db.get(&path_key) {
                        if now.duration_since(*last) < Duration::from_millis(200) {
                            return;
                        }
                    }
                    db.insert(path_key.clone(), now);
                }

                let _ = tx.blocking_send(serde_json::json!({
                    "event": kind,
                    "data": {
                        "path": path_str,
                        "kind": file_kind,
                    },
                }));
            },
        ) {
            Ok(w) => w,
            Err(_) => return,
        };

        let _ = watcher.watch(&working_dir, RecursiveMode::Recursive);

        // 保持 watcher 存活直到 channel 关闭
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if tx_closed.is_closed() {
                break;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx)
        .map(|v| {
            axum::response::sse::Event::default()
                .event(v["event"].as_str().unwrap_or("unknown"))
                .data(v["data"].to_string())
        })
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[cfg(windows)]
fn list_windows_drives() -> Vec<serde_json::Value> {
    let mut drives = Vec::new();
    for letter in b'A'..=b'Z' {
        let path_str = format!("{}:\\", letter as char);
        let p = std::path::Path::new(&path_str);
        if p.exists() {
            drives.push(serde_json::json!({
                "name": path_str,
                "path": path_str,
            }));
        }
    }
    drives
}

#[cfg(not(windows))]
fn list_windows_drives() -> Vec<serde_json::Value> {
    Vec::new()
}

#[derive(Deserialize)]
struct FileQuery {
    session_id: String,
    path: String,
}

#[derive(Deserialize)]
struct FileWriteBody {
    content: String,
}

/// GET /file?session_id=...&path=... — 读取文件内容
async fn read_file(
    State(state): State<AppState>,
    Query(q): Query<FileQuery>,
) -> impl IntoResponse {
    let session = match state.store.get(&q.session_id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "session_not_found", "message": "会话不存在" }
                })),
            )
                .into_response();
        }
    };

    let working_dir = session.read().await.working_dir.clone();
    let canonical_wd = working_dir.canonicalize().unwrap_or_else(|_| working_dir.clone());
    // If path is absolute, use it directly; otherwise join with working_dir
    let path_buf = std::path::PathBuf::from(&q.path);
    let full_path = if path_buf.is_absolute() {
        path_buf
    } else {
        working_dir.join(&q.path)
    };

    // Security: ensure path is within working_dir
    let Ok(canonical) = full_path.canonicalize() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "not_found", "message": "文件不存在" }
            })),
        )
            .into_response();
    };
    if !canonical.starts_with(&canonical_wd) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": { "code": "forbidden", "message": "禁止访问工作目录外的文件" }
            })),
        )
            .into_response();
    }

    // Try UTF-8 first, fall back to base64 for binary files
    match std::fs::read_to_string(&canonical) {
        Ok(content) => Json(serde_json::json!({ "content": content, "binary": false })).into_response(),
        Err(_) => match std::fs::read(&canonical) {
            Ok(bytes) => {
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Json(serde_json::json!({ "content": encoded, "binary": true })).into_response()
            }
            Err(_) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "not_found", "message": "文件无法读取" }
                })),
            )
                .into_response(),
        },
    }
}

/// PUT /file?session_id=...&path=... — 写入文件内容
async fn write_file(
    State(state): State<AppState>,
    Query(q): Query<FileQuery>,
    Json(body): Json<FileWriteBody>,
) -> impl IntoResponse {
    let session = match state.store.get(&q.session_id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "session_not_found", "message": "会话不存在" }
                })),
            )
                .into_response();
        }
    };

    let working_dir = session.read().await.working_dir.clone();
    let canonical_wd = working_dir.canonicalize().unwrap_or_else(|_| working_dir.clone());
    // If path is absolute, use it directly; otherwise join with working_dir
    let path_buf = std::path::PathBuf::from(&q.path);
    let full_path = if path_buf.is_absolute() {
        path_buf
    } else {
        working_dir.join(&q.path)
    };

    // Security: ensure path is within working_dir
    match full_path.canonicalize() {
        Ok(canonical) => {
            if !canonical.starts_with(&canonical_wd) {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": { "code": "forbidden", "message": "禁止访问工作目录外的文件" }
                    })),
                )
                    .into_response();
            }
        }
        Err(_) => {
            // File doesn't exist yet — verify parent directory is within working_dir
            if let Some(parent) = full_path.parent() {
                if let Ok(canonical) = parent.canonicalize() {
                    if !canonical.starts_with(&canonical_wd) {
                        return (
                            StatusCode::FORBIDDEN,
                            Json(serde_json::json!({
                                "error": { "code": "forbidden", "message": "禁止访问工作目录外的文件" }
                            })),
                        )
                            .into_response();
                    }
                }
            }
        }
    }

    match std::fs::write(&full_path, &body.content) {
        Ok(_) => Json(serde_json::json!({ "status": "saved" })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "code": "write_error", "message": format!("写入失败: {e}") }
            })),
        )
            .into_response(),
    }
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
            .handle_bot_turn(&mut ctx, &user_content, system_prompt, event_tx.clone(), None)
            .await
        {
            let _ = event_tx
                .send(rust_agent_core::agent::AgentEvent::Error {
                    code: "bot_agent_error".to_owned(),
                    message: format!("{e:#}"),
                })
                .await;
        }
        // 无论成功还是失败，都发送 Done 事件，让客户端知道 SSE 流已结束
        let _ = event_tx
            .send(rust_agent_core::agent::AgentEvent::Done)
            .await;
    });

    // 将 AgentEvent 流转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
