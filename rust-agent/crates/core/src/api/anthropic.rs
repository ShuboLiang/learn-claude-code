//! Anthropic API 客户端
//!
//! 封装与 Claude Messages API 的 HTTP 通信逻辑。

use std::time::Duration;

use anyhow::{anyhow, Context};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use tokio::time::sleep;

use super::types::{MessagesRequest, MessagesResponse, ProviderRequest, ProviderResponse};
use crate::AgentResult;

/// Anthropic API 的协议版本号
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic API 的 HTTP 客户端，封装了认证和请求发送逻辑
#[derive(Clone, Debug)]
pub struct AnthropicClient {
    /// reqwest HTTP 客户端（已配置默认请求头）
    http: reqwest::Client,
    /// Anthropic API 密钥
    api_key: String,
    /// API 基础 URL（默认为 https://api.anthropic.com，可自定义用于代理）
    base_url: String,
}

impl AnthropicClient {
    /// 使用给定的 API 密钥和基础 URL 创建客户端
    pub fn new(api_key: &str, base_url: &str) -> AgentResult<Self> {
        Self::build_client(api_key, base_url)
    }

    /// 从环境变量创建 Anthropic API 客户端
    pub fn from_env() -> AgentResult<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("Missing ANTHROPIC_API_KEY in environment or .env")?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_owned());
        Self::build_client(&api_key, &base_url)
    }

    /// 内部构造方法
    fn build_client(api_key: &str, base_url: &str) -> AgentResult<Self> {

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            http,
            api_key: api_key.to_owned(),
            base_url: base_url.to_owned(),
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
    /// 支持对 429（限流）和 529（过载）状态码的指数退避重试（最多 5 次）。
    pub(crate) async fn create_message_raw(
        &self,
        request: &MessagesRequest<'_>,
    ) -> AgentResult<MessagesResponse> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let max_retries: u32 = 5;

        for attempt in 0..=max_retries {
            let send_result = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .json(request)
                .send()
                .await;

            let response = match send_result {
                Ok(resp) => resp,
                Err(e) if e.to_string().contains("UTF-8") && attempt < max_retries => {
                    let wait = Duration::from_secs(1 << attempt);
                    sleep(wait).await;
                    continue;
                }
                Err(e) => return Err(e).context("调用 Anthropic Messages API 失败"),
            };

            let status = response.status();

            let body_bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    if attempt < max_retries {
                        let wait = Duration::from_secs(1 << attempt);
                        sleep(wait).await;
                        continue;
                    }
                    return Err(e).context("读取 Anthropic 响应体失败");
                }
            };

            let body = match String::from_utf8(body_bytes.to_vec()) {
                Ok(s) => s,
                Err(_) => String::from_utf8_lossy(&body_bytes).into_owned(),
            };

            if status.is_success() {
                return serde_json::from_str(&body)
                    .context("解析 Anthropic 响应 JSON 失败");
            }

            // 仅对 429（限流）和 529（过载）进行重试
            if (status.as_u16() == 429 || status.as_u16() == 529) && attempt < max_retries {
                let wait = Duration::from_secs(1 << attempt);
                sleep(wait).await;
                continue;
            }

            return Err(anyhow!("Anthropic API 错误 {status}: {body}"));
        }

        unreachable!()
    }
}

impl AnthropicClient {
    /// 发送消息并获取统一的 ProviderResponse
    pub async fn create_message(
        &self,
        request: &ProviderRequest<'_>,
    ) -> AgentResult<ProviderResponse> {
        let raw_request = MessagesRequest {
            model: request.model,
            system: request.system,
            messages: request.messages,
            tools: request.tools,
            max_tokens: request.max_tokens,
        };

        let raw_response = self.create_message_raw(&raw_request).await?;

        // 统一 stop_reason：将 Anthropic 的 "tool_use" 映射为 "tool_calls"
        let stop_reason = match raw_response.stop_reason.as_deref() {
            Some("tool_use") => "tool_calls".to_owned(),
            Some(other) => other.to_owned(),
            None => String::new(),
        };

        Ok(ProviderResponse {
            content: raw_response.content,
            stop_reason,
        })
    }
}
