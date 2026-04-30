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

#[derive(Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub message_count: usize,
    pub preview: String,
}

fn extract_preview(messages: &[rust_agent_core::api::types::ApiMessage]) -> String {
    for msg in messages {
        if msg.role != "user" {
            continue;
        }
        let text = match &msg.content {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) => {
                arr.iter()
                    .filter_map(|b| {
                        if b.get("type")?.as_str()? == "text" {
                            b.get("text")?.as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            }
            _ => continue,
        };
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return trimmed.chars().take(30).collect();
        }
    }
    "(无预览)".to_owned()
}

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

    pub async fn list(&self) -> Vec<SessionSummary> {
        let mut summaries = Vec::with_capacity(self.sessions.len());
        for entry in self.sessions.iter() {
            let session = entry.read().await;
            summaries.push(SessionSummary {
                id: session.id.clone(),
                created_at: session.created_at,
                last_active: session.last_active,
                message_count: session.context.len(),
                preview: extract_preview(session.context.messages()),
            });
        }
        summaries.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        summaries
    }

    pub async fn get_messages(
        &self,
        id: &str,
    ) -> Option<Vec<rust_agent_core::api::types::ApiMessage>> {
        let entry = self.sessions.get(id)?;
        let session = entry.read().await;
        Some(session.context.messages().to_vec())
    }
}

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

    #[tokio::test]
    async fn session_store_persists_and_reloads() {
        let tmp = std::env::temp_dir().join(format!("rust-agent-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let store = SessionStore::new(tmp.clone()).await;
        let session_arc = store.create().await;
        let id = session_arc.read().await.id.clone();

        // File must exist on disk after persist
        store.persist(&id).await;
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
        store.persist(&id).await;

        assert!(store.remove(&id).await);
        assert!(store.get(&id).is_none());
        assert!(!tmp.join(format!("{id}.json")).exists());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn session_store_list_returns_sorted_summaries() {
        let tmp = std::env::temp_dir().join(format!("rust-agent-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let store = SessionStore::new(tmp.clone()).await;
        let s1 = store.create().await;
        let s2 = store.create().await;

        {
            let mut s1_locked = s1.write().await;
            s1_locked.context.push_user_text("first session");
            s1_locked.last_active = Utc::now() - chrono::Duration::hours(1);
        }
        {
            let mut s2_locked = s2.write().await;
            s2_locked.context.push_user_text("second session");
        }

        store.persist(&s1.read().await.id).await;
        store.persist(&s2.read().await.id).await;

        let list = store.list().await;
        assert_eq!(list.len(), 2);
        assert!(list[0].last_active >= list[1].last_active);
        assert_eq!(list[0].preview, "second session");
        assert_eq!(list[1].preview, "first session");

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn session_store_get_messages_returns_history() {
        let tmp = std::env::temp_dir().join(format!("rust-agent-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&tmp).await.unwrap();

        let store = SessionStore::new(tmp.clone()).await;
        let session = store.create().await;
        let id = session.read().await.id.clone();
        {
            let mut s = session.write().await;
            s.context.push_user_text("hello");
        }
        store.persist(&id).await;

        let messages = store.get_messages(&id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");

        let missing = store.get_messages("nonexistent").await;
        assert!(missing.is_none());

        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }
}
