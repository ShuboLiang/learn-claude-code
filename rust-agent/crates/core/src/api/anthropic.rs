//! Anthropic API 客户端
//!
//! 封装与 Claude Messages API 的 HTTP 通信逻辑。
//! 重试行为对齐 Claude Code 官方实现：
//! - 默认最大重试 10 次（可通过 `RUST_AGENT_MAX_RETRIES` 环境变量覆盖）
//! - 默认请求超时 10 分钟（可通过 `RUST_AGENT_API_TIMEOUT_MS` 环境变量覆盖，单位毫秒）
//! - 对 429（限流）、529（过载）、5xx（服务器错误）、连接错误进行指数退避重试
//! - 429 响应优先解析 `Retry-After` 响应头

use std::time::Duration;

use anyhow::{Context, anyhow};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use tokio::time::sleep;

use super::retry::{self, CancelFlag, RetryNotifier};
use super::types::{
    LlmStreamChunk, MessagesRequest, MessagesResponse, ProviderRequest, ProviderResponse,
    ResponseContentBlock, TokenUsage,
};
use futures::stream::{self, BoxStream};
use futures::StreamExt;
use crate::AgentResult;

// ── Anthropic SSE 流式事件类型 ──

#[derive(Debug, Deserialize)]
struct AnthropicMessageStart {
    #[serde(default)]
    usage: AnthropicStreamUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart { message: AnthropicMessageStart },
    ContentBlockStart { index: u32, content_block: AnthropicContentBlock },
    ContentBlockDelta { index: u32, delta: AnthropicDelta },
    ContentBlockStop { index: u32 },
    MessageDelta { delta: AnthropicMessageDelta, usage: AnthropicStreamUsage },
    MessageStop,
    Ping,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicMessageDelta {
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicStreamUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// 工具调用构建器，用于在 SSE 流中累积工具参数
#[derive(Debug)]
struct ToolUseBuilder {
    id: String,
    name: String,
    input_json: String,
}

/// Anthropic SSE 流解析器状态机
#[derive(Debug)]
struct AnthropicStreamParser {
    current_tool: Option<ToolUseBuilder>,
    usage: AnthropicStreamUsage,
    stop_reason: Option<String>,
}

impl AnthropicStreamParser {
    fn new() -> Self {
        Self {
            current_tool: None,
            usage: AnthropicStreamUsage::default(),
            stop_reason: None,
        }
    }

    fn feed_event(&mut self, event: AnthropicStreamEvent) -> Vec<LlmStreamChunk> {
        let mut chunks = Vec::new();
        match event {
            AnthropicStreamEvent::MessageStart { message } => {
                self.usage.input_tokens = message.usage.input_tokens;
            }
            AnthropicStreamEvent::ContentBlockStart { content_block, .. } => {
                match content_block {
                    AnthropicContentBlock::ToolUse { id, name } => {
                        self.current_tool = Some(ToolUseBuilder {
                            id: id.clone(),
                            name: name.clone(),
                            input_json: String::new(),
                        });
                        chunks.push(LlmStreamChunk::ToolUseStart { id, name });
                    }
                    AnthropicContentBlock::Text { .. } => {}
                }
            }
            AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
                match delta {
                    AnthropicDelta::TextDelta { text } => {
                        chunks.push(LlmStreamChunk::TextDelta(text));
                    }
                    AnthropicDelta::InputJsonDelta { partial_json } => {
                        if let Some(ref mut tool) = self.current_tool {
                            tool.input_json.push_str(&partial_json);
                            chunks.push(LlmStreamChunk::ToolUseDelta {
                                id: tool.id.clone(),
                                input_json_delta: partial_json,
                            });
                        }
                    }
                }
            }
            AnthropicStreamEvent::ContentBlockStop { .. } => {
                if let Some(tool) = self.current_tool.take() {
                    chunks.push(LlmStreamChunk::ToolUseEnd { id: tool.id });
                }
            }
            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                if let Some(reason) = delta.stop_reason {
                    let mapped = if reason == "tool_use" {
                        "tool_calls".to_string()
                    } else {
                        reason
                    };
                    self.stop_reason = Some(mapped.clone());
                    chunks.push(LlmStreamChunk::StopReason(mapped));
                }
                self.usage.output_tokens += usage.output_tokens;
            }
            AnthropicStreamEvent::MessageStop => {
                if self.usage.input_tokens > 0 || self.usage.output_tokens > 0 {
                    chunks.push(LlmStreamChunk::Usage(TokenUsage {
                        input_tokens: self.usage.input_tokens,
                        output_tokens: self.usage.output_tokens,
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                    }));
                }
                chunks.push(LlmStreamChunk::Done);
            }
            AnthropicStreamEvent::Ping => {}
        }
        chunks
    }
}

