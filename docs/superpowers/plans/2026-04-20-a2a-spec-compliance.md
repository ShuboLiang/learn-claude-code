# A2A v1.0 Spec Compliance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the existing `rust-agent/crates/a2a` implementation into full compliance with the A2A v1.0 Protocol Specification (HTTP+JSON/REST binding).

**Architecture:** Incremental refactoring of existing handlers, types, and streaming layer. Add missing endpoints as new route + handler pairs. Preserve existing behavior for already-working flows while tightening protocol boundaries.

**Tech Stack:** Rust, Axum 0.8, Tokio, Serde, UUID, DashMap, rust-agent-core

---

## File Map

| File | Responsibility |
|------|---------------|
| `src/types.rs` | Canonical A2A v1.0 data models (AgentCard, Task, Message, Part, TaskStatus, Artifact, StreamResponse, errors) |
| `src/agent_card.rs` | Builds the AgentCard from tool schemas; must include all v1.0 fields |
| `src/handlers.rs` | All HTTP handlers: send_message, send_message_stream, get_task, cancel_task, list_tasks, subscribe_task, get_extended_agent_card |
| `src/routes.rs` | Axum router wiring; registers REST endpoints |
| `src/streaming.rs` | SSE event generation: converts AgentEvent → StreamResponse wrappers |
| `src/state.rs` | In-memory AppState (DashMap tasks + contexts) |
| `src/task_runner.rs` | Synchronous task execution helper |
| `tests/integration.rs` | Integration tests covering all endpoints and error paths |
| `API.md` | Human-facing API documentation |

---

### Task 1: Fix AgentCard and Capabilities model

**Files:**
- Modify: `src/types.rs`
- Modify: `src/agent_card.rs`
- Test: `tests/integration.rs` (agent_card test)

- [ ] **Step 1: Move `extended_agent_card` into `Capabilities`**

Remove `supports_authenticated_extended_card` from `AgentCard` top-level. Add `extended_agent_card: bool` to `Capabilities`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub streaming: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notifications: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_transition_history: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    pub extended_agent_card: bool,
}
```

- [ ] **Step 2: Remove non-standard fields from AgentCard**

Remove `preferred_transport` and `additional_interfaces` from `AgentCard` (these exist only in early Google drafts, not in LF v1.0).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub protocol_version: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    pub capabilities: Capabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_schemes: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<serde_json::Value>>,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
    pub skills: Vec<Skill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<serde_json::Value>>,
}
```

- [ ] **Step 3: Update agent_card.rs builder**

```rust
AgentCard {
    protocol_version: "1.0".to_string(),
    name: "rust-agent".to_string(),
    description: "A Rust-based programming assistant with tool execution capabilities.".to_string(),
    url: base_url.to_string(),
    version: env!("CARGO_PKG_VERSION").to_string(),
    capabilities: Capabilities {
        streaming: true,
        push_notifications: Some(false),
        state_transition_history: Some(false),
        extensions: Some(vec![]),
        extended_agent_card: false,
    },
    security_schemes: None,
    security: None,
    default_input_modes: vec!["text/plain".to_string()],
    default_output_modes: vec!["text/plain".to_string()],
    skills,
    signatures: None,
}
```

- [ ] **Step 4: Fix Capabilities serialization: omit empty extensions**

In `agent_card.rs`, pass `extensions: None` instead of `Some(vec![])` when empty.

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml agent_card`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add rust-agent/crates/a2a/src/types.rs rust-agent/crates/a2a/src/agent_card.rs
git commit -m "fix(a2a): align AgentCard and Capabilities with v1.0 spec"
```

---

### Task 2: Add missing TaskState and fix Task serialization

**Files:**
- Modify: `src/types.rs`
- Modify: `src/handlers.rs`
- Modify: `src/state.rs`
- Modify: `src/streaming.rs`

