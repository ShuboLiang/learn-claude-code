# CLI Session Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `/sessions` and `/load` slash commands to the CLI so users can list and recover historical server-side sessions at runtime.

**Architecture:** Extend `SessionStore` with `list()` and `get_messages()`, expose two new HTTP endpoints, and wire the CLI to fetch summaries, render a numbered list, and load a selected session's history into the React state.

**Tech Stack:** Rust (axum, tokio, serde), TypeScript (React, Ink), Node.js built-in test runner.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/server/src/session.rs` | Add `list()`, `get_messages()`, `extract_preview()`, and unit tests. |
| `crates/server/src/routes.rs` | Add `GET /sessions` and `GET /sessions/:id/messages` handlers. |
| `cli/src/api.ts` | Add `SessionSummary`, `fetchSessions()`, `fetchSessionMessages()`, `setSessionId()`. |
| `cli/src/session-utils.ts` | New pure function `transformMessages()` to convert `ApiMessage[]` → frontend `Message[]`. |
| `cli/tests/session-utils.test.ts` | Unit tests for `transformMessages()` using Node.js built-in test runner. |
| `cli/src/app.tsx` | Add `/sessions` and `/load` command parsing, `sessionListRef`, and state replacement logic. |

---

### Task 1: Extend SessionStore with list, get_messages, and preview extraction

**Files:**
- Modify: `crates/server/src/session.rs`
- Test: inline `#[cfg(test)]` module at bottom of `crates/server/src/session.rs`

- [ ] **Step 1: Add `SessionSummary` struct and `extract_preview` helper**

Insert near the top of `crates/server/src/session.rs`, after the existing `SessionRecord` impl:

```rust
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
```

- [ ] **Step 2: Add `list()` and `get_messages()` to `SessionStore`**

Inside the `impl SessionStore` block, add:

```rust
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
```

- [ ] **Step 3: Add unit tests for `list()` and `get_messages()`**

Append to the `#[cfg(test)]` module at the bottom of `crates/server/src/session.rs`:

```rust
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
```

- [ ] **Step 4: Run backend tests**

Command:
```bash
cd crates/server && cargo test
```

Expected: all tests pass, including the two new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/session.rs
git commit -m "feat(server): add SessionStore::list and get_messages with tests"
```

---

### Task 2: Add GET /sessions and GET /sessions/:id/messages routes

**Files:**
- Modify: `crates/server/src/routes.rs`

- [ ] **Step 1: Import `SessionSummary` and add route handlers**

At the top of `crates/server/src/routes.rs`, add:
```rust
use crate::session::SessionSummary;
```

Inside the `routes` function, add the two new routes:
```rust
.route("/sessions", get(list_sessions))
.route("/sessions/{id}/messages", get(get_session_messages))
```

Add the handlers before the existing `health_check`:

```rust
async fn list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state.store.list().await;
    Json(serde_json::json!({ "sessions": sessions })).into_response()
}

async fn get_session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_messages(&id).await {
        Some(messages) => Json(serde_json::json!({ "messages": messages })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": {
                    "code": "session_not_found",
                    "message": "会话不存在或已过期"
                }
            })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 2: Verify compilation**

Command:
```bash
cd crates/server && cargo check
```

Expected: clean compile with zero errors.

- [ ] **Step 3: Commit**

```bash
git add crates/server/src/routes.rs
git commit -m "feat(server): add GET /sessions and GET /sessions/:id/messages routes"
```

---

### Task 3: Extend CLI API client

**Files:**
- Modify: `cli/src/api.ts`

- [ ] **Step 1: Add `SessionSummary` interface and new functions**

Insert after the existing `export interface ServerConfig` block in `cli/src/api.ts`:

```ts
export interface SessionSummary {
  id: string;
  created_at: string;
  last_active: string;
  message_count: number;
  preview: string;
}

export async function fetchSessions(): Promise<SessionSummary[]> {
  const res = await fetch(`${getConfig().baseUrl}/sessions`);
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(
      `获取会话列表失败 (${res.status}): ${data?.error?.message || res.statusText}`
    );
  }
  const data = await res.json();
  return data.sessions || [];
}

export async function fetchSessionMessages(
  id: string
): Promise<Array<{ role: string; content: any }>> {
  const res = await fetch(`${getConfig().baseUrl}/sessions/${id}/messages`);
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(
      `获取会话消息失败 (${res.status}): ${data?.error?.message || res.statusText}`
    );
  }
  const data = await res.json();
  return data.messages || [];
}

export function setSessionId(sessionId: string) {
  if (!config) throw new Error("API 未初始化");
  config.sessionId = sessionId;
}
```

- [ ] **Step 2: Verify TypeScript compilation**

Command:
```bash
cd cli && npx tsc --noEmit
```

Expected: no type errors.

- [ ] **Step 3: Commit**

