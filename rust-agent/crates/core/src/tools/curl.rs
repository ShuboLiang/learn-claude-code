//! HTTP 请求工具封装
//!
//! 提供结构化 HTTP 请求能力，支持黑名单安全策略。

use std::time::Duration;

use anyhow::{anyhow, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentResult;
use crate::infra::config::AppConfig;

/// 黑名单条目
enum BlacklistEntry {
    Exact(String),
    Wildcard(String),
    Regex(Regex),
}

impl BlacklistEntry {
    fn matches(&self, host: &str) -> bool {
        match self {
            BlacklistEntry::Exact(s) => host == s,
            BlacklistEntry::Wildcard(pattern) => {
                let parts: Vec<&str> = pattern.split('*').collect();
                if parts.len() == 1 {
                    return host == *pattern;
                }
                let mut host_remaining = host;
                for (i, part) in parts.iter().enumerate() {
                    if part.is_empty() {
                        continue;
                    }
                    if i == 0 {
                        if !host_remaining.starts_with(part) {
                            return false;
                        }
                        host_remaining = &host_remaining[part.len()..];
                    } else if i == parts.len() - 1 {
                        return host_remaining.ends_with(part);
                    } else {
                        match host_remaining.find(part) {
                            Some(pos) => {
                                host_remaining = &host_remaining[pos + part.len()..];
                            }
                            None => return false,
                        }
                    }
                }
                true
            }
            BlacklistEntry::Regex(re) => re.is_match(host),
        }
    }
}

fn parse_blacklist(items: &[String]) -> Vec<BlacklistEntry> {
    items
        .iter()
        .filter_map(|item| {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(pattern) = trimmed.strip_prefix("regex:") {
                match Regex::new(pattern) {
                    Ok(re) => Some(BlacklistEntry::Regex(re)),
                    Err(e) => {
                        eprintln!("[curl] 黑名单正则编译失败 '{}': {e}", trimmed);
                        None
                    }
                }
            } else if trimmed.contains('*') {
                Some(BlacklistEntry::Wildcard(trimmed.to_string()))
            } else {
                Some(BlacklistEntry::Exact(trimmed.to_string()))
            }
        })
        .collect()
}

fn is_blacklisted(host: &str, entries: &[BlacklistEntry]) -> bool {
    entries.iter().any(|e| e.matches(host))
}

/// 完整模式响应结构
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CurlResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Value,
    pub body: String,
    pub elapsed_ms: u64,
}

/// HTTP 请求客户端
pub struct CurlClient {
    http: reqwest::Client,
    blacklist: Vec<BlacklistEntry>,
}

impl Default for CurlClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::new(),
            blacklist: vec![],
        }
    }
}

impl CurlClient {
    /// 从 AppConfig 创建客户端
    pub fn from_config(config: &AppConfig) -> Self {
        let blacklist = config
            .curl_blacklist
            .as_ref()
            .map(|items| parse_blacklist(items))
            .unwrap_or_default();
        Self {
            http: reqwest::Client::new(),
            blacklist,
        }
    }

    /// 校验 URL 是否被黑名单禁止
    fn check_blacklist(&self, url: &str) -> AgentResult<()> {
        let parsed = reqwest::Url::parse(url)
            .map_err(|e| anyhow!("无效的 URL: {e}"))?;
        let host = parsed.host_str()
            .ok_or_else(|| anyhow!("URL 缺少主机名"))?;
        if is_blacklisted(host, &self.blacklist) {
            bail!("URL 被安全策略禁止: {host}");
        }
        Ok(())
    }

    /// 执行 HTTP 请求
    ///
    /// * `url` — 请求地址
    /// * `method` — HTTP 方法（GET/POST/PUT/DELETE/PATCH）
    /// * `headers` — 可选的请求头（JSON 对象）
    /// * `body` — 原始 body 文本（与 json 互斥）
    /// * `json` — JSON body（自动设置 Content-Type，与 body 互斥）
    /// * `timeout` — 超时秒数
    /// * `detailed` — true 返回完整 CurlResponse，false 仅返回 body 文本
    pub async fn execute(
        &self,
        url: &str,
        method: &str,
        headers: Option<Value>,
        body: Option<&str>,
        json: Option<Value>,
        timeout: u64,
        detailed: bool,
    ) -> AgentResult<String> {
        self.check_blacklist(url)?;

        let method = match method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            other => bail!("不支持的 HTTP 方法: {other}"),
        };

