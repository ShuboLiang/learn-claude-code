//! Mock LLM Server — 用于测试 CLI 的异常处理与状态透传
//!
//! 兼容 OpenAI Chat Completions API（流式 SSE）。
//! 在请求消息的 content 中包含以下标记可触发特定行为：
//!
//! | 标记 | 行为 |
//! |------|------|
//! | `[mock:ok]` | 正常响应（默认） |
//! | `[mock:timeout]` | 永不响应，模拟请求超时 |
//! | `[mock:slow]` | 延迟 20 秒后正常响应 |
//! | `[mock:429]` | 返回 429 + Retry-After: 2 |
//! | `[mock:500]` | 返回 500 服务器错误 |
//! | `[mock:disconnect]` | 建立 SSE 连接后 1 秒断开 |
//! | `[mock:large]` | 返回超大文本（约 2 万字符） |
//! | `[mock:tool_calls]` | 返回 tool_calls 响应（调用 bash） |
//! | `[mock:empty]` | 返回空 assistant 回复 |
//! | `[mock:retry_once]` | 第一次请求返回 500，第二次正常 |
//!
//! 启动: `cargo run --bin mock-llm`
//! 配置使用: base_url = "http://127.0.0.1:3001", api_key = "fake"

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, sse::Event, Sse},
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;


// ── 请求体 ──

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(default)]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    stream: bool,
}

#[derive(Deserialize)]
struct Message {
    role: String,
    content: serde_json::Value,
}

// ── 响应体 ──

#[derive(Serialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Serialize)]
struct Choice {
    message: RespMessage,
    finish_reason: String,
}

#[derive(Serialize)]
struct RespMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Serialize)]
struct ToolCall {
    id: String,
    r#type: String,
    function: FunctionCall,
}

#[derive(Serialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

// ── 全局计数器（用于 retry_once 等场景） ──

struct Counters {
    retry_once: AtomicUsize,
}

// ── 解析 mock 标记 ──

fn extract_mock_tag(messages: &[Message]) -> Option<String> {
    // 只从最后一条消息中查找 mock 标记，避免历史消息干扰
    let last_msg = messages.last()?;
    let text = match &last_msg.content {
        serde_json::Value::String(s) => s.clone(),
        _ => return None,
    };
    if let Some(start) = text.find("[mock:") {
        if let Some(end) = text[start..].find(']') {
            return Some(text[start + 6..start + end].to_string());
        }
    }
    None
}

fn estimate_tokens(messages: &[Message]) -> u64 {
    let text: String = messages
        .iter()
        .filter_map(|m| m.content.as_str())
        .collect();
    // 粗略估算：1 token ≈ 4 字符（中文约 1 字符 1 token）
    (text.len() as u64).max(1)
}

// ── 主路由 ──

async fn chat_completions(
    State(counters): State<Arc<Counters>>,
    Json(body): Json<ChatRequest>,
) -> impl IntoResponse {
    let tag = extract_mock_tag(&body.messages).unwrap_or_default();
    let input_tokens = estimate_tokens(&body.messages);

    println!("[mock-llm] 收到请求 | model={} | tag=[mock:{}] | stream={} | messages={}",
        body.model, tag, body.stream, body.messages.len());

    match tag.as_str() {
        "timeout" => {
            // 永不返回，模拟超时
            sleep(Duration::from_secs(3600)).await;
            unreachable!()
        }
        "slow" => {
            // 延迟 20 秒后正常响应
            sleep(Duration::from_secs(20)).await;
            normal_response("（模拟慢响应）这是延迟 20 秒后的正常回复。", input_tokens, body.stream)
        }
        "429" => {
            // 限流
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "2")],
                Json(serde_json::json!({"error": {"message": "Rate limited", "type": "rate_limit_error"}})),
            )
                .into_response()
        }
        "500" => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": {"message": "Internal server error", "type": "api_error"}})),
            )
                .into_response()
        }
        "retry_once" => {
            let count = counters.retry_once.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                println!("[mock-llm] retry_once: 第 1 次返回 500");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": {"message": "Temporary error", "type": "api_error"}})),
                )
                    .into_response()
            } else {
                println!("[mock-llm] retry_once: 第 {} 次正常响应", count + 1);
                normal_response("retry_once 正常响应，前面故意失败了一次。", input_tokens, body.stream)
            }
        }
        "disconnect" => {
            // SSE 模式下 1 秒后断开；非 SSE 直接 500
            if body.stream {
                let stream = async_stream::stream! {
                    yield Ok::<_, std::convert::Infallible>(
                        Event::default().data(serde_json::json!({
                            "choices": [{"delta": {"content": "即将断开..."}, "finish_reason": null}]
                        }).to_string())
                    );
                    sleep(Duration::from_secs(1)).await;
                    // 直接结束流，不发送 [DONE]
                };
                Sse::new(stream).into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": {"message": "Disconnected"}})),
                )
                    .into_response()
            }
        }
        "large" => {
            let big_text = "这是一段用于测试大输出处理的重复文本。\n".repeat(1000);
            normal_response(&format!("[mock:large] 超大输出测试\n\n{}", big_text), input_tokens, body.stream)
        }
        "tool_calls" => {
            tool_call_response(input_tokens, body.stream)
        }
        "empty" => {
            normal_response("", input_tokens, body.stream)
        }
        _ => {
            // 默认正常响应
            normal_response("这是 mock-llm 的正常响应。如果你想测试异常场景，请在消息中加入 [mock:timeout]、[mock:slow]、[mock:429]、[mock:500]、[mock:disconnect]、[mock:large]、[mock:tool_calls]、[mock:empty] 等标记。", input_tokens, body.stream)
        }
    }
}

