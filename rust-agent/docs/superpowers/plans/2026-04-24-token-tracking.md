# Token Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement token usage tracking in `crates/core`, extracting usage data from LLM API responses, accumulating via `TokenTracker`, and displaying in CLI.

**Architecture:** `TokenUsage` (per-call) → `TokenTracker` (session accumulator with `Arc<Mutex>`) → `AgentEvent::TurnEnd` (event delivery) → CLI display.

**Design Decisions:**

- `Arc<std::sync::Mutex>` for concurrency (lock held for nanoseconds, never across await)
- Sub-agents share `TokenTracker` via `AgentApp.clone()` (no merge needed)
- Session-level only (no persistence), token count only (no cost estimation)

**Tech Stack:** Rust 2024, serde, std::sync

---

## File Structure

| File                                     | Operation | Responsibility                                      |
| ---------------------------------------- | --------- | --------------------------------------------------- |
| `crates/core/src/api/types.rs`           | Modify    | Add `TokenUsage`, add `usage` to `ProviderResponse` |
| `crates/core/src/api/anthropic.rs`       | Modify    | Parse Anthropic usage, update mock data             |
| `crates/core/src/api/openai.rs`          | Modify    | Parse OpenAI usage                                  |
| `crates/core/src/infra/token_tracker.rs` | Create    | Token tracker module                                |
| `crates/core/src/agent.rs`               | Modify    | Integrate tracker, update `AgentEvent::TurnEnd`     |
| `crates/core/src/infra/mod.rs`           | Modify    | Register new module                                 |
| `crates/core/src/lib.rs`                 | Modify    | Export new types                                    |
| `crates/server/src/sse.rs`               | Modify    | SSE serialization for token_usage                   |
| `cli/src/app.tsx`                        | Modify    | Display token info                                  |

---

## Task 1: Data Layer — `TokenUsage` + `ProviderResponse`

**Files:**

- Modify: `crates/core/src/api/types.rs`

- [ ] **Step 1: Add `TokenUsage` struct**

Add after the `ProviderResponse` definition:

```rust
/// 单次 LLM API 调用的 token 用量
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// 输入 token 数（含缓存命中部分）
    pub input_tokens: u64,
    /// 输出 token 数
    pub output_tokens: u64,
    /// 缓存读取的 token 数（Anthropic 的 `cache_read_input_tokens`，
    /// OpenAI Chat Completions 的 `prompt_tokens_details.cached_tokens`）
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// 缓存写入的 token 数（Anthropic 的 `cache_creation_input_tokens`，
    /// OpenAI 当前无对应字段）
    #[serde(default)]
    pub cache_creation_tokens: u64,
}

impl std::ops::AddAssign for TokenUsage {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
    }
}

impl TokenUsage {
    /// 总 token 数（输入 + 输出）
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}
```

- [ ] **Step 2: Add `usage` field to `ProviderResponse`**

Change `ProviderResponse` to include `usage`:

```rust
pub struct ProviderResponse {
    pub content: Vec<ResponseContentBlock>,
    pub stop_reason: String,
    /// 本次 API 调用的 token 用量（部分 provider 可能不返回）
    pub usage: TokenUsage,
}
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p rust-agent-core 2>&1
```

Expected: Errors in anthropic.rs and openai.rs (missing `usage` field) — will fix in Task 2.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/api/types.rs
git commit -m "feat(core): add TokenUsage type and usage field to ProviderResponse"
```

---

## Task 2: Provider Layer — Anthropic + OpenAI Usage Parsing

**Files:**

- Modify: `crates/core/src/api/anthropic.rs`
- Modify: `crates/core/src/api/openai.rs`

- [ ] **Step 1: Add `AnthropicUsage` to `MessagesResponse` in `anthropic.rs`**

Add `usage` field to `MessagesResponse`:

```rust
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct MessagesResponse {
    pub content: Vec<ResponseContentBlock>,
    pub stop_reason: Option<String>,
    /// Anthropic 返回的 token 用量
    pub usage: AnthropicUsage,
}

/// Anthropic API 返回的 usage 字段
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnthropicUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}
```

- [ ] **Step 2: Convert Anthropic usage in `create_message` method**

Update the `create_message` method to include usage:

```rust
Ok(ProviderResponse {
    content: raw_response.content,
    stop_reason,
    usage: TokenUsage {
        input_tokens: raw_response.usage.input_tokens,
        output_tokens: raw_response.usage.output_tokens,
        cache_read_tokens: raw_response.usage.cache_read_input_tokens,
        cache_creation_tokens: raw_response.usage.cache_creation_input_tokens,
    },
})
```

- [ ] **Step 3: Update mock response bodies in anthropic tests**

Add `"usage"` to all mock JSON responses:

```json
"usage": {"input_tokens": 10, "output_tokens": 5}
```

- [ ] **Step 4: Add `OpenAIUsage` to `OpenAIResponse` in `openai.rs`**

```rust
#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: OpenAIUsage,
}

