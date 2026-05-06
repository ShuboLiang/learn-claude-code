//! 会话级 SSE 广播器
//!
//! 每个活跃会话拥有一个 tokio::sync::broadcast 频道，
//! agent 事件写入频道，所有订阅的客户端（包括刷新后的浏览器）实时接收。

use std::sync::Arc;

use dashmap::DashMap;
use rust_agent_core::agent::AgentEvent;
use tokio::sync::broadcast;

/// 会话级事件广播器
#[derive(Clone)]
pub struct SessionBroadcaster {
    channels: Arc<DashMap<String, broadcast::Sender<AgentEvent>>>,
    capacity: usize,
}

impl SessionBroadcaster {
    /// 创建广播器，指定每个频道的缓冲区容量
    pub fn new(capacity: usize) -> Self {
        Self {
            channels: Arc::new(DashMap::new()),
            capacity,
        }
    }

    /// 获取或创建会话的广播发送端
    pub fn get_or_create(&self, session_id: &str) -> broadcast::Sender<AgentEvent> {
        if let Some(entry) = self.channels.get(session_id) {
            entry.clone()
        } else {
            let (tx, _rx) = broadcast::channel(self.capacity);
            self.channels.insert(session_id.to_owned(), tx.clone());
            tx
        }
    }

    /// 订阅会话的广播（用于 SSE 客户端）
    pub fn subscribe(&self, session_id: &str) -> Option<broadcast::Receiver<AgentEvent>> {
        self.channels.get(session_id).map(|entry| entry.subscribe())
    }

    /// 发送事件到指定会话的所有订阅者
    pub fn send(&self, session_id: &str, event: AgentEvent) {
        if let Some(entry) = self.channels.get(session_id) {
            // 忽略发送失败（如所有订阅者已断开）
            let _ = entry.send(event);
        }
    }

    /// 清理会话的广播频道（流结束后调用）
    pub fn remove(&self, session_id: &str) {
        self.channels.remove(session_id);
    }

    /// 检查会话是否有活跃广播频道
    pub fn has_session(&self, session_id: &str) -> bool {
        self.channels.contains_key(session_id)
    }
}
