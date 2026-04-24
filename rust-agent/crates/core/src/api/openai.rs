//! OpenAI 兼容 API 客户端
//!
//! 支持调用 OpenAI 官方 API 以及任何兼容 OpenAI 格式的 API（如 Ollama、vLLM、DeepSeek 等）。
//! 内部处理 OpenAI ↔ 内部格式的双向转换。
//! 重试行为对齐 Claude Code 官方实现：
//! - 默认最大重试 10 次（可通过 `RUST_AGENT_MAX_RETRIES` 环境变量覆盖）
//! - 默认请求超时 10 分钟（可通过 `RUST_AGENT_API_TIMEOUT_MS` 环境变量覆盖，单位毫秒）
//! - 对 429（限流）、5xx（服务器错误）、连接错误进行指数退避重试
//! - 429 响应优先解析 `Retry-After` 响应头

use std::time::Duration;

use anyhow::{Context, anyhow};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use super::retry;
use super::types::{
    ApiMessage, ProviderRequest, ProviderResponse, ResponseContentBlock, TokenUsage,
};
use crate::AgentResult;

// ── OpenAI 请求/响应类型（用于序列化和反序列化） ──

/// OpenAI Chat Completions 请求体
#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAIToolOwned>>,
    max_tokens: u32,
}

/// OpenAI 消息格式
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "role")]
#[serde(rename_all = "lowercase")]
enum OpenAIMessage {
    /// 系统消息（OpenAI 用 messages 中的 system role 传递系统提示词）
    System { content: String },
    /// 用户消息
    User {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<serde_json::Value>,
    },
    /// 助手消息（可能包含 tool_calls）
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<OpenAIToolCall>,
        /// 用于传递 Claude thinking 内容（部分兼容层要求）
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
    },
    /// 工具调用结果
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// OpenAI 工具定义（拥有所有权，避免生命周期问题）
#[derive(Serialize, Clone)]
struct OpenAIToolOwned {
    r#type: String,
    function: serde_json::Value,
}

/// OpenAI 工具调用（在 assistant 消息中返回）
#[derive(Serialize, Deserialize, Clone, Debug)]
struct OpenAIToolCall {
    id: String,
    r#type: String,
    function: OpenAIFunctionCall,
}

/// OpenAI 函数调用详情
#[derive(Serialize, Deserialize, Clone, Debug)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

/// OpenAI Chat Completions 响应体
#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: OpenAIUsage,
}

#[derive(Deserialize, Default)]
struct OpenAIUsage {
    #[serde(alias = "prompt_tokens")]
    input_tokens: u64,
    #[serde(alias = "completion_tokens")]
    output_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAITokensDetails>,
    #[serde(default)]
    input_tokens_details: Option<OpenAITokensDetails>,
}

#[derive(Deserialize)]
struct OpenAITokensDetails {
    #[serde(default)]
    cached_tokens: u64,
}

/// OpenAI 响应中的选项
#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIChoiceMessage,
    finish_reason: Option<String>,
}

/// OpenAI 响应中的消息内容
#[derive(Deserialize)]
struct OpenAIChoiceMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

/// OpenAI 兼容 API 的 HTTP 客户端
#[derive(Clone, Debug)]
pub struct OpenAIClient {
    /// reqwest HTTP 客户端（已配置默认请求头）
    http: reqwest::Client,
    /// API 密钥
    api_key: String,
    /// API 基础 URL（默认为 https://api.openai.com）
    base_url: String,
    /// 最大重试次数
    max_retries: u32,
}

impl OpenAIClient {
    /// 使用给定的 API 密钥和基础 URL 创建客户端
    pub fn new(api_key: &str, base_url: &str) -> AgentResult<Self> {
        Self::build_client(api_key, base_url)
    }

