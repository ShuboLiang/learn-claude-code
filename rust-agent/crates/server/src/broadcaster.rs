//! 会话级 SSE 广播器
//!
//! 每个活跃会话拥有一个 tokio::sync::broadcast 频道，
//! agent 事件写入频道，所有订阅的客户端（包括刷新后的浏览器）实时接收。
//!
//! 新增：事件缓存机制。刷新浏览器后新 subscriber 会收到历史事件重放，
//! 避免丢失刷新前已累积的流式文本。

use std::sync::Arc;

use dashmap::DashMap;
use rust_agent_core::agent::AgentEvent;
use tokio::sync::broadcast;

/// 单个会话的广播状态
struct BroadcastState {
    tx: broadcast::Sender<AgentEvent>,
    /// 缓存已发送的事件（供新 subscriber 重放）
    history: Vec<AgentEvent>,
}

/// 会话级事件广播器
#[derive(Clone)]
pub struct SessionBroadcaster {
    channels: Arc<DashMap<String, BroadcastState>>,
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
            entry.tx.clone()
        } else {
            let (tx, _rx) = broadcast::channel(self.capacity);
            let state = BroadcastState {
                tx: tx.clone(),
                history: Vec::new(),
            };
            self.channels.insert(session_id.to_owned(), state);
            tx
        }
    }

    /// 订阅会话的广播（用于 SSE 客户端）
    /// 返回接收端 + 历史事件缓存，供新 subscriber 先重放历史再接收实时事件
    pub fn subscribe(&self, session_id: &str) -> Option<(broadcast::Receiver<AgentEvent>, Vec<AgentEvent>)> {
        self.channels.get(session_id).map(|entry| {
            let rx = entry.tx.subscribe();
            let history = entry.history.clone();
            (rx, history)
        })
    }

    /// 发送事件到指定会话的所有订阅者，并缓存到历史
    pub fn send(&self, session_id: &str, event: AgentEvent) {
        if let Some(mut entry) = self.channels.get_mut(session_id) {
            // 缓存事件（排除 done/error 等终止事件，避免无意义累积）
            match &event {
                AgentEvent::Done { .. } => { /* 不缓存 done */ }
                _ => entry.history.push(event.clone()),
            }
            // 忽略发送失败（如所有订阅者已断开）
            let _ = entry.tx.send(event);
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

    /// 获取指定会话广播频道的当前接收者数量
    pub fn receiver_count(&self, session_id: &str) -> usize {
        self.channels.get(session_id).map(|e| e.tx.receiver_count()).unwrap_or(0)
    }
}
