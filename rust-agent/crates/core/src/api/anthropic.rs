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
use tokio::time::sleep;

use super::types::{MessagesRequest, MessagesResponse, ProviderRequest, ProviderResponse};
use crate::AgentResult;

/// Anthropic API 的协议版本号
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// 默认最大重试次数（对齐 Claude Code）
const DEFAULT_MAX_RETRIES: u32 = 10;

/// 默认请求超时时间（毫秒），10 分钟（对齐 Claude Code）
const DEFAULT_API_TIMEOUT_MS: u64 = 600_000;

/// 默认连接超时（秒）
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30;

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

        // 从环境变量读取超时配置，默认 10 分钟
        let timeout_ms = std::env::var("RUST_AGENT_API_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_API_TIMEOUT_MS);

        let connect_timeout = Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS);
        let timeout = Duration::from_millis(timeout_ms);

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(connect_timeout)
            .timeout(timeout)
            .build()
            .context("构建 HTTP 客户端失败")?;

        // 从环境变量读取最大重试次数，默认 10
        let max_retries = std::env::var("RUST_AGENT_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DEFAULT_MAX_RETRIES);

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

    /// 判断 HTTP 状态码是否属于可重试的错误
    ///
    /// 对齐 Claude Code 的可重试条件：
    /// - 429：限流（容量不足，非账户配额耗尽）
    /// - 529：服务器过载
    /// - 5xx：服务器内部错误
    fn is_retryable_status(status: reqwest::StatusCode) -> bool {
        let code = status.as_u16();
        code == 429 || code == 529 || (code >= 500 && code < 600)
    }

    /// 判断网络错误是否属于可重试的类型
    ///
    /// 可重试的网络错误：连接重置、连接拒绝、超时等瞬态故障
    fn is_retryable_error(&self, err: &reqwest::Error) -> bool {
        let err_str = err.to_string().to_lowercase();
        // 连接相关错误
        err_str.contains("connection reset")
            || err_str.contains("connection refused")
            || err_str.contains("econnreset")
            || err_str.contains("econnrefused")
            || err_str.contains("etimedout")
            || err_str.contains("timeout")
            || err.is_timeout()
            || err.is_connect()
    }

    /// 解析 Retry-After 响应头（如果存在）
    ///
    /// 429 限流时服务器可能返回此头，指示客户端等待的秒数
    fn parse_retry_after(response: &reqwest::Response) -> Option<Duration> {
        response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }

    /// 计算退避等待时间
    ///
    /// 优先使用 Retry-After 响应头，否则使用指数退避（1, 2, 4, 8, ... 秒）
    fn calculate_backoff(response: Option<&reqwest::Response>, attempt: u32) -> Duration {
        if let Some(retry_after) = response.and_then(Self::parse_retry_after) {
            return retry_after;
        }
        // 指数退避：2^attempt 秒，最大不超过 60 秒
        let secs = (1 << attempt).min(60);
        Duration::from_secs(secs as u64)
    }

    /// 基于已解析的 Retry-After 值计算退避时间
    ///
    /// 内部辅助方法，避免在 response 被消费后再次借用
    fn calculate_backoff_from_retry_after(retry_after: Option<Duration>, attempt: u32) -> Duration {
        if let Some(delay) = retry_after {
            return delay;
        }
        // 指数退避：2^attempt 秒，最大不超过 60 秒
        let secs = (1 << attempt).min(60);
        Duration::from_secs(secs as u64)
    }

    /// 调用 Claude Messages API，发送请求并获取回复
    ///
    /// 支持对 429（限流）、529（过载）、5xx（服务器错误）、连接错误的指数退避重试。
    pub(crate) async fn create_message_raw(
        &self,
        request: &MessagesRequest<'_>,
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
                    if self.is_retryable_error(&e) && attempt < self.max_retries {
                        let backoff = Self::calculate_backoff(None, attempt);
                        eprintln!(
                            "[Anthropic API 重试] 请求失败（可重试）: {}，等待 {:?} 后重试 ({}/{})",
                            e,
                            backoff,
                            attempt + 1,
                            self.max_retries + 1
                        );
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(anyhow!("调用 Anthropic Messages API 失败: {}", e));
                }
            };

            let status = response.status();

            // 在消费 response 之前解析 Retry-After 响应头
            let retry_after = Self::parse_retry_after(&response);

            let body_bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    if attempt < self.max_retries {
                        let backoff = Self::calculate_backoff_from_retry_after(retry_after, attempt);
                        sleep(backoff).await;
                        continue;
                    }
                    return Err(anyhow!("读取 Anthropic 响应体失败: {}", e));
                }
            };

            // 使用 lossy 转换避免 UTF-8 编码问题
            let body = String::from_utf8_lossy(&body_bytes).into_owned();

            if status.is_success() {
                return serde_json::from_str(&body).context("解析 Anthropic 响应 JSON 失败");
            }

            // 对可重试状态码进行重试（429, 529, 5xx）
            if Self::is_retryable_status(status) && attempt < self.max_retries {
                let backoff = Self::calculate_backoff_from_retry_after(retry_after, attempt);
                eprintln!(
                    "[Anthropic API 重试] 返回 {status}，等待 {:?} 后重试 ({}/{})",
                    backoff,
                    attempt + 1,
                    self.max_retries + 1
                );
                sleep(backoff).await;
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