    /// 从环境变量创建 OpenAI API 客户端
    pub fn from_env() -> AgentResult<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").context("环境变量中缺少 OPENAI_API_KEY")?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_owned());
        Self::build_client(&api_key, &base_url)
    }

    /// 内部构造方法
    fn build_client(api_key: &str, base_url: &str) -> AgentResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let timeout_ms = retry::api_timeout_ms_from_env();
        let connect_timeout = Duration::from_secs(retry::DEFAULT_CONNECT_TIMEOUT_SECS);
        let timeout = Duration::from_millis(timeout_ms);

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(connect_timeout)
            .timeout(timeout)
            .build()
            .context("构建 HTTP 客户端失败")?;

        let max_retries = retry::max_retries_from_env();

        Ok(Self {
            http,
            api_key: api_key.to_owned(),
            base_url: base_url.to_owned(),
            max_retries,
        })
    }

    /// 获取 API 基础 URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 获取 API 密钥
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// 调用 OpenAI Chat Completions API
    ///
    /// 支持对 429（限流）、5xx（服务器错误）、连接错误的指数退避重试。
    async fn call_api(&self, request: &OpenAIRequest) -> AgentResult<OpenAIResponse> {
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        for attempt in 0..=self.max_retries {
            let send_result = self
                .http
                .post(&url)
                .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
                .header("X-Client-Name", "claude-code")
                .header(USER_AGENT, "claude-code/1.0")
                .json(request)
                .send()
                .await;

            let response = match send_result {
                Ok(resp) => resp,
                Err(e) => {
                    let is_retryable = e.is_timeout() || e.is_connect();
                    if is_retryable && attempt < self.max_retries {
                        let backoff = retry::calculate_backoff(None, attempt);
                        retry::log_retry(
                            "OpenAI",
                            &format!("请求失败: {}", retry::format_reqwest_error(&e)),
                            backoff,
                            attempt,
                            self.max_retries,
                        );
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(anyhow!(
                        "调用 OpenAI API 失败 (URL: {}): {}",
                        url,
                        retry::format_reqwest_error(&e)
                    ));
                }
            };

            let status = response.status();

            // 在消费 response 之前解析 Retry-After 响应头
            let retry_after = retry::parse_retry_after(&response);

            let body_bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    if attempt < self.max_retries {
                        let backoff = retry::calculate_backoff(retry_after, attempt);
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(e).context("读取 OpenAI 响应体失败");
                }
            };

            // 使用 lossy 转换避免 UTF-8 编码问题
            let body = String::from_utf8_lossy(&body_bytes).into_owned();

            if status.is_success() {
                return serde_json::from_str(&body).context("解析 OpenAI 响应 JSON 失败");
            }

            // 对可重试状态码进行重试（429, 5xx）
            if retry::is_retryable_status(status, &[]) && attempt < self.max_retries {
                let backoff = retry::calculate_backoff(retry_after, attempt);
                retry::log_retry(
                    "OpenAI",
                    &format!("返回 {status}"),
                    backoff,
                    attempt,
                    self.max_retries,
                );
                sleep(backoff).await;
                continue;
            }

            return Err(anyhow!("OpenAI API 错误 {status}: {body}"));
        }

        unreachable!()
    }

    /// 发送消息并获取回复
    pub async fn create_message(
        &self,
        request: &ProviderRequest<'_>,
    ) -> AgentResult<ProviderResponse> {
        let messages = convert_messages(request.system, request.messages);
        let tools = convert_tools(request.tools);

        let openai_request = OpenAIRequest {
            model: request.model.to_owned(),
            messages,
            tools,
            max_tokens: request.max_tokens,
        };

        let openai_response = self.call_api(&openai_request).await?;
        Ok(convert_response(openai_response))
    }
}

/// 将内部 ApiMessage 列表转换为 OpenAI 消息列表
fn convert_messages(system: &str, messages: &[ApiMessage]) -> Vec<OpenAIMessage> {
    let mut openai_messages = Vec::with_capacity(messages.len() + 1);

    // 系统提示词作为第一条 system 消息
    if !system.is_empty() {
        openai_messages.push(OpenAIMessage::System {
            content: system.to_owned(),
        });
    }

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                // 检查用户消息中是否包含 tool_result 块，需要拆分为多条 tool 消息
                if let Some(blocks) = msg.content.as_array() {
                    let has_tool_result = blocks
                        .iter()
                        .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("tool_result"));

                    if has_tool_result {
                        // 将 tool_result 块转换为 OpenAI 的 tool 消息
                        for block in blocks {
                            if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                let tool_id = block
                                    .get("tool_use_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_owned();
                                let content = block
                                    .get("content")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_owned();
                                openai_messages.push(OpenAIMessage::Tool {
                                    tool_call_id: tool_id,
                                    content,
                                });
                            } else {
                                // 非 tool_result 块作为普通文本
                                if let Some(text) = block.get("text").and_then(|v| v.as_str())
                                    && !text.is_empty()
                                {
                                    openai_messages.push(OpenAIMessage::User {
                                        content: Some(serde_json::Value::String(text.to_owned())),
                                    });
                                }
                            }
                        }
                        continue;
                    }
                }
                // 普通用户消息直接透传
                openai_messages.push(OpenAIMessage::User {
                    content: Some(msg.content.clone()),
                });
            }
            "assistant" => {
                let (text, tool_calls, reasoning_content) =
                    extract_assistant_parts(&msg.content);
                openai_messages.push(OpenAIMessage::Assistant {
                    content: if text.is_empty() { None } else { Some(text) },
                    tool_calls,
                    reasoning_content,
                });
            }
            _ => {
                // 其他角色直接透传
                openai_messages.push(OpenAIMessage::User {
                    content: Some(msg.content.clone()),
                });
            }
        }
    }

    openai_messages
}