```bash
git add cli/src/api.ts
git commit -m "feat(cli): add fetchSessions, fetchSessionMessages, and setSessionId"
```

---

### Task 4: Create message transformation utility and unit tests

**Files:**
- Create: `cli/src/session-utils.ts`
- Create: `cli/tests/session-utils.test.ts`

- [ ] **Step 1: Create `cli/src/session-utils.ts`**

```ts
export interface Message {
  role: string;
  content: string;
}

export function transformMessages(
  apiMessages: Array<{ role: string; content: any }>
): Message[] {
  const result: Message[] = [];

  for (const msg of apiMessages) {
    if (msg.role === "user") {
      if (typeof msg.content === "string") {
        result.push({ role: "user", content: msg.content });
      } else if (Array.isArray(msg.content)) {
        const texts: string[] = [];
        for (const block of msg.content) {
          if (block?.type === "tool_result") {
            result.push({
              role: "tool_result",
              content: String(block.content ?? ""),
            });
          } else if (block?.type === "text" && typeof block.text === "string") {
            texts.push(block.text);
          }
        }
        if (texts.length > 0) {
          result.push({ role: "user", content: texts.join("") });
        }
      }
    } else if (msg.role === "assistant") {
      if (typeof msg.content === "string") {
        result.push({ role: "assistant", content: msg.content });
      } else if (Array.isArray(msg.content)) {
        for (const block of msg.content) {
          if (block?.type === "text" && typeof block.text === "string") {
            result.push({ role: "assistant", content: block.text });
          } else if (block?.type === "tool_use") {
            result.push({
              role: "tool_call",
              content: JSON.stringify({
                name: block.name,
                input: block.input,
              }),
            });
          }
        }
      }
    } else {
      console.warn(`Unknown role in history: ${msg.role}`);
    }
  }

  return result;
}
```

- [ ] **Step 2: Create `cli/tests/session-utils.test.ts`**

```ts
import { test } from "node:test";
import assert from "node:assert";
import { transformMessages } from "../src/session-utils.js";

test("user text message", () => {
  const result = transformMessages([{ role: "user", content: "hello" }]);
  assert.deepStrictEqual(result, [{ role: "user", content: "hello" }]);
});

test("assistant text message", () => {
  const result = transformMessages([{ role: "assistant", content: "hi" }]);
  assert.deepStrictEqual(result, [{ role: "assistant", content: "hi" }]);
});

test("assistant with tool_use", () => {
  const result = transformMessages([
    {
      role: "assistant",
      content: [
        { type: "text", text: "Let me search." },
        {
          type: "tool_use",
          name: "search",
          input: { q: "test" },
          id: "t1",
        },
      ],
    },
  ]);
  assert.deepStrictEqual(result, [
    { role: "assistant", content: "Let me search." },
    {
      role: "tool_call",
      content: JSON.stringify({ name: "search", input: { q: "test" } }),
    },
  ]);
});

test("user with tool_result", () => {
  const result = transformMessages([
    {
      role: "user",
      content: [
        { type: "text", text: "Result:" },
        { type: "tool_result", tool_use_id: "t1", content: "found" },
      ],
    },
  ]);
  assert.deepStrictEqual(result, [
    { role: "user", content: "Result:" },
    { role: "tool_result", content: "found" },
  ]);
});

test("skips unknown role", () => {
  const result = transformMessages([{ role: "system", content: "warn" }]);
  assert.deepStrictEqual(result, []);
});
```

- [ ] **Step 3: Run tests**

Command:
```bash
cd cli && npx tsx --test tests/session-utils.test.ts
```

Expected output:
```
✔ user text message (0.1234ms)
✔ assistant text message (0.0567ms)
✔ assistant with tool_use (0.089ms)
✔ user with tool_result (0.0765ms)
✔ skips unknown role (0.0456ms)
ℹ tests 5
ℹ pass 5
ℹ fail 0
```

- [ ] **Step 4: Commit**

```bash
git add cli/src/session-utils.ts cli/tests/session-utils.test.ts
git commit -m "feat(cli): add session message transform utility with tests"
```

---

### Task 5: Integrate /sessions and /load commands into App component

**Files:**
- Modify: `cli/src/app.tsx`

- [ ] **Step 1: Add imports and `sessionListRef`**

At the top of `cli/src/app.tsx`, add to the existing `api` import:
```ts
import {
  sendMessage,
  init,
  createSession,
  clearSession,
  fetchBots,
  sendBotTask,
  BotInfo,
  fetchSessions,
  fetchSessionMessages,
  setSessionId,
} from "./api";
```

Add below the existing imports:
```ts
import { transformMessages } from "./session-utils";
```

Inside the `App` component, after `abortControllerRef`, add:
```ts
const sessionListRef = useRef<
  Array<{ id: string; message_count: number; preview: string; last_active: string }>
>([]);
```