        let mut request = self
            .http
            .request(method, url)
            .timeout(Duration::from_secs(timeout));

        // 添加自定义 headers
        if let Some(headers_obj) = headers {
            if let Some(obj) = headers_obj.as_object() {
                for (key, value) in obj {
                    let val = value.as_str().map(|s| s.to_string()).unwrap_or_else(|| value.to_string());
                    request = request.header(key, val);
                }
            }
        }

        // body 处理（json 优先）
        if let Some(json_value) = json {
            request = request.json(&json_value);
        } else if let Some(body_text) = body {
            request = request.body(body_text.to_string());
        }

        let start = std::time::Instant::now();
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                anyhow!("请求超时（{timeout} 秒）")
            } else if e.is_connect() {
                anyhow!("无法连接到服务器: {e}")
            } else {
                anyhow!("请求失败: {e}")
            }
        })?;
        let elapsed = start.elapsed().as_millis() as u64;

        let status = response.status();
        let status_text = status.canonical_reason().unwrap_or("Unknown").to_string();

        // 收集 headers
        let mut headers_map = serde_json::Map::new();
        for (key, value) in response.headers() {
            let val = value.to_str().unwrap_or("[binary]");
            headers_map.insert(key.to_string(), Value::String(val.to_string()));
        }

        // 读取 body（10MB 限制）
        const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;
        let body_bytes = response.bytes().await.map_err(|e| anyhow!("读取响应失败: {e}"))?;
        let (body_text, truncated) = if body_bytes.len() > MAX_BODY_SIZE {
            let truncated = &body_bytes[..MAX_BODY_SIZE];
            let text = String::from_utf8_lossy(truncated).to_string();
            (text, true)
        } else {
            let text = decode_body(&body_bytes);
            (text, false)
        };

        if detailed {
            let result = CurlResponse {
                status: status.as_u16(),
                status_text,
                headers: Value::Object(headers_map),
                body: if truncated {
                    format!("{body_text}\n[响应体超过 10MB，已截断]")
                } else {
                    body_text
                },
                elapsed_ms: elapsed,
            };
            Ok(serde_json::to_string_pretty(&result)?)
        } else {
            let output = if !status.is_success() {
                format!("[HTTP {}] {body_text}", status.as_u16())
            } else if truncated {
                format!("{body_text}\n[响应体超过 10MB，已截断]")
            } else {
                body_text
            };
            Ok(output)
        }
    }
}

/// 解码响应体：优先 UTF-8，失败时尝试 encoding_rs 检测
fn decode_body(bytes: &[u8]) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            let (cow, _, had_errors) = encoding_rs::UTF_8.decode(bytes);
            if !had_errors {
                return cow.to_string();
            }
            // 尝试 GBK
            let (cow, _, _) = encoding_rs::GBK.decode(bytes);
            cow.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_blacklist_match() {
        let entries = parse_blacklist(&["localhost".to_string()]);
        assert!(is_blacklisted("localhost", &entries));
        assert!(!is_blacklisted("example.com", &entries));
    }

    #[test]
    fn wildcard_blacklist_match() {
        let entries = parse_blacklist(&["192.168.*".to_string()]);
        assert!(is_blacklisted("192.168.1.1", &entries));
        assert!(!is_blacklisted("193.168.1.1", &entries));
    }

    #[test]
    fn regex_blacklist_match() {
        let entries = parse_blacklist(&["regex:^10\\.".to_string()]);
        assert!(is_blacklisted("10.0.0.1", &entries));
        assert!(!is_blacklisted("11.0.0.1", &entries));
    }

    #[test]
    fn check_blacklist_blocks() {
        let client = CurlClient {
            http: reqwest::Client::new(),
            blacklist: parse_blacklist(&["localhost".to_string()]),
        };
        assert!(client.check_blacklist("http://localhost:8080/test").is_err());
        assert!(client.check_blacklist("https://example.com/test").is_ok());
    }
}