/// 从助手消息的内容块中提取纯文本、工具调用和思考内容
fn extract_assistant_parts(
    content: &serde_json::Value,
) -> (String, Vec<OpenAIToolCall>, Option<String>) {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut reasoning = String::new();

    // 纯文本消息
    if let Some(s) = content.as_str() {
        return (s.to_owned(), Vec::new(), None);
    }

    // 内容块数组
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        text.push_str(t);
                    }
                }
                "thinking" => {
                    if let Some(t) = block.get("thinking").and_then(|v| v.as_str()) {
                        if !reasoning.is_empty() {
                            reasoning.push('\n');
                        }
                        reasoning.push_str(t);
                    }
                }
                "tool_use" => {
                    if let (Some(id), Some(name), Some(input)) = (
                        block.get("id").and_then(|v| v.as_str()),
                        block.get("name").and_then(|v| v.as_str()),
                        block.get("input"),
                    ) {
                        tool_calls.push(OpenAIToolCall {
                            id: id.to_owned(),
                            r#type: "function".to_owned(),
                            function: OpenAIFunctionCall {
                                name: name.to_owned(),
                                arguments: input.to_string(),
                            },
                        });
                    }
                }
                _ => {}
            }
        }
    }

    let reasoning_content = if reasoning.is_empty() {
        None
    } else {
        Some(reasoning)
    };
    (text, tool_calls, reasoning_content)
}

/// 将内部工具 schema 转换为 OpenAI 工具定义
fn convert_tools(tools: &[serde_json::Value]) -> Option<Vec<OpenAIToolOwned>> {
    if tools.is_empty() {
        return None;
    }

    let openai_tools: Vec<OpenAIToolOwned> = tools
        .iter()
        .map(|tool| {
            // 内部格式：{ "name": "...", "description": "...", "input_schema": {...} }
            // OpenAI 格式：{ "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
            let name = tool.get("name").cloned().unwrap_or(serde_json::Value::Null);
            let description = tool
                .get("description")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let parameters = tool
                .get("input_schema")
                .or_else(|| tool.get("parameters"))
                .cloned()
                .unwrap_or(serde_json::json!({}));

            OpenAIToolOwned {
                r#type: "function".to_owned(),
                function: serde_json::json!({
                    "name": name,
                    "description": description,
                    "parameters": parameters,
                }),
            }
        })
        .collect();

    if openai_tools.is_empty() {
        None
    } else {
        Some(openai_tools)
    }
}

/// 将 OpenAI 响应转换为统一的 ProviderResponse
fn convert_response(response: OpenAIResponse) -> ProviderResponse {
    let mut content_blocks = Vec::new();
    let mut stop_reason = String::new();

    if let Some(choice) = response.choices.first() {
        stop_reason = match choice.finish_reason.as_deref() {
            Some("tool_calls") | Some("function_call") => "tool_calls".to_owned(),
            Some("stop") => "end_turn".to_owned(),
            Some(other) => other.to_owned(),
            None => String::new(),
        };

        // 提取文本内容
        if let Some(text) = &choice.message.content
            && !text.is_empty()
        {
            content_blocks.push(ResponseContentBlock::Text { text: text.clone() });
        }

        // 提取工具调用
        if let Some(calls) = &choice.message.tool_calls {
            for call in calls {
                let input: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                content_blocks.push(ResponseContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    input,
                });
            }
        }
    }

    ProviderResponse {
        content: content_blocks,
        stop_reason,
        usage: TokenUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
            cache_read_tokens: {
                let from_prompt = response
                    .usage
                    .prompt_tokens_details
                    .as_ref()
                    .map(|d| d.cached_tokens)
                    .unwrap_or(0);
                let from_input = response
                    .usage
                    .input_tokens_details
                    .as_ref()
                    .map(|d| d.cached_tokens)
                    .unwrap_or(0);
                from_prompt + from_input
            },
            cache_creation_tokens: 0,
        },
    }
}
