//! Anthropic API 客户端模块
//!
//! 封装与 Claude Messages API 的 HTTP 通信逻辑，包含认证、请求发送和重试。

pub mod types;

use std::time::Duration;

use anyhow::{anyhow, Context};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use tokio::time::sleep;

use crate::AgentResult;
use types::{MessagesRequest, MessagesResponse};

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
    /// 从环境变量创建 Anthropic API 客户端
    ///
    /// # 读取的环境变量
    /// - `ANTHROPIC_API_KEY`: API 密钥（必需）
    /// - `ANTHROPIC_BASE_URL`: 自定义 API 地址（可选，默认 `https://api.anthropic.com`）
    pub fn from_env() -> AgentResult<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("Missing ANTHROPIC_API_KEY in environment or .env")?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_owned());

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
            api_key,
            base_url,
        })
    }

    /// 调用 Claude Messages API，发送请求并获取回复
    ///
    /// 支持对 429（限流）和 529（过载）状态码的指数退避重试（最多 5 次）。
    pub async fn create_message(
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