- [ ] **Step 2: Add `/sessions` command handling in `handleSubmit`**

Inside `handleSubmit`, after the existing `/bots` command block and before `setError(null)`, add:

```ts
      // /sessions command: list historical sessions
      if (input.trim().toLowerCase() === "/sessions") {
        try {
          const sessions = await fetchSessions();
          sessionListRef.current = sessions;
          if (sessions.length === 0) {
            setMessages((prev) => [
              ...prev,
              { role: "system", content: "═══ 暂无历史会话 ═══" },
            ]);
            return;
          }
          const lines = sessions.map((s, i) => {
            const date = new Date(s.last_active).toLocaleString("zh-CN", {
              month: "2-digit",
              day: "2-digit",
              hour: "2-digit",
              minute: "2-digit",
            });
            return `[${i + 1}] ${date}  (${s.message_count} 条)  ${s.preview}`;
          });
          const text = `═══ 历史会话 ═══\n${lines.join("\n")}\n════════════════\n使用 /load <序号> 或 /load <uuid> 恢复`;
          setMessages((prev) => [...prev, { role: "system", content: text }]);
        } catch (err) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: `获取历史会话失败: ${err}`,
            },
          ]);
        }
        return;
      }
```

- [ ] **Step 3: Add `/load` command handling in `handleSubmit`**

Immediately after the `/sessions` block, add:

```ts
      // /load command: recover a historical session
      const loadMatch = input.trim().match(/^\/load\s+(.+)$/);
      if (loadMatch) {
        if (isLoading) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: "当前有进行中的对话，请等待结束后再加载。",
            },
          ]);
          return;
        }
        const arg = loadMatch[1].trim();
        let targetId: string | undefined;
        const index = parseInt(arg, 10);
        if (
          !isNaN(index) &&
          index > 0 &&
          index <= sessionListRef.current.length
        ) {
          targetId = sessionListRef.current[index - 1].id;
        } else if (!isNaN(index)) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: "无效序号，先用 /sessions 查看列表。",
            },
          ]);
          return;
        } else {
          targetId = arg;
        }

        if (!targetId) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: "无效序号或 UUID，先用 /sessions 查看列表。",
            },
          ]);
          return;
        }

        try {
          const apiMessages = await fetchSessionMessages(targetId);
          const newMessages = transformMessages(apiMessages);
          setSessionId(targetId);
          const preview =
            sessionListRef.current.find((s) => s.id === targetId)?.preview ||
            targetId.slice(0, 8);
          setMessages([
            ...newMessages,
            { role: "system", content: `═══ 已恢复会话 ${preview} ═══` },
          ]);
        } catch (err) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: `加载会话失败: ${err}`,
            },
          ]);
        }
        return;
      }
```

- [ ] **Step 4: Verify TypeScript compilation**

Command:
```bash
cd cli && npx tsc --noEmit
```

Expected: no type errors.

- [ ] **Step 5: Commit**

```bash
git add cli/src/app.tsx
git commit -m "feat(cli): add /sessions and /load commands for session recovery"
```

---

### Task 6: End-to-end smoke test

**Files:**
- No file changes; manual verification.

- [ ] **Step 1: Build the server**

Command:
```bash
cargo build --release -p rust-agent-server
```

Expected: binary built at `target/release/rust-agent-server`.

- [ ] **Step 2: Start the CLI and create a conversation**

Command:
```bash
cd cli && npx tsx src/index.tsx
```

Inside the CLI, send a message such as `hello`，wait for the assistant reply, then type `/exit`.

- [ ] **Step 3: Restart CLI and test recovery commands**

Command:
```bash
cd cli && npx tsx src/index.tsx
```

Inside the CLI:
1. Type `/sessions` → expect a numbered list showing the previous session.
2. Type `/load 1` → expect the chat area to replace with the previous session's messages plus a system line "═══ 已恢复会话 ... ═══".
3. Send a new message → expect the assistant to respond with awareness of the recovered context.

- [ ] **Step 3: Commit if smoke test passes**

```bash
# No new files to add; optionally tag the state
git log --oneline -3
```

---

## Self-Review Checklist

- [x] **Spec coverage:** Every requirement from `2026-04-30-cli-session-recovery-design.md` is mapped to a task.
  - `GET /sessions` → Task 2
  - `GET /sessions/:id/messages` → Task 2
  - `/sessions` command → Task 5 Step 2
  - `/load` command → Task 5 Step 3
  - message transformation → Task 4
  - error handling → embedded in Task 5 code
  - testing → Task 1 (backend), Task 4 (frontend utility), Task 6 (e2e)
- [x] **Placeholder scan:** No TBD, TODO, or vague steps.
- [x] **Type consistency:** `SessionSummary` fields, `setSessionId` signature, and `transformMessages` types match between spec and plan.
