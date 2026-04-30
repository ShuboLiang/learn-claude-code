//! API 请求重试与指数退避公共模块
//!
//! 封装可复用的 HTTP 错误重试逻辑，供各 LLM Provider 客户端使用。
//! 对齐 Claude Code 官方实现：
//! - 默认最大重试 10 次（可通过 `RUST_AGENT_MAX_RETRIES` 覆盖）
//! - 默认请求超时 10 分钟（可通过 `RUST_AGENT_API_TIMEOUT_MS` 覆盖，单位毫秒）
//! - 对 429（限流）、5xx（服务器错误）、连接错误进行指数退避重试
//! - 429 响应优先解析 `Retry-After` 响应头
//! - 支持通过 `RetryNotifier` 向客户端实时推送重试进度

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

/// 重试进度通知，由 API 层发送给上层以便推送给客户端
#[derive(Debug, Clone)]
pub struct RetryNotification {
    /// 当前重试次数（0 表示第 1 次重试）
    pub attempt: u32,
    /// 最大重试次数
    pub max_retries: u32,
    /// 本次等待秒数
    pub wait_seconds: u64,
    /// 重试原因描述（如 "返回 429 Too Many Requests"）
    pub detail: String,
}

/// 重试通知发送器（无界通道，避免阻塞 API 调用）
pub type RetryNotifier = mpsc::UnboundedSender<RetryNotification>;

/// 客户端取消标志，由上层 Agent 设置，API 层在重试循环中检查
///
/// 当 HTTP SSE 客户端断开连接时，Agent 层将此标志设为 true，
/// API 层的重试循环检测到后立即终止，避免浪费 API 配额。
pub type CancelFlag = Arc<AtomicBool>;

/// 检查取消标志，如果已取消则返回 true
pub fn is_cancelled(cancel: Option<&CancelFlag>) -> bool {
    cancel.map_or(false, |f| f.load(Ordering::Relaxed))
}

/// 默认最大重试次数（对齐 Claude Code）
pub const DEFAULT_MAX_RETRIES: u32 = 10;

/// 默认请求超时时间（毫秒），10 分钟（对齐 Claude Code）
pub const DEFAULT_API_TIMEOUT_MS: u64 = 600_000;

/// 默认连接超时（秒）
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30;

/// 判断 HTTP 状态码是否属于可重试的错误
///
/// 基础可重试条件：
/// - 429：限流
/// - 5xx：服务器内部错误
///
/// `extra_codes` 允许各 Provider 传入额外的可重试状态码（如 Anthropic 的 529）
pub fn is_retryable_status(status: reqwest::StatusCode, extra_codes: &[u16]) -> bool {
    let code = status.as_u16();
    code == 429 || extra_codes.contains(&code) || (500..600).contains(&code)
}

/// 解析 Retry-After 响应头（如果存在）
///
/// 429 限流时服务器可能返回此头，指示客户端等待的秒数
pub fn parse_retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// 计算指数退避等待时间
///
/// 优先使用 Retry-After 响应头，否则使用指数退避（1, 2, 4, 8, ... 秒）
/// 使用 `saturating_pow` 避免 attempt 过大时发生整数溢出
pub fn calculate_backoff(retry_after: Option<Duration>, attempt: u32) -> Duration {
    if let Some(delay) = retry_after {
        return delay;
    }
    let secs = 2u64.saturating_pow(attempt).min(60);
    Duration::from_secs(secs)
}

/// 将 reqwest 错误及其完整的 source 链展开为单行可读字符串
pub fn format_reqwest_error(err: &reqwest::Error) -> String {
    use std::error::Error;
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(s) = source {
        parts.push(s.to_string());
        source = s.source();
    }
    parts.join(" -> ")
}

