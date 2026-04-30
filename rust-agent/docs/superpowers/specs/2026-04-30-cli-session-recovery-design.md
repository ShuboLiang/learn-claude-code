# CLI Session Recovery Design

## 1. Objective

Enable the Node.js CLI (`cli/`) to list and recover historical server-side sessions at runtime via slash commands. When a user types `/sessions`, the CLI displays a numbered list of persisted sessions. Typing `/load <index>` or `/load <uuid>` switches the current UI context to that session's conversation history, allowing the user to continue where they left off.

## 2. Background

- The server (`crates/server/src/session.rs`) already persists every session to `~/.rust-agent/sessions/{id}.json` via `SessionStore`.
- The server reloads these sessions on startup, so historical sessions survive server restarts.
- The CLI currently calls `POST /sessions` on every launch and keeps conversation state only in-memory (`App` component's `messages` state).
- There is no server endpoint to list sessions or retrieve a session's full message history for a frontend.

## 3. Constraints

- **Zero SQL**: reuse the existing JSON-file persistence.
- **Minimal UI complexity**: no overlay panels or modal modes; use the existing message stream for output.
- **Frontend-friendly format**: the CLI should not need to deeply understand LLM provider content-block internals.

## 4. API Design

### 4.1 `GET /sessions`

Returns session summaries ordered by `last_active` descending.

**Response:**
```json
{
  "sessions": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "created_at": "2026-04-30T10:00:00Z",
      "last_active": "2026-04-30T11:30:00Z",
      "message_count": 12,
      "preview": "安装英语学习技能"
    }
  ]
}
```

- `preview` is derived from the first user message's text content, truncated to 30 characters.
- Empty or whitespace-only previews fall back to `"(无预览)"`.

### 4.2 `GET /sessions/:id/messages`

Returns the full conversation history of a session as an array of `ApiMessage` objects.

**Response (200):**
```json
{
  "messages": [
    { "role": "user", "content": "安装英语学习技能" },
    { "role": "assistant", "content": "好的，我来帮你查找英语学习技能。" },
    { "role": "assistant", "content": [{"type":"tool_use","name":"search_skillhub","input":{"queries":["英语学习"]},"id":"tool_123"}] }
  ]
}
```

**Response (404):**
```json
{
  "error": {
    "code": "session_not_found",
    "message": "会话不存在或已过期"
  }
}
```

- The endpoint returns the raw `ApiMessage` array so that the frontend controls how each message is rendered.
- No pagination: sessions are expected to remain small enough for a single JSON response.

## 5. CLI Frontend Changes

### 5.1 `api.ts`

Add two functions:

```ts
export interface SessionSummary {
  id: string;
  created_at: string;
  last_active: string;
  message_count: number;
  preview: string;
}

export async function fetchSessions(): Promise<SessionSummary[]> { ... }
export async function fetchSessionMessages(id: string): Promise<Array<{role: string; content: any}>> { ... }
```

### 5.2 `app.tsx`

Introduce a ref to cache the most recently fetched session list:

```ts
const sessionListRef = useRef<SessionSummary[]>([]);
```

#### `/sessions` command

1. Call `fetchSessions()`.
2. Store result in `sessionListRef.current`.
3. Format a numbered list and append it as a single `system` message to `messages`:

```
═══ 历史会话 ═══
[1] 04-30 11:30  (12 条)  安装英语学习技能
[2] 04-30 10:00  (5 条)   帮我写个 Rust 函数
════════════════
使用 /load <序号> 或 /load <uuid> 恢复
```

#### `/load <arg>` command

1. If `isLoading` is true, append a `system` message: `"当前有进行中的对话，请等待结束后再加载。"` and return.
2. Resolve the target session ID:
   - If `arg` is a positive integer, look it up in `sessionListRef.current` (1-based indexing).
   - Otherwise treat `arg` as a raw UUID.
3. If resolution fails, show: `"无效序号或 UUID，先用 /sessions 查看列表。"`
4. Call `fetchSessionMessages(id)`.
5. Update `config.sessionId` to the loaded ID.
6. Transform the returned `ApiMessage` array into the frontend `Message[]` format:
   - `role === "user"`, `content` is string → `{role: "user", content}`.
   - `role === "assistant"`, `content` is string → `{role: "assistant", content}`.
   - `role === "assistant"`, `content` is array → iterate blocks:
     - `type === "text"` → `{role: "assistant", content: text}`.
     - `type === "tool_use"` → `{role: "tool_call", content: JSON.stringify({name, input})}`.
   - `role === "user"`, `content` is array → iterate blocks:
     - `type === "tool_result"` → `{role: "tool_result", content: content}`.
     - Other blocks → collect text and emit `{role: "user", content: joinedText}`.
   - Unknown `role` → skip with a console warning.
7. Replace `messages` state with the transformed array.
8. Append a `system` message: `"═══ 已恢复会话 {preview} ═══"`.

### 5.3 `handleClear` behavior

`handleClear` already calls `clearSession()`, which uses the current `config.sessionId`. After a `/load`, it will correctly clear the loaded session.

## 6. Data Flow

```
User: /sessions
  → GET /sessions
    → sessionListRef populated
      → system message rendered in Chat

User: /load 1
  → Resolve index 1 → uuid
    → GET /sessions/:id/messages
      → ApiMessage[] returned
        → Frontend transform → Message[]
          → setMessages(newMessages)
            → config.sessionId = uuid
              → UI renders historical conversation
                → User sends new message
                  → POST /sessions/:uuid/messages (continues existing session)
```

## 7. Error Handling

| Scenario | Behavior |
|----------|----------|
| `GET /sessions` fails | Render `system` message: `"获取历史会话失败: {error}"` |
| `GET /messages` returns 404 | Render `system` message: `"会话不存在或已过期"` |
| `/load` while `isLoading` | Reject and render `system` message |
| `/load` with invalid index | Render `system` message: `"无效序号，先用 /sessions 查看列表"` |
| `/load` with invalid UUID | Render `system` message: `"未找到该会话"` |
| History contains unknown block type | Skip silently; do not crash the UI |

## 8. Testing Strategy

### Backend
- Unit test `SessionStore::list()` returns summaries sorted by `last_active` descending.
- Unit test `GET /sessions/:id/messages` returns exact `ApiMessage` array for existing session.
- Unit test 404 for non-existent session ID.

### Frontend
- Verify `/sessions` command formats and renders the numbered list correctly.
- Verify `/load 1` resolves index to UUID, fetches messages, replaces `messages` state, and updates `sessionId`.
- Verify `/load` while `isLoading` is rejected.
- Verify that after `/load`, subsequent `sendMessage` uses the new `sessionId`.

## 9. Files to Modify

- `crates/server/src/session.rs` — add `list()` and `get_messages()` on `SessionStore`.
- `crates/server/src/routes.rs` — add `GET /sessions` and `GET /sessions/:id/messages` handlers.
- `cli/src/api.ts` — add `fetchSessions()` and `fetchSessionMessages()`.
- `cli/src/app.tsx` — add `/sessions` and `/load` command parsing, message transformation, and state updates.
