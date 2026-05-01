//! OpenAI Chat Completions 兼容 API
//!
//! 提供 `/v1/chat/completions` 端点，兼容 OpenAI API 格式，
//! 供 Cursor、Continue 等工具直接调用。

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::{Value, json};

use rust_agent_core::context::ContextService;
use rust_agent_core::mpsc;

/// OpenAI Chat Completions 请求格式
#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    /// 模型 ID（忽略，使用服务端配置的模型）
    #[allow(dead_code)]
    model: Option<String>,
    /// 消息列表
    messages: Vec<ChatMessage>,
    /// 工具定义（当前未使用，Agent 工具由服务端管理）
    #[allow(dead_code)]
    tools: Option<Vec<Value>>,
    /// 是否流式响应（当前仅支持非流式）
    #[allow(dead_code)]
    stream: bool,
    /// 最大生成 token 数（当前未使用）
    #[allow(dead_code)]
    max_tokens: Option<u32>,
}

/// OpenAI 消息格式
#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "role")]
#[serde(rename_all = "lowercase")]
pub enum ChatMessage {
    System {
        content: String,
    },
    User {
        #[serde(default)]
        content: Option<Value>,
    },
    Assistant {
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        #[allow(dead_code)]
        tool_calls: Option<Vec<Value>>,
    },
    Tool {
        #[allow(dead_code)]
        tool_call_id: String,
        content: String,
    },
}

/// OpenAI Chat Completions 响应格式
#[derive(serde::Serialize)]
pub struct ChatCompletionResponse {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<ChatCompletionChoice>,
    usage: ChatCompletionUsage,
}

/// OpenAI 响应中的选项
#[derive(serde::Serialize)]
struct ChatCompletionChoice {
    index: usize,
    message: ChatCompletionMessage,
    finish_reason: String,
}

/// OpenAI 响应中的消息
#[derive(serde::Serialize)]
struct ChatCompletionMessage {
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<Value>>,
}

/// OpenAI 响应中的 token 使用量（当前为占位值）
#[derive(serde::Serialize)]
struct ChatCompletionUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// 生成短 ID
fn short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos)[..16].to_owned()
}

/// 将 OpenAI 格式的消息列表提取为一条用户输入文本
///
/// 由于 AgentApp 的 handle_user_turn 接受单条输入，
/// 这里将所有消息拼接为上下文供 agent 处理。
fn extract_user_input(messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();

    for msg in messages {
        match msg {
            ChatMessage::System { content } => {
                parts.push(format!("[System]: {content}"));
            }
            ChatMessage::User { content } => {
                if let Some(c) = content {
                    let text = if c.is_string() {
                        c.as_str().unwrap_or("").to_owned()
                    } else {
                        c.to_string()
                    };
                    parts.push(text);
                }
            }
            ChatMessage::Assistant { content, .. } => {
                if let Some(c) = content {
                    parts.push(format!("[Assistant]: {c}"));
                }
            }
            ChatMessage::Tool { content, .. } => {
                parts.push(format!("[Tool Result]: {content}"));
            }
        }
    }

    parts.join("\n\n")
}

/// POST /v1/chat/completions — OpenAI 兼容端点
pub async fn chat_completions(
    State(state): State<crate::routes::AppState>,
    Json(body): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let agent = state.agent.as_ref().clone();

    let model = std::env::var("MODEL_ID").unwrap_or_else(|_| "unknown".to_owned());
    let user_input = extract_user_input(&body.messages);

    if user_input.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": "消息列表中未找到有效的用户输入",
                    "type": "invalid_request_error",
                    "code": "empty_messages"
                }
            })),
        )
            .into_response();
    }

    let (event_tx, mut event_rx) = mpsc::channel(64);
    let mut ctx = ContextService::new();

    tokio::spawn(async move {
        let _ = agent
            .handle_user_turn(&mut ctx, &user_input, event_tx)
            .await;
    });

    // 收集所有事件
    let mut final_text = String::new();
    let mut tool_calls_collected = Vec::new();
    let mut stop_reason = "stop".to_owned();

    while let Some(event) = event_rx.recv().await {
        match event {
            rust_agent_core::agent::AgentEvent::TextDelta(text) => {
                final_text.push_str(&text);
            }
            rust_agent_core::agent::AgentEvent::ToolCall {
                id,
                name,
                input,
                parallel_index: _,
            } => {
                tool_calls_collected.push(json!({
                    "id": id.unwrap_or_else(|| format!("call_{}", short_id())),
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": input.to_string(),
                    }
                }));
            }
            rust_agent_core::agent::AgentEvent::TurnEnd {
                api_calls: _,
                token_usage: _,
            } => {
                if !tool_calls_collected.is_empty() {
                    stop_reason = "tool_calls".to_owned();
                }
            }
            rust_agent_core::agent::AgentEvent::Error { code, message } => {
                let status = if code == "rate_limited" {
                    StatusCode::TOO_MANY_REQUESTS
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                };
                return (
                    status,
                    Json(json!({
                        "error": {
                            "message": message,
                            "type": "llm_api_error",
                            "code": code
                        }
                    })),
                )
                    .into_response();
            }
            _ => {}
        }
    }

    let response = ChatCompletionResponse {
        id: format!("chatcmpl-{}", short_id()),
        object: "chat.completion".to_owned(),
        created: chrono::Utc::now().timestamp(),
        model,
        choices: vec![ChatCompletionChoice {
            index: 0,
            message: ChatCompletionMessage {
                role: "assistant".to_owned(),
                content: if final_text.is_empty() {
                    None
                } else {
                    Some(final_text)
                },
                tool_calls: if tool_calls_collected.is_empty() {
                    None
                } else {
                    Some(tool_calls_collected)
                },
            },
            finish_reason: stop_reason,
        }],
        usage: ChatCompletionUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}