/// 从环境变量读取最大重试次数
pub fn max_retries_from_env() -> u32 {
    std::env::var("RUST_AGENT_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_MAX_RETRIES)
}

/// 从环境变量读取 API 超时（毫秒）
pub fn api_timeout_ms_from_env() -> u64 {
    std::env::var("RUST_AGENT_API_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_API_TIMEOUT_MS)
}

/// 打印重试日志到 stderr
pub fn log_retry(provider: &str, detail: &str, backoff: Duration, attempt: u32, max_retries: u32) {
    eprintln!(
        "[{provider} API 重试] {detail}，等待 {backoff:?} 后重试 ({}/{total})",
        attempt + 1,
        total = max_retries + 1
    );
}

/// 打印重试日志并向客户端发送通知（如果提供了 notifier）
pub fn notify_retry(
    provider: &str,
    detail: &str,
    backoff: Duration,
    attempt: u32,
    max_retries: u32,
    notifier: Option<&RetryNotifier>,
) {
    log_retry(provider, detail, backoff, attempt, max_retries);
    if let Some(tx) = notifier {
        let _ = tx.send(RetryNotification {
            attempt,
            max_retries,
            wait_seconds: backoff.as_secs(),
            detail: detail.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn calculate_backoff_without_retry_after() {
        assert_eq!(calculate_backoff(None, 0), Duration::from_secs(1));
        assert_eq!(calculate_backoff(None, 1), Duration::from_secs(2));
        assert_eq!(calculate_backoff(None, 2), Duration::from_secs(4));
        assert_eq!(calculate_backoff(None, 3), Duration::from_secs(8));
        assert_eq!(calculate_backoff(None, 5), Duration::from_secs(32));
        // 上限 60 秒
        assert_eq!(calculate_backoff(None, 6), Duration::from_secs(60));
        assert_eq!(calculate_backoff(None, 100), Duration::from_secs(60));
    }

    #[test]
    fn calculate_backoff_with_retry_after() {
        let delay = Duration::from_secs(42);
        // Retry-After 优先于指数退避
        assert_eq!(calculate_backoff(Some(delay), 0), Duration::from_secs(42));
        assert_eq!(calculate_backoff(Some(delay), 5), Duration::from_secs(42));
    }

    #[test]
    fn calculate_backoff_does_not_overflow() {
        // attempt 极大时也应安全返回 60，不会 panic
        let result = calculate_backoff(None, u32::MAX);
        assert_eq!(result, Duration::from_secs(60));
    }

    #[test]
    fn is_retryable_status_basic() {
        // 429 始终可重试
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS, &[]));
        // 5xx 可重试
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR, &[]));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY, &[]));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE, &[]));
        // 2xx / 3xx / 4xx（除 429）不可重试
        assert!(!is_retryable_status(StatusCode::OK, &[]));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST, &[]));
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED, &[]));
        assert!(!is_retryable_status(StatusCode::FORBIDDEN, &[]));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND, &[]));
    }

    #[test]
    fn is_retryable_status_with_extra_codes() {
        // 非 429/5xx 的额外码，如 418（I’m a teapot）
        let teapot = StatusCode::from_u16(418).unwrap();
        assert!(!is_retryable_status(teapot, &[]));
        assert!(is_retryable_status(teapot, &[418]));
        // 529 本身属于 5xx，因此无论 extra_codes 是什么都可重试
        assert!(is_retryable_status(StatusCode::from_u16(529).unwrap(), &[]));
        assert!(is_retryable_status(
            StatusCode::from_u16(529).unwrap(),
            &[529]
        ));
    }

    #[tokio::test]
    async fn parse_retry_after_present() {
        let client = reqwest::Client::new();
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = server.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nRetry-After: 15\r\nContent-Length: 0\r\n\r\n")
                .await;
        });

        let resp = client.get(format!("http://{addr}")).send().await.unwrap();
        assert_eq!(parse_retry_after(&resp), Some(Duration::from_secs(15)));
    }

    #[tokio::test]
    async fn parse_retry_after_missing() {
        let client = reqwest::Client::new();
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = server.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await;
        });

        let resp = client.get(format!("http://{addr}")).send().await.unwrap();
        assert_eq!(parse_retry_after(&resp), None);
    }

    #[tokio::test]
    async fn parse_retry_after_invalid() {
        let client = reqwest::Client::new();
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut stream, _) = server.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nRetry-After: not-a-number\r\nContent-Length: 0\r\n\r\n",
                )
                .await;
        });

        let resp = client.get(format!("http://{addr}")).send().await.unwrap();
        assert_eq!(parse_retry_after(&resp), None);
    }
}
