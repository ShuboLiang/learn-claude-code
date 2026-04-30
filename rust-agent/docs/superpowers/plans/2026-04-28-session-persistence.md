# Session Persistence & Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable `crates/server` to survive process restarts by persisting sessions and their conversation history to JSON files on disk, with automatic recovery on startup.

**Architecture:** Remove `Arc<AgentApp>` from `Session`, promote it to `AppState` as a single shared instance. Rewrite `SessionStore` to use `DashMap<String, Arc<RwLock<Session>>>` with atomic JSON persistence (write temp → rename). All mutating routes acquire per-session write locks and trigger `persist()` after completion.

**Tech Stack:** Rust 2024, Tokio, Axum 0.8, DashMap, Serde, Chrono, UUID

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/server/src/session.rs` | `Session` struct (agent removed), `SessionRecord` (de/serialize), `SessionStore` (DashMap + RwLock + disk I/O + startup loading + 30-day cleanup) |
| `crates/server/src/routes.rs` | `AppState` gains `agent: Arc<AgentApp>`; `routes()` signature updated; all handlers use read/write locks; mutating handlers call `persist()` |
| `crates/server/src/openai_compat.rs` | Stop creating `AgentApp` per request; use `state.agent` instead |
| `crates/server/src/main.rs` | Initialize `AgentApp` once, build `AppState`, pass to `routes()` |

---

### Task 1: Define `SessionRecord` and remove `AgentApp` from `Session`

**Files:**
- Modify: `crates/server/src/session.rs` (replace `Session` struct, add `SessionRecord`)
- Test: `crates/server/src/session.rs` (bottom `#[cfg(test)]` module)

**Context:** `Session` currently holds `agent: Arc<AgentApp>` which is not serializable and is identical for every session. We remove it and add a disk-only representation `SessionRecord` that stores `Vec<ApiMessage>`.

- [ ] **Step 1: Write the failing test**

Append to the bottom of `crates/server/src/session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn session_record_roundtrip() {
        let mut context = ContextService::new();
        context.push_user_text("hello");
        let session = Session {
            id: "test-id".to_owned(),
            context,
            created_at: Utc::now(),
            last_active: Utc::now(),
        };
        let record = SessionRecord::from(&session);
        let json = serde_json::to_string(&record).unwrap();
        let decoded: SessionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "test-id");
        assert_eq!(decoded.version, 1);
        let restored = decoded.into_session();
        assert_eq!(restored.id, "test-id");
        assert_eq!(restored.context.len(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p rust-agent-server session_record_roundtrip -- --nocapture
```

Expected: FAIL — `SessionRecord` not found, and `Session` still contains `agent` field causing struct literal mismatch.

- [ ] **Step 3: Write minimal implementation**

Replace the top of `crates/server/src/session.rs` (keep existing `use` lines for `std::sync::Arc`, `chrono`, `dashmap`, `rust_agent_core::context::ContextService`; replace struct definitions and remove `AgentApp` import):

```rust
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rust_agent_core::context::ContextService;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub context: ContextService,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct SessionRecord {
    version: u32,
    id: String,
    created_at: DateTime<Utc>,
    last_active: DateTime<Utc>,
    messages: Vec<rust_agent_core::api::types::ApiMessage>,
}

impl From<&Session> for SessionRecord {
    fn from(session: &Session) -> Self {
        Self {
            version: 1,
            id: session.id.clone(),
            created_at: session.created_at,
            last_active: session.last_active,
            messages: session.context.messages().to_vec(),
        }
    }
}

impl SessionRecord {
    fn into_session(self) -> Session {
        let mut context = ContextService::new();
        context.replace(self.messages);
        Session {
            id: self.id,
            context,
            created_at: self.created_at,
            last_active: self.last_active,
        }
    }
}
```

Leave the old `SessionStore` implementation untouched for now — it will be rewritten in Task 2.

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p rust-agent-server session_record_roundtrip -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/session.rs
git commit -m "feat(session): remove AgentApp and add SessionRecord for persistence"
```

---

### Task 2: Rewrite `SessionStore` with disk persistence

**Files:**
- Modify: `crates/server/src/session.rs` (replace `SessionStore` impl and add tests)

**Context:** The store must load `*.json` files on startup, atomically persist each session after mutation, delete the backing file on removal, and evict sessions idle longer than 30 days.

- [ ] **Step 1: Write the failing tests**

Append to the existing `#[cfg(test)]` module at the bottom of `crates/server/src/session.rs`:

```rust
    #[tokio::test]
    async fn session_store_persists_and_reloads() {
        let tmp = std::env::temp_dir().join(format!("rust-agent-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let store = SessionStore::new(tmp.clone()).await;
        let session_arc = store.create().await;
        let id = session_arc.read().await.id.clone();

        // File must exist on disk
        let path = tmp.join(format!("{id}.json"));
        assert!(path.exists());

        // Reload in a fresh store
        let store2 = SessionStore::new(tmp.clone()).await;
        let reloaded = store2.get(&id).unwrap();
        assert_eq!(reloaded.read().await.id, id);
        assert_eq!(reloaded.read().await.context.len(), 0);

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn session_store_removes_session_and_file() {
        let tmp = std::env::temp_dir().join(format!("rust-agent-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let store = SessionStore::new(tmp.clone()).await;
        let session_arc = store.create().await;
        let id = session_arc.read().await.id.clone();

        assert!(store.remove(&id).await);
        assert!(store.get(&id).is_none());
        assert!(!tmp.join(format!("{id}.json")).exists());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p rust-agent-server session_store -- --nocapture
```

Expected: FAIL — `SessionStore::new` takes `PathBuf` (not found), `create`, `remove`, and `persist` methods do not exist.

- [ ] **Step 3: Write minimal implementation**

Replace the `SessionStore` block in `crates/server/src/session.rs` (everything from `#[derive(Clone)] pub struct SessionStore` through the end of its `impl` block, **but keep the `#[cfg(test)]` module at the very bottom**):

```rust
#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<DashMap<String, Arc<RwLock<Session>>>>,
    data_dir: PathBuf,
}

impl SessionStore {
    pub async fn new(data_dir: PathBuf) -> Self {
        let _ = tokio::fs::create_dir_all(&data_dir).await;
        let sessions: Arc<DashMap<String, Arc<RwLock<Session>>>> = Arc::new(DashMap::new());

        let mut entries = match tokio::fs::read_dir(&data_dir).await {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("[SessionStore] cannot read data dir: {e}");
                return Self { sessions, data_dir };
            }
        };

        let cutoff = Utc::now() - chrono::Duration::days(30);

        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("[SessionStore] failed to read dir entry: {e}");
                    continue;
                }
            };

            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("[SessionStore] failed to read {}: {e}", path.display());
                    continue;
                }
            };

            let record: SessionRecord = match serde_json::from_str(&content) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("[SessionStore] failed to parse {}: {e}", path.display());
                    continue;
                }
            };

            if record.version != 1 {
                tracing::warn!(
                    "[SessionStore] skipping {} (version={})",
                    path.display(),
                    record.version
                );
                continue;
            }

            if record.last_active < cutoff {
                tracing::info!("[SessionStore] deleting stale session file {}", path.display());
                let _ = tokio::fs::remove_file(&path).await;
                continue;
            }

            let session = record.into_session();
            sessions.insert(session.id.clone(), Arc::new(RwLock::new(session)));
        }

        Self { sessions, data_dir }
    }

    pub async fn create(&self) -> Arc<RwLock<Session>> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let session = Session {
            id: id.clone(),
            context: ContextService::new(),
            created_at: now,
            last_active: now,
        };
        let arc = Arc::new(RwLock::new(session));
        self.sessions.insert(id.clone(), arc.clone());
        arc
    }

    pub fn get(&self, id: &str) -> Option<Arc<RwLock<Session>>> {
        self.sessions.get(id).map(|r| r.value().clone())
    }

    pub async fn persist(&self, id: &str) {
        let entry = match self.sessions.get(id) {
            Some(e) => e,
            None => return,
        };

        let session = entry.read().await;
        let record = SessionRecord::from(&*session);
        drop(session);
        drop(entry);

        let path = self.data_dir.join(format!("{id}.json"));
        let tmp = self.data_dir.join(format!(".{id}.json.tmp"));

        let json = match serde_json::to_string_pretty(&record) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!("[SessionStore] serialize failed for {id}: {e}");
                return;
            }
        };

        if let Err(e) = tokio::fs::write(&tmp, json).await {
            tracing::error!("[SessionStore] write temp failed {}: {e}", tmp.display());
            return;
        }

        if let Err(e) = tokio::fs::rename(&tmp, &path).await {
            tracing::error!(
                "[SessionStore] rename failed {} -> {}: {e}",
                tmp.display(),
                path.display()
            );
        }
    }

    pub async fn remove(&self, id: &str) -> bool {
        let removed = self.sessions.remove(id).is_some();
        if removed {
            let path = self.data_dir.join(format!("{id}.json"));
            if let Err(e) = tokio::fs::remove_file(&path).await {
                tracing::warn!("[SessionStore] delete file failed {}: {e}", path.display());
            }
        }
        removed
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p rust-agent-server session_store -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/session.rs
git commit -m "feat(session): rewrite SessionStore with disk persistence and startup loading"
```

