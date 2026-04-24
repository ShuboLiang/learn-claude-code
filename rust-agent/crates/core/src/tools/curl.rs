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
