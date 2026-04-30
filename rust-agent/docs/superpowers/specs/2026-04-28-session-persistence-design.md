# Session Persistence & Recovery Design

## 1. Objective

Enable `crates/server` to survive process restarts: sessions and their conversation history must be persisted to disk and automatically recovered on the next startup. This covers the HTTP API server only; CLI and Desktop are out of scope.

## 2. Background

- `crates/server/src/session.rs` uses an in-memory `DashMap<String, Session>`. All sessions vanish when the server restarts.
- `Session` currently embeds `Arc<AgentApp>`, which is not serializable.
- `crates/core/src/context/history.rs` (`Conversation`) and `crates/core/src/context/mod.rs` (`ContextService`) are pure-memory structs.
- `crates/core/src/infra/storage.rs` already persists large tool results externally; messages themselves remain small enough for JSON files.

## 3. Constraints

- **Storage medium**: one JSON file per session (zero SQL dependency).
- **Deployment**: single server instance (multi-threaded, not multi-process).
- **Write timing**: persist at the end of each user turn (when `TurnEnd` is reached / `store.update` is called).
- **Cleanup**: automatically remove sessions idle longer than 30 days.

## 4. Data Model

### 4.1 On-Disk JSON Format

File path: `~/.rust-agent/sessions/{session_id}.json`

```json
{
  "version": 1,
  "id": "uuid-string",
  "created_at": "2024-01-01T00:00:00Z",
  "last_active": "2024-01-01T00:00:00Z",
  "messages": [
    { "role": "user", "content": "..." },
    { "role": "assistant", "content": [...] }
  ]
}
```

- `version` is mandatory for future migration. The loader refuses files with `version != 1`.
- `messages` is a direct serialization of `Vec<ApiMessage>`. `ApiMessage` already derives `Serialize`/`Deserialize`.
- Large tool outputs are **not** inlined; they reference `~/.rust-agent/tool-results/` via the existing `storage::maybe_persist` mechanism, keeping JSON files small.

### 4.2 Runtime `Session`

`Arc<AgentApp>` is removed from `Session` because it is neither serializable nor per-session unique (each `create_session` currently calls `AgentApp::from_env()` and produces the same configuration).

```rust
pub struct Session {
    pub id: String,
    pub context: ContextService,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}
```

## 5. SessionStore Architecture

### 5.1 Structure

```rust
pub struct SessionStore {
    sessions: Arc<DashMap<String, Arc<tokio::sync::RwLock<Session>>>>,
    data_dir: PathBuf,
}
```

### 5.2 Concurrency Model

- `DashMap` provides concurrent access to the session index.
- Each entry is wrapped in `Arc<tokio::sync::RwLock<Session>>`:
  - **Read lock** (`read().await`) for `GET /sessions/:id`.
  - **Write lock** (`write().await`) for `POST /sessions/:id/messages`, `clear`, and internal mutations.
- **Same-session requests are serialized**: if two clients concurrently send messages to the same session, the second waits for the first agent turn to finish before acquiring the write lock. This prevents race conditions on `ContextService`.

### 5.3 Startup Loading

`SessionStore::new(data_dir)` performs the following on startup:

1. Ensure `data_dir` exists (create if missing).
2. Scan `data_dir` for `*.json` files.
3. For each file:
   - Deserialize into `SessionRecord`.
   - If `version != 1`, skip with a warning log.
   - If deserialization fails (corrupted file), skip with an error log.
   - Convert `SessionRecord` → `Session` and insert into `DashMap`.
4. Delete files whose `last_active` is older than 30 days.

### 5.4 Persistence (Write Path)

A dedicated async method writes a single session atomically:

```rust
pub async fn persist(&self, id: &str) {
    if let Some(entry) = self.sessions.get(id) {
        let session = entry.read().await;
        let record = SessionRecord::from(&*session);
        let path = self.data_dir.join(format!("{id}.json"));
        let tmp = self.data_dir.join(format!(".{id}.json.tmp"));
        // 1. serialize to temp file
        // 2. tokio::fs::rename(tmp, path) for atomicity
    }
}
```

**Call sites**:
- After `POST /sessions/:id/messages` finishes an agent turn (`store.persist(&id).await`).
- After `POST /sessions/:id/clear` (`store.persist(&id).await`).
- After `POST /sessions` creation (`store.persist(&id).await`).

### 5.5 Deletion

`DELETE /sessions/:id` removes the entry from `DashMap` and deletes the JSON file from disk.

## 6. Server Integration

### 6.1 AppState Change

`AgentApp` is promoted to a global shared instance in `AppState` because it is stateless across sessions and expensive to reconstruct per request.

```rust
pub struct AppState {
    pub store: SessionStore,
    pub agent: Arc<AgentApp>,      // NEW: initialized once in main.rs
    pub bot_registry: Arc<BotRegistry>,
}
```

### 6.2 Route Adjustments

| Route | Change |
|-------|--------|
| `POST /sessions` | No longer creates `AgentApp`; calls `store.create()` and immediately `store.persist(&id).await`. |
| `GET /sessions/:id` | Uses `store.get(id)` → `read().await` to return `message_count` and timestamps. |
| `POST /sessions/:id/messages` | Acquires `session_arc.write().await`, runs `agent.handle_user_turn(&mut session.context, ...).await`, updates `last_active`, drops lock, then calls `store.persist(&id).await`. |
| `POST /sessions/:id/clear` | Acquires write lock, replaces `session.context` with `ContextService::new()`, updates `last_active`, drops lock, calls `store.persist(&id).await`. |
| `DELETE /sessions/:id` | Calls `store.remove(&id).await` (removes from DashMap + deletes file). |

### 6.3 `main.rs` Startup

```rust
let agent = Arc::new(AgentApp::from_env().await?);
let store = SessionStore::new(dirs::home_dir().unwrap().join(".rust-agent/sessions"));
let state = AppState { store, agent, bot_registry: Arc::new(BotRegistry::load()?) };
```

## 7. Error Handling & Edge Cases

| Scenario | Behavior |
|----------|----------|
| Corrupted JSON file on startup | Skip file, log error, continue loading others. |
| Version mismatch (`version != 1`) | Treat as corrupted; skip and log. |
| Disk full / write failure | Log error; in-memory session remains valid and continues to serve requests. |
| Concurrent `send_message` on same session | Second request blocks on `write().await` until first turn completes and lock is released. |
| Session not found | Existing 404 behavior preserved. |

## 8. Future Extensibility

- The `SessionStore` internals can later be swapped for a `SessionStorage` trait without changing route code.
- Lazy loading (load from disk on first `get` instead of startup) can be introduced if session volume grows large.
- Multi-instance deployment would require replacing the JSON-file backend with a shared store (Redis, PostgreSQL) behind the same trait.

## 9. Files to Modify

- `crates/server/src/session.rs` — rewrite `Session` and `SessionStore`.
- `crates/server/src/routes.rs` — adjust `AppState` and all route handlers.
- `crates/server/src/main.rs` — initialize `AgentApp` once and inject into `AppState`.