/// 解析 Anthropic SSE 单行数据
fn parse_anthropic_sse_line(line: &str) -> Option<AnthropicStreamEvent> {
    let line = line.trim();
    if !line.starts_with("data: ") {
        return None;
    }
    let json = &line["data: ".len()..];
    if json == "[DONE]" {
        return None;
    }
    serde_json::from_str(json).ok()
}

fn parse_messages_response(body: &str) -> AgentResult<MessagesResponse> {
    serde_json::from_str(body)
        .with_context(|| format!("解析 Anthropic 响应 JSON 失败，响应体: {body}"))
}

/// Anthropic API 的协议版本号
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic API 的 HTTP 客户端，封装了认证、超时和重试逻辑
#[derive(Clone, Debug)]
pub struct AnthropicClient {
    /// reqwest HTTP 客户端（已配置默认请求头）
    http: reqwest::Client,
    /// Anthropic API 密钥
    api_key: String,
    /// API 基础 URL（默认为 https://api.anthropic.com，可自定义用于代理）
    base_url: String,
    /// 最大重试次数
    max_retries: u32,
}

impl AnthropicClient {
    /// 使用给定的 API 密钥和基础 URL 创建客户端
    pub fn new(api_key: &str, base_url: &str) -> AgentResult<Self> {
        Self::build_client(api_key, base_url)
    }

    /// 内部构造方法
    fn build_client(api_key: &str, base_url: &str) -> AgentResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

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