- [ ] **Step 1: Add `Rejected` to TaskState**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Submitted,
    Working,
    #[serde(rename = "input-required")]
    InputRequired,
    Completed,
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
    Rejected,
}
```

- [ ] **Step 2: Make Task.artifacts and Task.history omit when empty**

The spec requires that when there are no artifacts/history, the fields are **omitted entirely** (not `[]` or `null`). Change the struct to use `Option<Vec<T>>` and ensure we pass `None` instead of `Some(vec![])`.

Current code sets `artifacts: Some(vec![])` and `history: Some(vec![])` everywhere. Replace all such assignments with `None` when the vec is empty.

Search and replace pattern in `handlers.rs`, `task_runner.rs`, and `streaming.rs`:
- `artifacts: Some(vec![])` → `artifacts: None`
- `history: Some(vec![])` → `history: None`

- [ ] **Step 3: Update state.rs to use `Cancelled` spelling everywhere**

Search `Canceled` (American spelling) and replace with `Cancelled` in `state.rs` and any remaining references.

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add rust-agent/crates/a2a/src/types.rs rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/state.rs rust-agent/crates/a2a/src/streaming.rs rust-agent/crates/a2a/src/task_runner.rs
git commit -m "fix(a2a): add Rejected state and omit empty arrays per spec"
```

---

### Task 3: Implement standard error types and A2A-Version header

**Files:**
- Modify: `src/types.rs`
- Modify: `src/handlers.rs`
- Modify: `src/routes.rs`
- Create: `src/errors.rs`

- [ ] **Step 1: Create error types module**

Create `src/errors.rs`:

```rust
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AErrorResponse {
    pub error: A2AError,
}

impl A2AError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn task_not_found(task_id: &str) -> Self {
        Self::new("TaskNotFoundError", format!("Task {} not found", task_id))
    }

    pub fn task_not_cancelable(task_id: &str) -> Self {
        Self::new("TaskNotCancelableError", format!("Task {} is already in a terminal state and cannot be cancelled", task_id))
    }

    pub fn unsupported_operation(msg: impl Into<String>) -> Self {
        Self::new("UnsupportedOperationError", msg)
    }

    pub fn unsupported_part_type(msg: impl Into<String>) -> Self {
        Self::new("ContentTypeNotSupportedError", msg)
    }

    pub fn version_not_supported(version: &str) -> Self {
        Self::new("VersionNotSupportedError", format!("A2A version {} is not supported", version))
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new("InvalidRequestError", msg)
    }
}

impl IntoResponse for A2AErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let status = match self.error.code.as_str() {
            "TaskNotFoundError" => StatusCode::NOT_FOUND,
            "TaskNotCancelableError" => StatusCode::CONFLICT,
            "UnsupportedOperationError" => StatusCode::METHOD_NOT_ALLOWED,
            "ContentTypeNotSupportedError" => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "VersionNotSupportedError" => StatusCode::BAD_REQUEST,
            "InvalidRequestError" => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}
```

- [ ] **Step 2: Register errors module in lib.rs**

Add `pub mod errors;` to `src/lib.rs`.

- [ ] **Step 3: Replace all ad-hoc error JSON in handlers.rs with A2AErrorResponse**

Replace every occurrence of:
```rust
Json(serde_json::json!({ "error": { "code": "...", "message": "..." } }))
```
with:
```rust
Json(A2AErrorResponse { error: A2AError::new("...", "...") })
```

Replace error usages:
- `task_not_found` → `A2AError::task_not_found(&task_id)`
- `unsupported_part_type` → `A2AError::unsupported_part_type(e)`
- `task_id_mismatch` (now removed, but any remaining) → `A2AError::bad_request("...")`

- [ ] **Step 4: Add A2A-Version header validation middleware**

In `src/routes.rs`, add a lightweight layer that checks `A2A-Version` header. If present and not `"1.0"`, return `VersionNotSupportedError`. If absent, treat as acceptable (agent may assume default).

