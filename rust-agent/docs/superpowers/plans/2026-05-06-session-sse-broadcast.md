# 会话级 SSE 广播 — 刷新后继续实时流

## 目标

将 SSE 流从"单次 HTTP 连接绑定"升级为"会话级广播"，浏览器刷新后可重新订阅同一会话的实时流，后台 agent 不中断。

## 架构

- 每个会话拥有一个 `tokio::sync::broadcast` 频道
- `send_message` 只启动 agent（fire-and-forget），agent 事件写入广播频道
- `GET /sessions/{id}/stream` 订阅广播频道，支持多客户端同时接收
- 前端：发送消息后打开 SSE 订阅；页面加载时自动重连活跃流

## 文件改动

| 文件 | 改动 |
|------|------|
| `crates/server/src/broadcaster.rs` | 新建：会话级广播器管理 |
| `crates/server/src/routes.rs` | `send_message` 改为 fire-and-forget；新增 `stream` 端点 |
| `crates/server/src/main.rs` | 注册 broadcaster 模块 |
| `web/src/store/chat.ts` | 发送消息后订阅 SSE；加载时重连 |
| `web/src/api/sse.ts` | 新增 `subscribeSessionStream` |

## 任务

### Task 1: 后端广播器模块

创建 `crates/server/src/broadcaster.rs`：

```rust
use std::sync::Arc;
use dashmap::DashMap;
use rust_agent_core::agent::AgentEvent;
use tokio::sync::broadcast;

pub struct SessionBroadcaster {
    channels: Arc<DashMap<String, broadcast::Sender<AgentEvent>>>,
    capacity: usize,
}

impl SessionBroadcaster {
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

    /// 发送事件到指定会话
    pub fn send(&self, session_id: &str, event: AgentEvent) {
        if let Some(entry) = self.channels.get(session_id) {
            let _ = entry.send(event); // 忽略 lagging subscriber 错误
        }
    }

    /// 清理会话的广播频道
    pub fn remove(&self, session_id: &str) {
        self.channels.remove(session_id);
    }
}
```

### Task 2: 修改 send_message（fire-and-forget）

`routes.rs`：

1. `send_message` 改为返回 `Json<serde_json::Value>`：`{ "status": "started" }`
2. 启动 agent 的 `tokio::spawn` 块中，不再创建 `mpsc::channel`
3. 通过 `state.broadcaster.get_or_create(&session_id)` 获取发送端
4. agent 事件通过 `broadcaster.send()` 广播
5. agent 结束后发送 `Done` 事件，然后 `broadcaster.remove(&session_id)`

### Task 3: 新增 SSE 订阅端点

`routes.rs`：

```rust
async fn stream_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let rx = match state.broadcaster.subscribe(&id) {
        Some(rx) => rx,
        None => return (StatusCode::NOT_FOUND, "会话无活跃流").into_response(),
    };
    
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|result| result.ok()) // 忽略 lagging error
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);
    
    axum::response::sse::Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}
```

路由注册：`.route("/sessions/{id}/stream", get(stream_session))`

### Task 4: AppState 添加 broadcaster

```rust
pub struct AppState {
    pub store: SessionStore,
    pub agent: Arc<AgentApp>,
    pub bot_registry: Arc<BotRegistry>,
    pub config: Arc<AppConfig>,
    pub providers: Arc<DashMap<String, LlmProvider>>,
    pub broadcaster: Arc<SessionBroadcaster>,
}
```

### Task 5: 前端 SSE 订阅

`web/src/api/sse.ts` 新增：

```typescript
export function subscribeSessionStream(
  sessionId: string,
  signal: AbortController,
): AsyncGenerator<SSEEvent, void, undefined> {
  return parseSSEStream(
    fetch(`/sessions/${encodeURIComponent(sessionId)}/stream`, { signal: signal.signal }),
    signal.signal,
  )
}
```

### Task 6: 前端 chat.ts 改造

`sendMessage`：
1. POST 发送消息
2. 如果 POST 返回成功，立即打开 SSE 订阅到 `/sessions/{id}/stream`
3. SSE 事件循环和原来一样处理

页面加载时（`selectSession` 或初始化）：
1. 如果 `streamingBySession[sid]` 存在且 `active: true`，自动打开 SSE 订阅
2. 这样刷新后切回会话，会自动恢复实时流

### Task 7: 前端断开重连

SSE 连接意外断开时（非用户主动 abort）：
1. 等待 2 秒后自动重试订阅
2. 最多重试 3 次
3. 如果会话流已结束（`Done` 已收到），不再重连