    /// 调用 Claude Messages API，发送请求并获取回复
    ///
    /// 支持对 429（限流）、529（过载）、5xx（服务器错误）、连接错误的指数退避重试。
    pub(crate) async fn create_message_raw(
        &self,
        request: &MessagesRequest<'_>,
        retry_notifier: Option<&RetryNotifier>,
        cancel: Option<&CancelFlag>,
    ) -> AgentResult<MessagesResponse> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        for attempt in 0..=self.max_retries {
            let send_result = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .json(request)
                .send()
                .await;

            let response = match send_result {
                Ok(resp) => resp,
                Err(e) => {
                    let is_retryable = e.is_timeout() || e.is_connect();
                    if is_retryable && attempt < self.max_retries && !retry::is_cancelled(cancel) {
                        let backoff = retry::calculate_backoff(None, attempt);
                        retry::notify_retry(
                            "Anthropic",
                            &format!("请求失败: {}", retry::format_reqwest_error(&e)),
                            backoff,
                            attempt,
                            self.max_retries,
                            retry_notifier,
                        );
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(anyhow!(
                        "调用 Anthropic Messages API 失败 (URL: {}): {}",
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
                    if attempt < self.max_retries && !retry::is_cancelled(cancel) {
                        let backoff = retry::calculate_backoff(retry_after, attempt);
                        retry::notify_retry(
                            "Anthropic",
                            "读取 Anthropic 响应体失败",
                            backoff,
                            attempt,
                            self.max_retries,
                            retry_notifier,
                        );
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(anyhow!("读取 Anthropic 响应体失败: {}", e));
                }
            };

            // 使用 lossy 转换避免 UTF-8 编码问题
            let body = String::from_utf8_lossy(&body_bytes).into_owned();

            if status.is_success() {
                return parse_messages_response(&body);
            }

            // 对可重试状态码进行重试（429, 529, 5xx），但客户端已断开则立即终止
            if retry::is_retryable_status(status, &[529]) && attempt < self.max_retries && !retry::is_cancelled(cancel) {
                let backoff = retry::calculate_backoff(retry_after, attempt);
                retry::notify_retry(
                    "Anthropic",
                    &format!("返回 {status}"),
                    backoff,
                    attempt,
                    self.max_retries,
                    retry_notifier,
                );
                sleep(backoff).await;
                continue;
            }

            return Err(crate::api::error::LlmApiError {
                status: status.as_u16(),
                body,
                retry_after,
            }
            .into());
        }

        unreachable!()
    }
}

impl AnthropicClient {
    /// 流式发送消息
    pub(crate) async fn stream_message(
        &self,
        request: &ProviderRequest<'_>,
        retry_notifier: Option<&RetryNotifier>,
        cancel: Option<&CancelFlag>,
    ) -> AgentResult<BoxStream<'static, AgentResult<LlmStreamChunk>>> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        let raw_request = MessagesRequest {
            model: request.model,
            system: request.system,
            messages: request.messages,
            tools: request.tools,
            max_tokens: request.max_tokens,
        };

        let mut body = serde_json::to_value(&raw_request)
            .context("序列化 Anthropic 请求体失败")?;
        body["stream"] = serde_json::Value::Bool(true);

        for attempt in 0..=self.max_retries {
            if retry::is_cancelled(cancel) {
                return Err(anyhow!("请求已取消"));
            }

            let send_result = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .json(&body)
                .send()
                .await;

            let response = match send_result {
                Ok(resp) => resp,
                Err(e) => {
                    let is_retryable = e.is_timeout() || e.is_connect();
                    if is_retryable && attempt < self.max_retries && !retry::is_cancelled(cancel) {
                        let backoff = retry::calculate_backoff(None, attempt);
                        retry::notify_retry(
                            "Anthropic",
                            &format!("请求失败: {}", retry::format_reqwest_error(&e)),
                            backoff,
                            attempt,
                            self.max_retries,
                            retry_notifier,
                        );
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(anyhow!(
                        "调用 Anthropic Messages API 失败 (URL: {}): {}",
                        url,
                        retry::format_reqwest_error(&e)
                    ));
                }
            };

            let status = response.status();
            let retry_after = retry::parse_retry_after(&response);

            if !status.is_success() {
                let body_bytes = match response.bytes().await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        if attempt < self.max_retries && !retry::is_cancelled(cancel) {
                            let backoff = retry::calculate_backoff(retry_after, attempt);
                            retry::notify_retry(
                                "Anthropic",
                                "读取 Anthropic 响应体失败",
                                backoff,
                                attempt,
                                self.max_retries,
                                retry_notifier,
                            );
                            sleep(backoff).await;
                            continue;
                        }
                        return Err(anyhow!("读取 Anthropic 响应体失败: {}", e));
                    }
                };

                let body = String::from_utf8_lossy(&body_bytes).into_owned();

                if retry::is_retryable_status(status, &[529])
                    && attempt < self.max_retries
                    && !retry::is_cancelled(cancel)
                {
                    let backoff = retry::calculate_backoff(retry_after, attempt);
                    retry::notify_retry(
                        "Anthropic",
                        &format!("返回 {status}"),
                        backoff,
                        attempt,
                        self.max_retries,
                        retry_notifier,
                    );
                    sleep(backoff).await;
                    continue;
                }

                return Err(crate::api::error::LlmApiError {
                    status: status.as_u16(),
                    body,
                    retry_after,
                }
                .into());
            }

            // 成功响应，检查 Content-Type
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if !content_type.starts_with("text/event-stream") {
                // 非 SSE 回退到阻塞解析
                let body_bytes = response
                    .bytes()
                    .await
                    .context("读取 Anthropic 响应体失败")?;
                let body = String::from_utf8_lossy(&body_bytes);
                let parsed = parse_messages_response(&body)?;

                let mut chunks: Vec<AgentResult<LlmStreamChunk>> = Vec::new();

                let text = parsed
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ResponseContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>();

                if !text.is_empty() {
                    chunks.push(Ok(LlmStreamChunk::TextDelta(text)));
                }

                let stop_reason = match parsed.stop_reason.as_deref() {
                    Some("tool_use") => "tool_calls".to_owned(),
                    Some(other) => other.to_owned(),
                    None => String::new(),
                };

                if !stop_reason.is_empty() {
                    chunks.push(Ok(LlmStreamChunk::StopReason(stop_reason)));
                }

                chunks.push(Ok(LlmStreamChunk::Usage(TokenUsage {
                    input_tokens: parsed.usage.input_tokens,
                    output_tokens: parsed.usage.output_tokens,
                    cache_read_tokens: parsed.usage.cache_read_input_tokens,
                    cache_creation_tokens: parsed.usage.cache_creation_input_tokens,
                })));
                chunks.push(Ok(LlmStreamChunk::Done));

                return Ok(Box::pin(stream::iter(chunks)));
            }

            // SSE 流式处理
            let (mut event_tx, event_rx) = futures::channel::mpsc::channel(128);

            tokio::spawn(async move {
                let mut stream = response.bytes_stream();
                let mut parser = AnthropicStreamParser::new();
                let mut buffer = String::new();

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(bytes) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));

                            while let Some(pos) = buffer.find("\n\n") {
                                let event_text = buffer[..pos].to_string();
                                buffer.replace_range(..pos + 2, "");

                                for line in event_text.lines() {
                                    if let Some(evt) = parse_anthropic_sse_line(line) {
                                        for c in parser.feed_event(evt) {
                                            if event_tx.try_send(Ok(c)).is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.try_send(Err(anyhow!("读取 SSE 流失败: {}", e)));
                            return;
                        }
                    }
                }

                // 处理缓冲区中剩余的数据
                if !buffer.is_empty() {
                    for line in buffer.lines() {
                        if let Some(evt) = parse_anthropic_sse_line(line) {
                            for c in parser.feed_event(evt) {
                                if event_tx.try_send(Ok(c)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            });

            return Ok(Box::pin(event_rx));
        }

        unreachable!()
    }

    /// 发送消息并获取统一的 ProviderResponse
    pub async fn create_message(
        &self,
        request: &ProviderRequest<'_>,
        retry_notifier: Option<&RetryNotifier>,
        cancel: Option<&CancelFlag>,
    ) -> AgentResult<ProviderResponse> {
        let raw_request = MessagesRequest {
            model: request.model,
            system: request.system,
            messages: request.messages,
            tools: request.tools,
            max_tokens: request.max_tokens,
        };

        let raw_response = self.create_message_raw(&raw_request, retry_notifier, cancel).await?;

        // 统一 stop_reason：将 Anthropic 的 "tool_use" 映射为 "tool_calls"
        let stop_reason = match raw_response.stop_reason.as_deref() {
            Some("tool_use") => "tool_calls".to_owned(),
            Some(other) => other.to_owned(),
            None => String::new(),
        };

        Ok(ProviderResponse {
            content: raw_response.content,
            stop_reason,
            usage: TokenUsage {
                input_tokens: raw_response.usage.input_tokens,
                output_tokens: raw_response.usage.output_tokens,
                cache_read_tokens: raw_response.usage.cache_read_input_tokens,
                cache_creation_tokens: raw_response.usage.cache_creation_input_tokens,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResponseContentBlock;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// 启动一个极简 HTTP mock 服务器，按顺序返回预设的响应
    async fn mock_server(
        responses: Vec<(u16, &'static str, Option<&'static str>)>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", listener.local_addr().unwrap());
        let counter = Arc::new(AtomicUsize::new(0));

        tokio::spawn({
            let counter = counter.clone();
            async move {
                for (status, body, retry_after) in responses {
                    let (mut stream, _) = listener.accept().await.unwrap();

                    // 读取 HTTP 请求头（读到 \r\n\r\n 为止）
                    let mut buf = [0u8; 4096];
                    let mut pos = 0;
                    loop {
                        let n = stream.read(&mut buf[pos..]).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        pos += n;
                        if pos >= 4 && buf[..pos].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }

                    let retry_header = retry_after
                        .map(|v| format!("Retry-After: {v}\r\n"))
                        .unwrap_or_default();

                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{retry_header}\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;

                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }
        });

        (addr, counter)
    }

    /// 串行化修改环境变量，避免并行测试互相干扰
    fn with_max_retries<T>(retries: u32, f: impl FnOnce() -> T) -> T {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let _guard = LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();

        let old = std::env::var("RUST_AGENT_MAX_RETRIES").ok();
        // SAFETY：测试在单线程串行锁保护下修改环境变量，不会与其他线程并发访问
        unsafe {
            std::env::set_var("RUST_AGENT_MAX_RETRIES", retries.to_string());
        }
        let result = f();
        // SAFETY：同上
        unsafe {
            match old {
                Some(v) => std::env::set_var("RUST_AGENT_MAX_RETRIES", v),
                None => std::env::remove_var("RUST_AGENT_MAX_RETRIES"),
            }
        }
        result
    }

    #[test]
    fn parse_messages_response_should_include_body_when_json_is_invalid() {
        let err = parse_messages_response("not json").expect_err("无效 JSON 应返回错误");
        let err_text = format!("{err:#}");

        assert!(
            err_text.contains("not json"),
            "错误链应包含原始 body，实际为: {err_text}"
        );
        assert!(
            err_text.contains("expected ident") || err_text.contains("expected value"),
            "错误链应包含解析原因，实际为: {err_text}"
        );
    }

    #[test]
    fn parse_messages_response_should_accept_thinking_blocks_without_exposing_them_in_final_text() {
        let body = r#"
        {
          "content": [
            {
              "type": "thinking",
              "thinking": "I should inspect the weather tool first.",
              "signature": "sig_test"
            },
            {
              "type": "tool_use",
              "id": "toolu_123",
              "name": "get_weather",
              "input": {"city": "Shanghai"}
            }
          ],
          "stop_reason": "tool_use",
          "usage": {"input_tokens": 20, "output_tokens": 10}
        }
        "#;

        let response = parse_messages_response(body).expect("含 thinking 的响应应能被解析");
        let provider_response = ProviderResponse {
            content: response.content,
            stop_reason: "tool_calls".to_owned(),
            usage: TokenUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                cache_read_tokens: response.usage.cache_read_input_tokens,
                cache_creation_tokens: response.usage.cache_creation_input_tokens,
            },
        };

        assert_eq!(provider_response.final_text(), "");
        assert!(matches!(
            provider_response.content.get(1),
            Some(ResponseContentBlock::ToolUse { name, .. }) if name == "get_weather"
        ));
    }

    #[tokio::test]
    async fn should_retry_429_with_retry_after_and_succeed() {
        let (url, counter) = mock_server(vec![
            (
                429,
                r#"{"error":{"type":"rate_limit_error","message":"Rate limited"}}"#,
                Some("1"),
            ),
            (
                200,
                r#"{"content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}"#,
                None,
            ),
        ])
        .await;

        let client = with_max_retries(2, || AnthropicClient::new("fake-key", &url).unwrap());

        let request = MessagesRequest {
            model: "claude-test",
            system: "",
            messages: &[],
            tools: &[],
            max_tokens: 100,
        };

        let result = client.create_message_raw(&request, None, None).await;
        assert!(result.is_ok(), "应在第二次请求时成功: {:?}", result);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "应先收到 429，然后重试成功"
        );
    }

    #[tokio::test]
    async fn should_retry_500_without_retry_after_and_succeed() {
        let (url, counter) = mock_server(vec![
            (
                500,
                r#"{"error":{"type":"api_error","message":"Internal server error"}}"#,
                None,
            ),
            (
                200,
                r#"{"content":[{"type":"text","text":"world"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}"#,
                None,
            ),
        ])
        .await;

        let client = with_max_retries(2, || AnthropicClient::new("fake-key", &url).unwrap());

        let request = MessagesRequest {
            model: "claude-test",
            system: "",
            messages: &[],
            tools: &[],
            max_tokens: 100,
        };

        let start = std::time::Instant::now();
        let result = client.create_message_raw(&request, None, None).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok(), "应在第二次请求时成功: {:?}", result);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        // 无 Retry-After 时使用指数退避：attempt=0 → 1 秒
        assert!(
            elapsed >= Duration::from_millis(900),
            "应至少等待约 1 秒退避时间，实际 {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn should_not_retry_400_bad_request() {
        let (url, counter) = mock_server(vec![(
            400,
            r#"{"error":{"type":"invalid_request_error","message":"Bad request"}}"#,
            None,
        )])
        .await;

        let client = with_max_retries(2, || AnthropicClient::new("fake-key", &url).unwrap());

        let request = MessagesRequest {
            model: "claude-test",
            system: "",
            messages: &[],
            tools: &[],
            max_tokens: 100,
        };

        let result = client.create_message_raw(&request, None, None).await;
        assert!(result.is_err(), "400 错误应直接失败，不应重试");
        assert_eq!(counter.load(Ordering::SeqCst), 1, "400 错误不应触发重试");
    }

    #[test]
    fn test_anthropic_parser_text_only() {
        let mut parser = AnthropicStreamParser::new();
        let mut chunks = Vec::new();

        chunks.extend(parser.feed_event(AnthropicStreamEvent::MessageStart {
            message: AnthropicMessageStart {
                usage: AnthropicStreamUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                },
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockStart {
            index: 0,
            content_block: AnthropicContentBlock::Text {
                text: String::new(),
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicDelta::TextDelta {
                text: "Hello".to_string(),
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicDelta::TextDelta {
                text: " world".to_string(),
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockStop { index: 0 }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::MessageDelta {
            delta: AnthropicMessageDelta {
                stop_reason: Some("end_turn".to_string()),
                stop_sequence: None,
            },
            usage: AnthropicStreamUsage {
                input_tokens: 0,
                output_tokens: 2,
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::MessageStop));

        assert_eq!(chunks.len(), 5);
        assert!(matches!(&chunks[0], LlmStreamChunk::TextDelta(t) if t == "Hello"));
        assert!(matches!(&chunks[1], LlmStreamChunk::TextDelta(t) if t == " world"));
        assert!(matches!(&chunks[2], LlmStreamChunk::StopReason(r) if r == "end_turn"));
        assert!(
            matches!(&chunks[3], LlmStreamChunk::Usage(u) if u.input_tokens == 10 && u.output_tokens == 2)
        );
        assert!(matches!(&chunks[4], LlmStreamChunk::Done));
    }

    #[test]
    fn test_anthropic_parser_tool_use() {
        let mut parser = AnthropicStreamParser::new();
        let mut chunks = Vec::new();

        chunks.extend(parser.feed_event(AnthropicStreamEvent::MessageStart {
            message: AnthropicMessageStart {
                usage: AnthropicStreamUsage {
                    input_tokens: 20,
                    output_tokens: 0,
                },
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockStart {
            index: 0,
            content_block: AnthropicContentBlock::ToolUse {
                id: "toolu_123".to_string(),
                name: "get_weather".to_string(),
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicDelta::InputJsonDelta {
                partial_json: r#"{"city":"#.to_string(),
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicDelta::InputJsonDelta {
                partial_json: r#""Shanghai"}"#.to_string(),
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::ContentBlockStop { index: 0 }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::MessageDelta {
            delta: AnthropicMessageDelta {
                stop_reason: Some("tool_use".to_string()),
                stop_sequence: None,
            },
            usage: AnthropicStreamUsage {
                input_tokens: 0,
                output_tokens: 15,
            },
        }));
        chunks.extend(parser.feed_event(AnthropicStreamEvent::MessageStop));

        assert_eq!(chunks.len(), 7);
        assert!(
            matches!(&chunks[0], LlmStreamChunk::ToolUseStart { id, name } if id == "toolu_123" && name == "get_weather")
        );
        assert!(
            matches!(&chunks[1], LlmStreamChunk::ToolUseDelta { id, input_json_delta } if id == "toolu_123" && input_json_delta == r#"{"city":"#)
        );
        assert!(
            matches!(&chunks[2], LlmStreamChunk::ToolUseDelta { id, input_json_delta } if id == "toolu_123" && input_json_delta == r#""Shanghai"}"#)
        );
        assert!(
            matches!(&chunks[3], LlmStreamChunk::ToolUseEnd { id } if id == "toolu_123")
        );
        assert!(matches!(&chunks[4], LlmStreamChunk::StopReason(r) if r == "tool_calls"));
        assert!(
            matches!(&chunks[5], LlmStreamChunk::Usage(u) if u.input_tokens == 20 && u.output_tokens == 15)
        );
        assert!(matches!(&chunks[6], LlmStreamChunk::Done));
    }
}
