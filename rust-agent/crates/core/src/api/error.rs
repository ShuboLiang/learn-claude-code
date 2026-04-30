//! LLM API 调用错误类型

use std::time::Duration;

/// 结构化 LLM API 错误，包含 HTTP 状态码和可选的 Retry-After 信息
#[derive(Debug)]
pub struct LlmApiError {
    /// HTTP 状态码（如 429、500 等）
    pub status: u16,
    /// 响应体内容
    pub body: String,
    /// Retry-After 头指示的等待时间
    pub retry_after: Option<Duration>,
}

impl LlmApiError {
    /// 判断是否为 429 限流错误
    pub fn is_rate_limited(&self) -> bool {
        self.status == 429
    }

    /// 获取 Retry-After 的秒数
    pub fn retry_after_secs(&self) -> Option<u64> {
        self.retry_after.map(|d| d.as_secs())
    }
}

impl std::fmt::Display for LlmApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API 错误 {}: {}", self.status, self.body)
    }
}

impl std::error::Error for LlmApiError {}