#[derive(Deserialize, Default)]
struct OpenAIUsage {
    #[serde(alias = "prompt_tokens")]
    input_tokens: u64,
    #[serde(alias = "completion_tokens")]
    output_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAITokensDetails>,
    #[serde(default)]
    input_tokens_details: Option<OpenAITokensDetails>,
}

#[derive(Deserialize)]
struct OpenAITokensDetails {
    #[serde(default)]
    cached_tokens: u64,
}
```

- [ ] **Step 5: Convert OpenAI usage in `convert_response`**

```rust
usage: TokenUsage {
    input_tokens: response.usage.input_tokens,
    output_tokens: response.usage.output_tokens,
    cache_read_tokens: {
        let from_prompt = response.usage.prompt_tokens_details
            .as_ref()
            .map(|d| d.cached_tokens)
            .unwrap_or(0);
        let from_input = response.usage.input_tokens_details
            .as_ref()
            .map(|d| d.cached_tokens)
            .unwrap_or(0);
        from_prompt + from_input
    },
    cache_creation_tokens: 0,
},
```

- [ ] **Step 6: Verify compilation**

```bash
cargo check -p rust-agent-core 2>&1
```

Expected: Compiles successfully.

- [ ] **Step 7: Run tests**

```bash
cargo test -p rust-agent-core 2>&1
```

Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/api/anthropic.rs crates/core/src/api/openai.rs
git commit -m "feat(core): parse token usage from Anthropic and OpenAI responses"
```

---

## Task 3: Tracking Layer — `TokenTracker` Module

**Files:**

- Create: `crates/core/src/infra/token_tracker.rs`
- Modify: `crates/core/src/infra/mod.rs`

- [ ] **Step 1: Create `token_tracker.rs`**

```rust
//! Token 用量追踪器
//!
//! 在会话级别累计追踪 token 用量，支持多线程共享（子代理通过 Arc 共享同一个 tracker）。

use std::collections::HashMap;
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
```

- [ ] **Step 2: Register module in `infra/mod.rs`**

Add `pub mod token_tracker;` to the module list.

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p rust-agent-core 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/infra/token_tracker.rs crates/core/src/infra/mod.rs
git commit -m "feat(core): add TokenTracker for session-level token usage tracking"
```

---

## Task 4: Integration Layer — AgentApp + AgentEvent + SSE + Exports

**Files:**

- Modify: `crates/core/src/agent.rs`
- Modify: `crates/server/src/sse.rs`
- Modify: `crates/core/src/lib.rs`

- [ ] **Step 1: Add `token_tracker` field to `AgentApp`**

Add `token_tracker: crate::infra::token_tracker::TokenTracker` field to `AgentApp`.

- [ ] **Step 2: Initialize tracker in `from_env()`**

Add `token_tracker: TokenTracker::new()` to the constructor.

- [ ] **Step 3: Update `AgentEvent::TurnEnd` to include token usage**

```rust
TurnEnd {
    api_calls: usize,
    token_usage: Option<TokenUsage>,
},
```

- [ ] **Step 4: Record usage in `run_agent_loop`**

After each `self.client.create_message(&request).await`, call:

```rust
self.token_tracker.record(&self.model, &response.usage);
```

- [ ] **Step 5: Emit token usage in `TurnEnd` events**

Include `token_usage: Some(self.token_tracker.snapshot().total)` in TurnEnd.

- [ ] **Step 6: Update SSE serialization in `sse.rs`**

Add `token_usage` to the `turn_end` event JSON.

- [ ] **Step 7: Export new types in `lib.rs`**

Add `TokenUsage` and `TokenTracker` to public exports.

- [ ] **Step 8: Verify compilation**

```bash
cargo check 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add crates/core/src/agent.rs crates/core/src/lib.rs crates/server/src/sse.rs
git commit -m "feat(core): integrate TokenTracker into AgentApp and AgentEvent"
```

---

## Task 5: Display Layer — CLI + Tests

**Files:**

- Modify: `cli/src/app.tsx`

- [ ] **Step 1: Update `turn_end` handler in `app.tsx`**

Display token usage info when available:

```tsx
case 'turn_end': {
  // ... existing code ...
  const tokenUsage = event.data?.token_usage;
  let info = `── 完成，API 调用 ${apiCalls} 次`;
  if (tokenUsage) {
    info += ` │ Token: ${tokenUsage.input_tokens}入/${tokenUsage.output_tokens}出`;
    if (tokenUsage.cache_read_tokens > 0) {
      info += ` (缓存命中 ${tokenUsage.cache_read_tokens})`;
    }
  }
  info += ' ──';
  setMessages(prev => [...prev, { role: 'system', content: info }]);
  break;
}
```

- [ ] **Step 2: Verify full build**

```bash
cargo build 2>&1
cd cli && npm run build 2>&1
```

- [ ] **Step 3: Run all tests**

```bash
cargo test 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add cli/src/app.tsx
git commit -m "feat(cli): display token usage in turn_end event"
```