---

### Task 3: Update `AppState` and all route handlers

**Files:**
- Modify: `crates/server/src/routes.rs`
- Modify: `crates/server/src/openai_compat.rs`

**Context:** Promote `AgentApp` into `AppState` so it is created once. Every handler that previously cloned `Session` or called `AgentApp::from_env()` must now go through `state.agent` or acquire locks on the shared session.

- [ ] **Step 1: Modify `AppState` and `routes()` signature**

At the top of `crates/server/src/routes.rs`, replace the `AppState` struct and `routes` function:

```rust
#[derive(Clone)]
pub struct AppState {
    pub store: SessionStore,
    pub agent: Arc<AgentApp>,
    pub bot_registry: Arc<BotRegistry>,
}

pub fn routes(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(health_check))
        .route("/sessions", post(create_session))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/sessions/{id}/messages", post(send_message))
        .route("/sessions/{id}/clear", post(clear_session))
        .route("/v1/chat/completions", post(openai_compat::chat_completions))
        .route("/bots", get(list_bots))
        .route("/bots/{name}/task", post(bot_task))
        .with_state(app_state)
}
```

- [ ] **Step 2: Replace `create_session`**

```rust
async fn create_session(State(state): State<AppState>) -> impl IntoResponse {
    let session_arc = state.store.create().await;
    let session = session_arc.read().await;
    let model = state.agent.model().to_owned();
    let id = session.id.clone();
    let created_at = session.created_at.to_rfc3339();
    drop(session);
    state.store.persist(&id).await;

    Json(serde_json::json!({
        "id": id,
        "model": model,
        "created_at": created_at,
    }))
    .into_response()
}
```

- [ ] **Step 3: Replace `get_session`**