```rust
use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;

async fn a2a_version_check(req: Request, next: Next) -> Result<Response, A2AErrorResponse> {
    if let Some(version) = req.headers().get("A2A-Version") {
        if version.as_bytes() != b"1.0" {
            return Err(A2AErrorResponse {
                error: A2AError::version_not_supported(
                    version.to_str().unwrap_or("unknown")
                ),
            });
        }
    }
    Ok(next.run(req).await)
}
```

Wire it in `routes()`:
```rust
.layer(middleware::from_fn(a2a_version_check))
```

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add rust-agent/crates/a2a/src/errors.rs rust-agent/crates/a2a/src/lib.rs rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/routes.rs
git commit -m "feat(a2a): add standard A2A error types and A2A-Version header validation"
```

---

### Task 4: Fix send_message handler — configuration, contextId, and state lifecycle

**Files:**
- Modify: `src/types.rs`
- Modify: `src/handlers.rs`
- Modify: `src/task_runner.rs`
- Test: `tests/integration.rs`

- [ ] **Step 1: Add SendMessageConfiguration and update SendMessageRequest**

In `src/types.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub message: Message,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<SendMessageConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_output_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_immediately: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_push_notification_config: Option<serde_json::Value>,
}
```

- [ ] **Step 2: Generate contextId for new tasks**

When creating a new task in `send_message`, generate a `context_id` UUID alongside the `task_id`:

```rust
let task_id = uuid::Uuid::new_v4().to_string();
let context_id = uuid::Uuid::new_v4().to_string();
```

Store `context_id` in the `Task` and return it in responses.

- [ ] **Step 3: Infer contextId for multi-turn follow-ups**

In `send_message_followup`, when `message.task_id` is provided but `message.context_id` is absent, look up the existing task and copy its `context_id` into the response message.

Also validate: if both `message.context_id` and `message.task_id` are provided, they must match the stored task's values. If mismatched, return `A2AError::bad_request("contextId does not match task")`.

- [ ] **Step 4: Reject messages to terminal-state tasks**

In `send_message_followup`, after finding the task, check if its current status is a terminal state (`Completed`, `Failed`, `Cancelled`, `Rejected`). If so, return `A2AError::unsupported_operation("Task is already in a terminal state")`.

- [ ] **Step 5: Implement returnImmediately support**

In `send_message`, if `configuration.return_immediately == Some(true)`:
- Create the task with status `Working`
- Spawn the agent execution in a background tokio task (similar to streaming, but without SSE)
- Return the `Task` immediately with status `Working`

If `return_immediately` is `None` or `false`, keep current blocking behavior.

- [ ] **Step 6: Fix Submitted → Working transition**

In the blocking path, immediately after inserting the placeholder `Running` task, update its status to `Working` before calling `run_task`:

```rust
state.tasks.alter(&task_id, |_, mut ts| {
    if let AppTaskState::Running { ref mut task } = ts {
        task.status.state = TaskState::Working;
        task.status.timestamp = Some(chrono::Utc::now());
    }
    ts
});
```

- [ ] **Step 7: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add rust-agent/crates/a2a/src/types.rs rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/task_runner.rs
git commit -m "feat(a2a): add SendMessageConfiguration, contextId, returnImmediately, and terminal-state guards"
```

---

### Task 5: Fix SSE streaming — initial Task event and terminal failure event

**Files:**
- Modify: `src/streaming.rs`
- Modify: `src/handlers.rs`

- [ ] **Step 1: Emit initial Task snapshot as first SSE event**

In `send_message_stream` (and `stream_message_followup`), before spawning the bridge task, send an initial `StreamResponse` containing the current `Task` snapshot:

```rust
let initial_task = Task {
    id: task_id.clone(),
    context_id: Some(context_id.clone()),
    status: TaskStatus {
        state: TaskState::Submitted,
        message: None,
        timestamp: Some(chrono::Utc::now()),
    },
    history: None,
    artifacts: None,
    metadata: body.metadata.clone(),
};

let init_event = StreamResponse {
    task: Some(initial_task),
    message: None,
    status_update: None,
    artifact_update: None,
};
let _ = sse_tx.send(Ok(init_event.into_sse_event())).await;
```