// ── 正常响应（支持 SSE 流式） ──

fn normal_response(text: &str, input_tokens: u64, stream: bool) -> axum::response::Response {
    if !stream {
        return Json(ChatResponse {
            choices: vec![Choice {
                message: RespMessage {
                    role: "assistant".to_string(),
                    content: text.to_string(),
                    tool_calls: None,
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: input_tokens,
                completion_tokens: text.len() as u64 / 4,
            },
        }).into_response();
    }

    let text = text.to_string();
    let _input_tokens = input_tokens;
    let stream = async_stream::stream! {
        let char_vec: Vec<char> = text.chars().collect();
        let chunks: Vec<String> = char_vec
            .chunks(20)
            .map(|c| c.iter().collect::<String>())
            .collect();

        for chunk in chunks {
            let data = serde_json::json!({
                "choices": [{"delta": {"content": chunk}, "finish_reason": null}]
            });
            yield Ok::<_, std::convert::Infallible>(Event::default().data(data.to_string()));
            sleep(Duration::from_millis(10)).await;
        }

        let done = serde_json::json!({
            "choices": [{"delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": _input_tokens, "completion_tokens": text.len() as u64 / 4}
        });
        yield Ok::<_, std::convert::Infallible>(Event::default().data(done.to_string()));
        yield Ok::<_, std::convert::Infallible>(Event::default().data("[DONE]"));
    };

    Sse::new(stream).into_response()
}

// ── tool_calls 响应 ──

fn tool_call_response(input_tokens: u64, stream: bool) -> axum::response::Response {
    if !stream {
        return Json(ChatResponse {
            choices: vec![Choice {
                message: RespMessage {
                    role: "assistant".to_string(),
                    content: "".to_string(),
                    tool_calls: Some(vec![ToolCall {
                        id: "call_mock_001".to_string(),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "bash".to_string(),
                            arguments: r#"{"command": "echo hello from mock"}"#.to_string(),
                        },
                    }]),
                },
                finish_reason: "tool_calls".to_string(),
            }],
            usage: Usage {
                prompt_tokens: input_tokens,
                completion_tokens: 50,
            },
        }).into_response();
    }

    let stream = async_stream::stream! {
        let data = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_mock_001",
                        "type": "function",
                        "function": {"name": "bash", "arguments": ""}
                    }]
                },
                "finish_reason": null
            }]
        });
        yield Ok::<_, std::convert::Infallible>(Event::default().data(data.to_string()));
        sleep(Duration::from_millis(50)).await;

        let data = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": r#"{"command": "echo hello from mock"}"#}
                    }]
                },
                "finish_reason": null
            }]
        });
        yield Ok::<_, std::convert::Infallible>(Event::default().data(data.to_string()));
        sleep(Duration::from_millis(50)).await;

        let done = serde_json::json!({
            "choices": [{"delta": {}, "finish_reason": "tool_calls"}]
        });
        yield Ok::<_, std::convert::Infallible>(Event::default().data(done.to_string()));
        yield Ok::<_, std::convert::Infallible>(Event::default().data("[DONE]"));
    };

    Sse::new(stream).into_response()
}

// ── 主函数 ──

#[tokio::main]
async fn main() {
    let counters = Arc::new(Counters {
        retry_once: AtomicUsize::new(0),
    });

    let app = Router::new()
        .route("/v1/chat/completions", axum::routing::post(chat_completions))
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(counters);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Mock LLM Server 已启动                             ║");
    println!("║           http://127.0.0.1:3001                              ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  在消息中加入以下标记测试不同场景：                           ║");
    println!("║    [mock:ok]        正常响应（默认）                         ║");
    println!("║    [mock:timeout]   永不响应，模拟超时                       ║");
    println!("║    [mock:slow]      延迟 20 秒后响应                         ║");
    println!("║    [mock:429]       返回 429 限流                            ║");
    println!("║    [mock:500]       返回 500 错误                            ║");
    println!("║    [mock:retry_once] 首次 500，重试后正常                    ║");
    println!("║    [mock:disconnect] SSE 连接 1 秒后断开                     ║");
    println!("║    [mock:large]     返回约 2 万字符超大文本                 ║");
    println!("║    [mock:tool_calls] 返回工具调用                            ║");
    println!("║    [mock:empty]     返回空回复                               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("配置参考：");
    println!(r#"  {{ "provider": "openai", "base_url": "http://127.0.0.1:3001", "api_key": "fake", "model": "mock" }}"#);

    axum::serve(listener, app).await.unwrap();
}