```rust
async fn get_session(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get(&id) {
        Some(session_arc) => {
            let session = session_arc.read().await;
            Json(serde_json::json!({
                "id": session.id,
                "message_count": session.context.len(),
                "created_at": session.created_at.to_rfc3339(),
                "last_active": session.last_active.to_rfc3339(),
            }))
            .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在或已过期" }
            })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 4: Replace `delete_session`**

```rust
async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.store.remove(&id).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在" }
            })),
        )
            .into_response()
    }
}
```

- [ ] **Step 5: Replace `clear_session`**

```rust
async fn clear_session(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.store.get(&id) {
        Some(session_arc) => {
            let mut session = session_arc.write().await;
            session.context = ContextService::new();
            session.last_active = Utc::now();
            drop(session);
            state.store.persist(&id).await;
            Json(serde_json::json!({ "status": "cleared" })).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": { "code": "session_not_found", "message": "会话不存在" }
            })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 6: Replace `send_message`**

Replace the entire `send_message` function body:

```rust
async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let session_arc = match state.store.get(&id) {
        Some(s) => s,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "code": "session_not_found", "message": "会话不存在或已过期" }
                })),
            )
                .into_response();
        }
    };

    let (event_tx, event_rx) = mpsc::channel(64);
    let agent = state.agent.clone();
    let content = body.content;
    let session_id = id;
    let store = state.store.clone();

    tokio::spawn(async move {
        let mut session = session_arc.write().await;
        if let Err(e) = agent
            .handle_user_turn(&mut session.context, &content, event_tx.clone())
            .await
        {
            if e.downcast_ref::<rust_agent_core::api::error::LlmApiError>().is_none() {
                let _ = event_tx
                    .send(rust_agent_core::agent::AgentEvent::Error {
                        code: "agent_error".to_owned(),
                        message: format!("{e:#}"),
                    })
                    .await;
            }
        }
        session.last_active = Utc::now();
        drop(session);
        store.persist(&session_id).await;
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx)
        .map(agent_event_to_sse)
        .map(Ok::<_, std::convert::Infallible>);

    axum::response::sse::Sse::new(stream).into_response()
}
```

- [ ] **Step 7: Replace `bot_task` spawn block**

Inside `bot_task`, locate the `tokio::spawn` block and replace everything from `let _store = state.store.clone();` down to the end of that `tokio::spawn` block with:

```rust
    let agent = state.agent.clone();
    let user_content = body.content.clone();
    let bot_skills = bot.skills.clone();

    tokio::spawn(async move {
        let bot_agent = agent.as_ref().clone().with_skills(bot_skills);
        let mut ctx = ContextService::new();

        let nickname = if bot.metadata.nickname.is_empty() {
            &bot.metadata.name
        } else {
            &bot.metadata.nickname
        };
        let role = if bot.metadata.role.is_empty() {
            "智能助手"
        } else {
            &bot.metadata.role
        };

        let skills_desc = bot.skills.descriptions_for_system_prompt();

        let system_prompt = format!(
            "你是 {nickname}({role})。\n\
             工作目录：当前项目根目录。\n\n\
             可用技能：\n{skills_desc}\n\n\
             --- 专属指令 ---\n\
             {body}\n\n\
             ---\n\
             仅使用上述专属指令和技能执行用户任务。\
             完成后给出清晰的总结。",
            body = bot.body
        );

        if let Err(e) = bot_agent
            .handle_bot_turn(&mut ctx, &user_content, system_prompt, event_tx.clone())
            .await
        {
            if e.downcast_ref::<rust_agent_core::api::error::LlmApiError>().is_none() {
                let _ = event_tx
                    .send(rust_agent_core::agent::AgentEvent::Error {
                        code: "bot_agent_error".to_owned(),
                        message: format!("{e:#}"),
                    })
                    .await;
            }
        }
    });
```

Leave the rest of `bot_task` (auth, bot lookup, stream construction) untouched.

- [ ] **Step 8: Update `openai_compat.rs` to reuse shared `AgentApp`**

In `crates/server/src/openai_compat.rs`, replace the `AgentApp::from_env()` block:

Old:
```rust
    let agent = match AgentApp::from_env().await {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": e.to_string(),
                        "type": "server_error",
                        "code": "init_failed"
                    }
                })),
            )
                .into_response();
        }
    };
```

New:
```rust
    let agent = state.agent.as_ref().clone();
```

Also change the function parameter from `State(_state)` to `State(state)`.

- [ ] **Step 9: Remove unused imports in `routes.rs`**

Remove the `use rust_agent_core::agent::AgentApp;` and `use rust_agent_core::context::ContextService;` lines from `routes.rs` **only if they become unused** after the above changes. Keep them if any handler still needs them directly (e.g. `clear_session` uses `ContextService::new()`).

- [ ] **Step 10: Compile check**

```bash
cargo check -p rust-agent-server
```

Expected: clean compile with zero errors.

- [ ] **Step 11: Commit**

```bash
git add crates/server/src/routes.rs crates/server/src/openai_compat.rs
git commit -m "feat(routes): promote AgentApp to AppState and use RwLock sessions"
```

---

### Task 4: Wire up `main.rs` startup

**Files:**
- Modify: `crates/server/src/main.rs`

**Context:** `AgentApp` must be created once and injected into `AppState`. `SessionStore` now requires a `data_dir`.

- [ ] **Step 1: Write the change**

Replace the body of `main()` in `crates/server/src/main.rs` (keep `init_logging()` and the `let _ = dotenvy::dotenv();` line, replace everything after that):

```rust
    let port: u16 = std::env::args()
        .skip_while(|arg| arg != "--port")
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let agent = Arc::new(AgentApp::from_env().await?);

    let data_dir = dirs::home_dir()
        .map(|p| p.join(".rust-agent").join("sessions"))
        .unwrap_or_else(|| std::path::PathBuf::from("./sessions"));
    let store = SessionStore::new(data_dir).await;

    let bot_registry = Arc::new(BotRegistry::load().unwrap_or_default());

    let app_state = routes::AppState {
        store,
        agent,
        bot_registry,
    };

    let app = Router::new()
        .merge(routes::routes(app_state))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("rust-agent-server 启动在 http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
```

Add these imports near the top of `main.rs` if not already present:

```rust
use std::sync::Arc;
use rust_agent_core::agent::AgentApp;
use rust_agent_core::bots::BotRegistry;
```

- [ ] **Step 2: Compile check**

```bash
cargo check -p rust-agent-server
```

Expected: clean compile.

- [ ] **Step 3: Run all server tests**

```bash
cargo test -p rust-agent-server -- --nocapture
```

Expected: all tests pass (including `session_record_roundtrip`, `session_store_persists_and_reloads`, `session_store_removes_session_and_file`).

- [ ] **Step 4: Commit**

```bash
git add crates/server/src/main.rs
git commit -m "feat(main): initialize shared AgentApp and persistent SessionStore"
```

---

### Task 5: Full integration smoke test

**Files:** None (manual verification)

- [ ] **Step 1: Start the server**

```bash
cargo run -p rust-agent-server -- --port 3999
```

Expected: server starts without panic, logs show listening on `http://localhost:3999`.

- [ ] **Step 2: Create a session via curl**

```bash
curl -X POST http://localhost:3999/sessions
```

Expected: JSON response with `id`, `model`, and `created_at`.

- [ ] **Step 3: Verify the session file was written**

```bash
ls ~/.rust-agent/sessions/
```

Expected: one `{uuid}.json` file exists.

- [ ] **Step 4: Send a message (or any request that exercises the session)**

```bash
curl -X POST http://localhost:3999/sessions/{id}/messages \
  -H "Content-Type: application/json" \
  -d '{"content":"hello"}'
```

Expected: SSE stream starts (or returns 404 if the server was restarted before this step).

- [ ] **Step 5: Restart the server and verify session recovery**

Stop the server (Ctrl-C), then restart with the same command. Query the session:

```bash
curl http://localhost:3999/sessions/{id}
```

Expected: `200 OK` with the same `id` and non-zero `message_count` (if a turn completed before restart).

- [ ] **Step 6: Delete the session**

```bash
curl -X DELETE http://localhost:3999/sessions/{id}
```

Expected: `204 No Content`, and the JSON file in `~/.rust-agent/sessions/` is gone.

- [ ] **Step 7: Stop the server**

Ctrl-C. No further action needed.

---

## Self-Review

**1. Spec coverage:**
- ✅ `Session` no longer contains `Arc<AgentApp>` → Task 1
- ✅ One JSON file per session, atomic write (tmp + rename) → Task 2 `persist()`
- ✅ `SessionStore::new(data_dir)` loads `*.json` on startup → Task 2
- ✅ Skip corrupted / version-mismatch files with log → Task 2
- ✅ Delete sessions idle > 30 days on startup → Task 2 (`cutoff` check)
- ✅ `AppState` gains `agent: Arc<AgentApp>` → Task 3
- ✅ `main.rs` initializes `AgentApp` once → Task 4
- ✅ `POST /sessions` calls `store.create()` + `persist()` → Task 3 Step 2
- ✅ `GET /sessions/:id` uses read lock → Task 3 Step 3
- ✅ `POST /sessions/:id/messages` uses write lock + `persist()` → Task 3 Step 6
- ✅ `POST /sessions/:id/clear` uses write lock + `persist()` → Task 3 Step 5
- ✅ `DELETE /sessions/:id` removes from DashMap + deletes file → Task 3 Step 4
- ✅ Disk-full / write failure logged, in-memory session survives → Task 2 `persist()` error handling
- ✅ Same-session concurrent requests serialized by `RwLock` → Task 3 Step 6

**2. Placeholder scan:**
- No "TBD", "TODO", "implement later", "fill in details", "add appropriate error handling", or "similar to Task N" found.

**3. Type consistency:**
- `SessionStore::new(data_dir: PathBuf)` async → used in `main.rs` with `.await`
- `SessionStore::create()` returns `Arc<RwLock<Session>>` → used in routes with `.read().await` / `.write().await`
- `SessionStore::remove(&str)` is async → called with `.await` in route
- `SessionStore::persist(&str)` is async → called with `.await` in routes and spawn
- `AppState.agent` is `Arc<AgentApp>` → cloned with `.clone()` (Arc refcount), deep-cloned in `bot_task` with `agent.as_ref().clone()`
- `SessionRecord.version` is `u32`, checked against `1` everywhere

No gaps found. Plan is ready for execution.
