//! Token 用量追踪器
//!
//! 在会话级别累计追踪 token 用量，支持多线程共享（子代理通过 Arc 共享同一个 tracker）。

use std::collections::HashMap;
use std::ops::AddAssign;
use std::sync::{Arc, Mutex};

use crate::api::types::TokenUsage;

/// 会话级 token 用量追踪器
///
/// 使用 `Arc<Mutex<...>>` 实现线程安全，子代理通过 `AgentApp.clone()` 共享同一个实例。
#[derive(Clone, Debug)]
pub struct TokenTracker {
    inner: Arc<Mutex<TokenTrackerInner>>,
}

#[derive(Debug, Default)]
struct TokenTrackerInner {
    /// 累计总用量
    total: TokenUsage,
    /// 按模型分组的用量
    by_model: HashMap<String, TokenUsage>,
    /// API 调用次数
    api_calls: usize,
}

impl TokenTracker {
    /// 创建新的空追踪器
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TokenTrackerInner::default())),
        }
    }

    /// 记录一次 API 调用的 token 用量
    pub fn record(&self, model: &str, usage: &TokenUsage) {
        let mut inner = self.inner.lock().unwrap();
        inner.total += usage.clone();
        inner
            .by_model
            .entry(model.to_owned())
            .or_default()
            .add_assign(usage.clone());
        inner.api_calls += 1;
    }

    /// 获取当前累计的快照
    pub fn snapshot(&self) -> TokenSnapshot {
        let inner = self.inner.lock().unwrap();
        TokenSnapshot {
            total: inner.total.clone(),
            by_model: inner.by_model.clone(),
            api_calls: inner.api_calls,
        }
    }
}

impl Default for TokenTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Token 用量快照（只读）
#[derive(Clone, Debug)]
pub struct TokenSnapshot {
    /// 累计总用量
    pub total: TokenUsage,
    /// 按模型分组的用量
    pub by_model: HashMap<String, TokenUsage>,
    /// API 调用次数
    pub api_calls: usize,
}