- [ ] **Step 2: Emit terminal failure event when run_task errors**

In the runner tokio::spawn for streaming, when `run_task` returns `Err`, emit a `statusUpdate` with `state: Failed` and `final: true` before exiting:

```rust
Err(e) => {
    let msg = e.to_string();
    // ... store failed task state ...

    let fail_event = StreamResponse {
        status_update: Some(TaskStatusUpdateEvent {
            task_id: runner_task_id.clone(),
            status: TaskStatus {
                state: TaskState::Failed,
                message: Some(Message {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    context_id: None,
                    task_id: Some(runner_task_id.clone()),
                    role: Role::Agent,
                    parts: vec![Part::Text { text: msg.clone() }],
                    metadata: None,
                    extensions: None,
                }),
                timestamp: Some(chrono::Utc::now()),
            },
            final_: Some(true),
        }),
        task: None,
        message: None,
        artifact_update: None,
    };
    let _ = sse_tx.send(Ok(fail_event.into_sse_event())).await;
}
```

Same fix applies to `stream_message_followup`.

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/streaming.rs
git commit -m "fix(a2a): SSE stream emits initial Task and terminal failure events"
```

---

### Task 6: Fix cancel_task semantics

**Files:**
- Modify: `src/handlers.rs`
- Modify: `src/state.rs`
- Test: `tests/integration.rs`

- [ ] **Step 1: Store cancelled task data instead of discarding it**

Change `AppTaskState::Cancelled` to carry the full `Task`:

```rust
pub enum TaskState {
    Running { task: Task },
    Completed(Task),
    Failed { task: Task, error: String },
    Cancelled(Task),
}
```

- [ ] **Step 2: Reject cancellation of terminal-state tasks**

In `cancel_task` handler:

```rust
pub async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.tasks.get(&task_id) {
        Some(entry) => {
            let existing = entry.value().clone();
            match existing {
                AppTaskState::Completed(_) | AppTaskState::Failed { .. } | AppTaskState::Cancelled(_) => {
                    return Json(A2AErrorResponse {
                        error: A2AError::task_not_cancelable(&task_id),
                    }).into_response();
                }
                AppTaskState::Running { task } => {
                    let mut cancelled = task.clone();
                    cancelled.status = TaskStatus {
                        state: TaskState::Cancelled,
                        message: None,
                        timestamp: Some(chrono::Utc::now()),
                    };
                    state.tasks.insert(task_id.clone(), AppTaskState::Cancelled(cancelled.clone()));
                    return (StatusCode::OK, Json(cancelled)).into_response();
                }
            }
        }
        None => {
            return Json(A2AErrorResponse {
                error: A2AError::task_not_found(&task_id),
            }).into_response();
        }
    }
}
```

- [ ] **Step 3: Update get_task to handle Cancelled(Task)**

```rust
AppTaskState::Cancelled(task) => task.clone(),
```

- [ ] **Step 4: Update integration test expectations**

Change `cancel_nonexistent_task_returns_404` to assert on the new error response shape if needed. Add a new test `cancel_task_returns_task_with_cancelled_state`:

```rust
#[tokio::test]
async fn cancel_task_returns_cancelled_task() {
    let (client, base_url) = start_test_server().await;

    // Create a task first
    let create = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });
    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&create)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = res.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap();

    // Cancel it
    let cancel_res = client
        .post(format!("{}/tasks/{}/cancel", base_url, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(cancel_res.status(), StatusCode::OK);
    let cancelled: serde_json::Value = cancel_res.json().await.unwrap();
    assert_eq!(cancelled["status"]["state"], "cancelled");
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/state.rs rust-agent/crates/a2a/tests/integration.rs
git commit -m "fix(a2a): cancel returns Task, guards terminal states, preserves data"
```

---

### Task 7: Implement List Tasks endpoint

**Files:**
- Modify: `src/types.rs`
- Modify: `src/handlers.rs`
- Modify: `src/routes.rs`
- Test: `tests/integration.rs`

- [ ] **Step 1: Add ListTasksRequest and ListTasksResponse types**

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_timestamp_after: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResponse {
    pub tasks: Vec<Task>,
    pub next_page_token: String,
}
```

- [ ] **Step 2: Implement list_tasks handler**

```rust
pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ListTasksRequest>,
) -> impl IntoResponse {
    let mut tasks: Vec<Task> = state
        .tasks
        .iter()
        .filter_map(|entry| {
            let task = match entry.value() {
                AppTaskState::Running { task } => Some(task.clone()),
                AppTaskState::Completed(task) => Some(task.clone()),
                AppTaskState::Failed { task, .. } => Some(task.clone()),
                AppTaskState::Cancelled(task) => Some(task.clone()),
            }?;

            if let Some(ref ctx) = body.context_id {
                if task.context_id.as_ref() != Some(ctx) {
                    return None;
                }
            }
            if let Some(ref st) = body.status {
                if &task.status.state != st {
                    return None;
                }
            }
            if let Some(ref after) = body.status_timestamp_after {
                if task.status.timestamp.as_ref().map(|t| t < after).unwrap_or(true) {
                    return None;
                }
            }
            Some(task)
        })
        .collect();

    // Sort by updated timestamp desc
    tasks.sort_by(|a, b| b.status.timestamp.cmp(&a.status.timestamp));

    let resp = ListTasksResponse {
        tasks,
        next_page_token: String::new(),
    };
    (StatusCode::OK, Json(resp)).into_response()
}
```

- [ ] **Step 3: Add route**

In `routes.rs`:
```rust
.route("/tasks", post(handlers::list_tasks))
```

- [ ] **Step 4: Add integration test**

```rust
#[tokio::test]
async fn list_tasks_returns_created_task() {
    let (client, base_url) = start_test_server().await;

    let payload = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });
    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = res.json().await.unwrap();

    let list_res = client
        .post(format!("{}/tasks", base_url))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(list_res.status(), StatusCode::OK);
    let body: serde_json::Value = list_res.json().await.unwrap();
    assert!(body["tasks"].as_array().unwrap().len() >= 1);
    assert!(body.get("nextPageToken").is_some());
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add rust-agent/crates/a2a/src/types.rs rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/routes.rs rust-agent/crates/a2a/tests/integration.rs
git commit -m "feat(a2a): add ListTasks endpoint with filtering"
```

---

### Task 8: Implement Subscribe to Task endpoint

**Files:**
- Modify: `src/handlers.rs`
- Modify: `src/routes.rs`
- Modify: `src/streaming.rs`
- Test: `tests/integration.rs`

- [ ] **Step 1: Add route for subscribe**

In `routes.rs`:
```rust
.route("/tasks/{taskId}:subscribe", post(handlers::subscribe_task))
```

Wait — Axum 0.8 does not support `:subscribe` in the same segment as `{taskId}`. Use a nested path instead:
```rust
.route("/tasks/{taskId}/subscribe", post(handlers::subscribe_task))
```

- [ ] **Step 2: Implement subscribe_task handler**

This endpoint returns an SSE stream of real-time updates for an **existing** task. The first event must be the current `Task` snapshot. Since we don't have a live event bus for ongoing tasks, we can return the current snapshot and then close the stream (or heartbeat until completion if the task is running).

For MVP simplicity:
- Return the current `Task` as the first event
- If the task is in a terminal state, send `statusUpdate { state, final: true }` and close
- If the task is Running, we can't easily watch it without an event bus; return the snapshot + `statusUpdate { working }` and close

```rust
pub async fn subscribe_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let task = match state.tasks.get(&task_id) {
        Some(entry) => match entry.value() {
            AppTaskState::Running { task } => task.clone(),
            AppTaskState::Completed(task) => task.clone(),
            AppTaskState::Failed { task, .. } => task.clone(),
            AppTaskState::Cancelled(task) => task.clone(),
        },
        None => {
            return Json(A2AErrorResponse {
                error: A2AError::task_not_found(&task_id),
            }).into_response();
        }
    };

    let (sse_tx, sse_rx) = mpsc::channel::<Result<Event, Infallible>>(4);

    tokio::spawn(async move {
        let init = StreamResponse {
            task: Some(task.clone()),
            message: None,
            status_update: None,
            artifact_update: None,
        };
        let _ = sse_tx.send(Ok(init.into_sse_event())).await;

        let is_terminal = matches!(
            task.status.state,
            TaskState::Completed | TaskState::Failed | TaskState::Cancelled | TaskState::Rejected
        );

        if is_terminal {
            let final_evt = StreamResponse {
                status_update: Some(TaskStatusUpdateEvent {
                    task_id: task_id.clone(),
                    status: task.status.clone(),
                    final_: Some(true),
                }),
                task: None,
                message: None,
                artifact_update: None,
            };
            let _ = sse_tx.send(Ok(final_evt.into_sse_event())).await;
        }
    });

    let stream = ReceiverStream::new(sse_rx);
    Sse::new(stream).into_response()
}
```

- [ ] **Step 3: Add integration test**

```rust
#[tokio::test]
async fn subscribe_task_returns_snapshot() {
    let (client, base_url) = start_test_server().await;

    let payload = serde_json::json!({
        "message": {
            "messageId": "msg-1",
            "role": "user",
            "parts": [{ "text": "hello" }]
        }
    });
    let res = client
        .post(format!("{}/message:send", base_url))
        .json(&payload)
        .send()
        .await
        .unwrap();
    let task: serde_json::Value = res.json().await.unwrap();
    let task_id = task["id"].as_str().unwrap();

    let sub_res = client
        .post(format!("{}/tasks/{}/subscribe", base_url, task_id))
        .send()
        .await
        .unwrap();

    assert_eq!(sub_res.status(), StatusCode::OK);
    assert_eq!(
        sub_res.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/routes.rs rust-agent/crates/a2a/tests/integration.rs
git commit -m "feat(a2a): add Subscribe to Task endpoint"
```

---

### Task 9: Implement Extended Agent Card endpoint

**Files:**
- Modify: `src/types.rs`
- Modify: `src/handlers.rs`
- Modify: `src/routes.rs`
- Modify: `src/agent_card.rs`

- [ ] **Step 1: Add ExtendedAgentCard type**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedAgentCard {
    #[serde(flatten)]
    pub base: AgentCard,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<AgentProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
}
```

- [ ] **Step 2: Add route**

```rust
.route("/extendedAgentCard", get(handlers::get_extended_agent_card))
```

- [ ] **Step 3: Implement handler**

```rust
pub async fn get_extended_agent_card(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !state.agent_card.capabilities.extended_agent_card {
        return Json(A2AErrorResponse {
            error: A2AError::unsupported_operation("Extended agent card is not supported"),
        }).into_response();
    }

    let extended = ExtendedAgentCard {
        base: state.agent_card.clone(),
        provider: Some(AgentProvider {
            name: "rust-agent-project".to_string(),
            url: state.agent_card.url.clone(),
            logo_url: None,
        }),
    };
    (StatusCode::OK, Json(extended)).into_response()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add rust-agent/crates/a2a/src/types.rs rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/routes.rs rust-agent/crates/a2a/src/agent_card.rs
git commit -m "feat(a2a): add ExtendedAgentCard endpoint with capability gating"
```

---

### Task 10: Add Push Notification stub endpoints

**Files:**
- Modify: `src/handlers.rs`
- Modify: `src/routes.rs`

- [ ] **Step 1: Add routes for push notification CRUD**

```rust
.route("/tasks/{taskId}/pushNotificationConfigs", post(handlers::create_push_config).get(handlers::list_push_configs))
.route("/tasks/{taskId}/pushNotificationConfigs/{configId}", get(handlers::get_push_config).delete(handlers::delete_push_config))
```

- [ ] **Step 2: Implement stub handlers that always return PushNotificationNotSupportedError**

Since `capabilities.push_notifications` is `false`, all push endpoints must return the standard not-supported error:

```rust
pub async fn create_push_config() -> impl IntoResponse {
    Json(A2AErrorResponse {
        error: A2AError::new("PushNotificationNotSupportedError", "Push notifications are not supported"),
    })
}

pub async fn list_push_configs() -> impl IntoResponse {
    Json(A2AErrorResponse {
        error: A2AError::new("PushNotificationNotSupportedError", "Push notifications are not supported"),
    })
}

pub async fn get_push_config() -> impl IntoResponse {
    Json(A2AErrorResponse {
        error: A2AError::new("PushNotificationNotSupportedError", "Push notifications are not supported"),
    })
}

pub async fn delete_push_config() -> impl IntoResponse {
    Json(A2AErrorResponse {
        error: A2AError::new("PushNotificationNotSupportedError", "Push notifications are not supported"),
    })
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add rust-agent/crates/a2a/src/handlers.rs rust-agent/crates/a2a/src/routes.rs
git commit -m "feat(a2a): add push notification stub endpoints returning not-supported"
```

---

### Task 11: Final integration verification and API doc update

**Files:**
- Modify: `API.md`
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add comprehensive integration tests for error paths**

Add tests for:
- `send_followup_to_terminal_task_returns_error`
- `cancel_completed_task_returns_not_cancelable`
- `list_tasks_filters_by_context_id`
- `extended_agent_card_returns_error_when_disabled`
- `push_notification_returns_not_supported`

- [ ] **Step 2: Update API.md to reflect all changes**

Update every endpoint description, request/response schema, error code, and code example to match the v1.0-compliant implementation. Specifically:
- Document `configuration` in `SendMessageRequest`
- Document `contextId` behavior
- Document standard error codes (`TaskNotFoundError`, `TaskNotCancelableError`, etc.)
- Update cancel endpoint to show it returns `Task` instead of `204`
- Add List Tasks, Subscribe, Extended Agent Card, Push Notification endpoints
- Document `A2A-Version` header requirement

- [ ] **Step 3: Run full test suite**

Run: `cargo test --manifest-path rust-agent/crates/a2a/Cargo.toml`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add rust-agent/crates/a2a/API.md rust-agent/crates/a2a/tests/integration.rs
git commit -m "docs(a2a): update API.md for v1.0 spec compliance and add error-path tests"
```

---

## Self-Review Checklist

**1. Spec coverage:**
- ✅ AgentCard fields (protocolVersion, capabilities.extendedAgentCard)
- ✅ TaskState.Rejected
- ✅ Task.artifacts/history omission rules
- ✅ Standard error types with `details` array
- ✅ A2A-Version header validation
- ✅ SendMessageConfiguration (returnImmediately, acceptedOutputModes, historyLength)
- ✅ contextId generation and inference
- ✅ Terminal-state guards on follow-up and cancel
- ✅ Submitted → Working transition
- ✅ SSE initial Task event
- ✅ SSE terminal failure event
- ✅ Cancel returns Task + guards terminal states
- ✅ List Tasks endpoint
- ✅ Subscribe to Task endpoint
- ✅ Extended Agent Card endpoint
- ✅ Push Notification stubs

**2. Placeholder scan:**
- ✅ No TBD/TODO/fill-in-later
- ✅ All steps contain actual code or exact commands
- ✅ No vague descriptions like "add appropriate validation"

**3. Type consistency:**
- ✅ `A2AError` used throughout instead of ad-hoc JSON
- ✅ `TaskState` enum consistent across types.rs, handlers.rs, streaming.rs
- ✅ `AppTaskState::Cancelled(Task)` carries data everywhere

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-20-a2a-spec-compliance.md`.**

**Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration. Tasks 1–11 are mostly independent; some sequential dependencies exist (e.g., Task 3 error types must land before Task 4–10 can use them).

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
