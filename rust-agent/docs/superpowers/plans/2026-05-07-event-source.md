# SSE 事件来源标识实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 AgentEvent 添加 EventSource 来源标识，使前端能精确区分主 Agent 与 Bot 子代理的事件，消除并行 call_bot 归属错误。

**Architecture:** 在后端 AgentEvent 每个变体中注入 `source: EventSource` 字段（主 Agent 为 `Main`，Bot 为 `Bot { name, call_id }`），SSE 序列化时自动输出到 JSON；前端解析 `source` 字段取代 `activeBotName` 推断逻辑。

**Tech Stack:** Rust (Tokio, Axum, Serde), TypeScript/React (Zustand)

---

## 文件结构

| 文件 | 职责 |
|------|------|
| `crates/core/src/agent.rs` | 添加 `EventSource` 类型；修改 `AgentEvent` 定义；为所有事件构造点注入 `source` |
| `crates/core/src/lib.rs` | 导出 `EventSource` |
| `crates/core/Cargo.toml` | 添加 `nanoid` 依赖（Bot `call_id` 生成） |
| `crates/server/src/sse.rs` | SSE 序列化时注入 `"source"` 字段 |
| `crates/server/src/openai_compat.rs` | 更新 `AgentEvent` 模式匹配（新增字段兼容） |
| `crates/server/src/routes.rs` | 更新路由层 `AgentEvent` 构造点 |
| `web/src/types/wire.ts` | 扩展 `SSEEvent` 类型，添加 `source` 字段 |
| `web/src/types/ui.ts` | 扩展 `UIToolCall`，添加 `source`、`botText`、`botThinking` |
| `web/src/store/chat.ts` | 替换 `activeBotName` 推断逻辑，按 `source.call_id` 精确归属 |

---

## Task 1: 定义 EventSource 并修改 AgentEvent

**Files:**
- Modify: `crates/core/src/agent.rs:26-59`（AgentEvent 定义前插入 EventSource）
- Modify: `crates/core/src/agent.rs:26-59`（AgentEvent 每个变体）
- Modify: `crates/core/Cargo.toml`（添加 nanoid 依赖）
- Test: `cargo check -p rust-agent-core`

### Step 1: 添加 nanoid 依赖

```toml
# crates/core/Cargo.toml
[dependencies]
# 在现有依赖列表末尾添加
nanoid = "0.4"
```

Run: `cargo check -p rust-agent-core`
Expected: 通过（仅新增依赖，无代码改动）

### Step 2: 在 agent.rs 中添加 EventSource 类型

在 `AgentEvent` 定义前（约第 26 行）插入：

```rust
/// 事件来源标识：区分事件由主 Agent 还是某个 Bot 子代理产生
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum EventSource {
    /// 主 Agent（协调员）
    Main,
    /// Bot 子代理
    Bot {
        /// Bot 名称（如 "代码审查Bot"）
        name: String,
        /// 本次 call_bot 调用的唯一标识，用于区分并行的多个 Bot
        call_id: String,
    },
}

impl Default for EventSource {
    fn default() -> Self {
        Self::Main
    }
}
```

### Step 3: 修改 AgentEvent 枚举定义

将 `crates/core/src/agent.rs:26-59` 的 `AgentEvent` 枚举改为：

```rust
#[derive(Clone, Debug)]
pub enum AgentEvent {
    TextDelta { content: String, source: EventSource },
    /// 思考内容增量（Kimi 等兼容层返回的 reasoning_content）
    ThinkingDelta { content: String, source: EventSource },
    ToolCall {
        id: Option<String>,
        name: String,
        input: serde_json::Value,
        parallel_index: Option<(usize, usize)>,
        source: EventSource,
    },
    ToolResult {
        id: Option<String>,
        name: String,
        output: String,
        parallel_index: Option<(usize, usize)>,
        source: EventSource,
    },
    TurnEnd {
        api_calls: usize,
        token_usage: Option<crate::api::types::TokenUsage>,
        source: EventSource,
    },
    Done { source: EventSource },
    Error {
        code: String,
        message: String,
        source: EventSource,
    },
    /// API 重试进行中，通知客户端当前进度
    Retrying {
        attempt: u32,
        max_retries: u32,
        wait_seconds: u64,
        detail: String,
        source: EventSource,
    },
}
```

Run: `cargo check -p rust-agent-core`
Expected: 编译失败（大量构造点未更新，这是预期的）

---

## Task 2: 修改 run_agent_loop 内部所有事件构造点

**Files:**
- Modify: `crates/core/src/agent.rs`（多处）
- Test: `cargo check -p rust-agent-core`

### Step 1: 修改 run_agent_loop 签名

将 `crates/core/src/agent.rs:449-459` 的签名：

```rust
    async fn run_agent_loop(
        &self,
        ctx: &mut ContextService,
        system_prompt: String,
        config: AgentRunConfig,
        logger: &mut ConversationLogger,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
        cwd: Option<&Path>,
        parent_session_id: Option<&str>,
        source: EventSource, // 新增
    ) -> AgentResult<String> {
```

### Step 2: 修改 TextDelta / ThinkingDelta 构造（stream 消费循环内）

将第 610-614 行：
```rust
crate::api::types::LlmStreamChunk::TextDelta(text) => {
    current_text.push_str(&text);
    if config.emit_events && !text.is_empty() {
        let _ = event_tx.send(AgentEvent::TextDelta { content: text, source: source.clone() }).await;
    }
}
```

将第 616-622 行：
```rust
crate::api::types::LlmStreamChunk::ThinkingDelta(thinking) => {
    current_thinking.push_str(&thinking);
    if config.emit_events && !thinking.is_empty() {
        let _ = event_tx.send(AgentEvent::ThinkingDelta { content: thinking, source: source.clone() }).await;
    }
}
```

### Step 3: 修改 Retrying 事件构造

将第 507-515 行：
```rust
let _ = event_tx_for_retry
    .send(AgentEvent::Retrying {
        attempt: notif.attempt,
        max_retries: notif.max_retries,
        wait_seconds: notif.wait_seconds,
        detail: notif.detail,
        source: source.clone(), // 继承调用者的 source（主 Agent 或 Bot）
    })
    .await;
```

### Step 4: 修改 Error 事件构造（stream_message 失败）

将第 565-570 行：
```rust
let _ = event_tx
    .send(AgentEvent::Error {
        code: code.to_owned(),
        message: format!("{e:#}"),
        source: source.clone(),
    })
    .await;
```

### Step 5: 修改 Error 事件构造（stream 中断）

将第 598-603 行：
```rust
let _ = event_tx
    .send(AgentEvent::Error {
        code: "stream_error".to_owned(),
        message: format!("流式响应中断: {e:#}"),
        source: source.clone(),
    })
    .await;
```

### Step 6: 修改普通 ToolCall / ToolResult 构造（other_calls）

将第 761-768 行：
```rust
let _ = event_tx
    .send(AgentEvent::ToolCall {
        id: None,
        name: tc.name.clone(),
        input: tc.input.clone(),
        parallel_index: None,
        source: source.clone(),
    })
    .await;
```

将第 784-791 行（成功结果）：
```rust
let _ = event_tx
    .send(AgentEvent::ToolResult {
        id: None,
        name: tc.name.clone(),
        output: dispatch.output.clone(),
        parallel_index: None,
        source: source.clone(),
    })
    .await;
```

将第 798-805 行（失败结果）：
```rust
let _ = event_tx
    .send(AgentEvent::ToolResult {
        id: None,
        name: tc.name.clone(),
        output: msg.clone(),
        parallel_index: None,
        source: source.clone(),
    })
    .await;
```

### Step 7: 修改 call_bot ToolCall / ToolResult 构造

将第 828-835 行（call_bot 的 ToolCall）：
```rust
let _ = event_tx
    .send(AgentEvent::ToolCall {
        id: None,
        name: "call_bot".to_owned(),
        input: tc.input.clone(),
        parallel_index: None,
        source: bot_source.clone(), // bot_source 在循环外生成
    })
    .await;
```

将第 867-877 行（call_bot 的 ToolResult）：
```rust
let _ = event_tx
    .send(AgentEvent::ToolResult {
        id: None,
        name: "call_bot".to_owned(),
        output: output.clone(),
        parallel_index: if bot_calls.len() > 1 {
            Some((idx + 1, bot_calls.len()))
        } else {
            None
        },
        source: bot_source.clone(),
    })
    .await;
```

### Step 8: 修改 task ToolCall / ToolResult 构造

将第 911-922 行：
```rust
let _ = event_tx
    .send(AgentEvent::ToolCall {
        id: None,
        name: "task".to_owned(),
        input: tc.input.clone(),
        parallel_index: if is_parallel {
            Some((idx + 1, actual_calls.len())
        } else {
            None
        },
        source: source.clone(),
    })
    .await;
```

将第 957-968 行：
```rust
let _ = event_tx
    .send(AgentEvent::ToolResult {
        id: None,
        name: "task".to_owned(),
        output: output.clone(),
        parallel_index: if is_parallel {
            Some((idx + 1, actual_calls.len())
        } else {
            None
        },
        source: source.clone(),
    })
    .await;
```

### Step 9: 修改 TurnEnd / TextDelta 构造（截断/完成路径）

将第 694-697 行（截断警告）：
```rust
let _ = event_tx
    .send(AgentEvent::TextDelta {
        content: "\n\n⚠️ 回复因达到 token 上限而被截断。如需继续，请简化输入或开启新的对话。".to_owned(),
        source: source.clone(),
    })
    .await;
```

将第 706-711 行：
```rust
let _ = event_tx
    .send(AgentEvent::TurnEnd {
        api_calls: api_call_count,
        token_usage: Some(self.token_tracker.snapshot().total),
        source: source.clone(),
    })
    .await;
```

将第 1001-1003 行：
```rust
let _ = event_tx
    .send(AgentEvent::TextDelta { content: "对话已手动压缩。".to_owned(), source: source.clone() })
    .await;
```

将第 1005-1010 行：
```rust
let _ = event_tx
    .send(AgentEvent::TurnEnd {
        api_calls: api_call_count,
        token_usage: Some(self.token_tracker.snapshot().total),
        source: source.clone(),
    })
    .await;
```

将第 1016-1021 行：
```rust
let _ = event_tx
    .send(AgentEvent::TextDelta {
        content: "已达到工具调用轮数安全上限（30轮），自动停止。".to_owned(),
        source: source.clone(),
    })
    .await;
```

将第 1022-1027 行：
```rust
let _ = event_tx
    .send(AgentEvent::TurnEnd {
        api_calls: api_call_count,
        token_usage: Some(self.token_tracker.snapshot().total),
        source: source.clone(),
    })
    .await;
```

Run: `cargo check -p rust-agent-core`
Expected: 仍有编译错误（子代理入口未更新）

---

## Task 3: 更新子代理和主入口调用链

**Files:**
- Modify: `crates/core/src/agent.rs`（run_subagent、run_bot、handle_user_turn、handle_bot_turn）
- Test: `cargo check -p rust-agent-core`

### Step 1: 修改 run_subagent

将第 1031-1055 行：
```rust
    async fn run_subagent(
        &self,
        prompt: String,
        logger: &mut ConversationLogger,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    ) -> AgentResult<String> {
        let system_prompt = build_subagent_prompt(...);
        let mut sub_ctx = ContextService::new();
        sub_ctx.push_user_text(&prompt);
        self.run_agent_loop(
            &mut sub_ctx,
            system_prompt,
            AgentRunConfig::child(),
            logger,
            event_tx,
            None,
            None,
            EventSource::Main, // 子代理事件标记为 Main（task 不向外发事件）
        )
        .await
    }
```

### Step 2: 修改 run_bot 签名和内部调用

将第 1061-1067 行签名：
```rust
    async fn run_bot(
        &self,
        bot_name: &str,
        task: &str,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
        parent_session_id: &str,
        source: EventSource, // 新增
    ) -> AgentResult<String> {
```

将第 1173-1183 行（run_bot 中调用 run_agent_loop）：
```rust
        let result = bot_app
            .run_agent_loop(
                &mut bot_ctx,
                system_prompt,
                AgentRunConfig::bot_api(),
                &mut sub_logger,
                event_tx,
                None,
                Some(parent_session_id),
                source, // 使用传入的 Bot source
            )
            .await;
```

### Step 3: 修改 handle_user_turn

将第 387-397 行：
```rust
        let result = self
            .run_agent_loop(
                ctx,
                system_prompt,
                AgentRunConfig::parent(),
                &mut logger,
                &event_tx,
                cwd,
                parent_session_id,
                EventSource::Main,
            )
            .await;
```

### Step 4: 修改 handle_bot_turn

将第 429-438 行：
```rust
        let result = self
            .run_agent_loop(
                &mut sub_ctx,
                system_prompt,
                AgentRunConfig::bot_api(),
                &mut logger,
                &event_tx,
                cwd,
                None,
                EventSource::Main, // HTTP Bot API 直接调用，视为 Main
            )
            .await;
```

### Step 5: 修改 bot_calls 并行调度，生成 call_id 和 bot_source

将第 817-858 行（bot_calls 处理）：
```rust
            if !bot_calls.is_empty() {
                // 为每个并行 Bot 调用预生成 call_id 和 source
                let mut bot_sources: Vec<(String, EventSource)> = Vec::new();
                for tc in &bot_calls {
                    let bot_name = tc
                        .input
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let call_id = nanoid::nanoid!();
                    let bot_source = EventSource::Bot {
                        name: bot_name.clone(),
                        call_id: call_id.clone(),
                    };
                    bot_sources.push((bot_name, bot_source));
                }

                for (idx, tc) in bot_calls.iter().enumerate() {
                    let (_, bot_source) = &bot_sources[idx];
                    let input_preview = preview_text(&tc.input.to_string(), 200);
                    logger.log(&format!(
                        "=== 工具调用: call_bot(name={}) ===\n输入: {input_preview}",
                        bot_sources[idx].0
                    ));
                    let _ = event_tx
                        .send(AgentEvent::ToolCall {
                            id: None,
                            name: "call_bot".to_owned(),
                            input: tc.input.clone(),
                            parallel_index: None,
                            source: bot_source.clone(),
                        })
                        .await;
                }

                let mut bot_handles = Vec::new();
                for (idx, tc) in bot_calls.iter().enumerate() {
                    let (_, bot_source) = &bot_sources[idx];
                    let bot_task = tc
                        .input
                        .get("task")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let app = self.clone();
                    let event_tx = Arc::clone(event_tx);
                    let sid = parent_session_id.unwrap_or("").to_owned();
                    let source_for_spawn = bot_source.clone();
                    bot_handles.push(tokio::spawn(async move {
                        app.run_bot(
                            &bot_sources[idx].0,
                            &bot_task,
                            &event_tx,
                            &sid,
                            source_for_spawn,
                        )
                        .await
                    }));
                }

                for (idx, handle) in bot_handles.into_iter().enumerate() {
                    let tc_id = bot_calls[idx].id.clone();
                    let (_, bot_source) = &bot_sources[idx];
                    let output = match handle.await {
                        Ok(Ok(out)) => out,
                        Ok(Err(e)) => format!("Bot 子代理执行失败: {e}"),
                        Err(e) => format!("Bot 子代理异常: {e}"),
                    };
                    let _ = event_tx
                        .send(AgentEvent::ToolResult {
                            id: None,
                            name: "call_bot".to_owned(),
                            output: output.clone(),
                            parallel_index: if bot_calls.len() > 1 {
                                Some((idx + 1, bot_calls.len()))
                            } else {
                                None
                            },
                            source: bot_source.clone(),
                        })
                        .await;
                    // ... 日志不变
                    results.push(tool_result_block(&tc_id, output));
                }
            }
```

Run: `cargo check -p rust-agent-core`
Expected: 通过（核心 crate 编译成功）

---

## Task 4: 更新 server crate

**Files:**
- Modify: `crates/server/src/sse.rs`
- Modify: `crates/server/src/openai_compat.rs`
- Modify: `crates/server/src/routes.rs`
- Test: `cargo check -p rust-agent-server`

### Step 1: 更新 SSE 序列化（sse.rs）

将 `crates/server/src/sse.rs` 全部替换：

```rust
use axum::response::sse::Event;
use rust_agent_core::agent::AgentEvent;
use serde_json::json;

/// 将 AgentEvent 转换为 SSE Event
pub fn agent_event_to_sse(event: AgentEvent) -> Event {
    match event {
        AgentEvent::TextDelta { content, source } => Event::default()
            .event("text_delta")
            .data(json!({ "content": content, "source": source }).to_string()),
        AgentEvent::ThinkingDelta { content, source } => Event::default()
            .event("thinking_delta")
            .data(json!({ "content": content, "source": source }).to_string()),
        AgentEvent::ToolCall {
            id,
            name,
            input,
            parallel_index,
            source,
        } => {
            let mut data = json!({ "name": name, "input": input, "source": source });
            if let Some(id) = id {
                data["id"] = json!(id);
            }
            if let Some((idx, total)) = parallel_index {
                data["parallel_index"] = json!({ "index": idx, "total": total });
            }
            Event::default().event("tool_call").data(data.to_string())
        }
        AgentEvent::ToolResult {
            id,
            name,
            output,
            parallel_index,
            source,
        } => {
            let mut data = json!({ "name": name, "output": output, "source": source });
            if let Some(id) = id {
                data["id"] = json!(id);
            }
            if let Some((idx, total)) = parallel_index {
                data["parallel_index"] = json!({ "index": idx, "total": total });
            }
            Event::default().event("tool_result").data(data.to_string())
        }
        AgentEvent::TurnEnd {
            api_calls,
            token_usage,
            source,
        } => {
            let mut data = json!({ "api_calls": api_calls, "source": source });
            if let Some(usage) = token_usage {
                data["token_usage"] = json!({
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cache_read_tokens": usage.cache_read_tokens,
                    "cache_creation_tokens": usage.cache_creation_tokens,
                });
            }
            Event::default().event("turn_end").data(data.to_string())
        }
        AgentEvent::Done { source } => Event::default()
            .event("done")
            .data(json!({ "source": source }).to_string()),
        AgentEvent::Error {
            code,
            message,
            source,
        } => Event::default()
            .event("error")
            .data(json!({ "code": code, "message": message, "source": source }).to_string()),
        AgentEvent::Retrying {
            attempt,
            max_retries,
            wait_seconds,
            detail,
            source,
        } => Event::default().event("retrying").data(
            json!({
                "attempt": attempt,
                "max_retries": max_retries,
                "wait_seconds": wait_seconds,
                "detail": detail,
                "source": source,
            })
            .to_string(),
        ),
    }
}
```

### Step 2: 更新 OpenAI 兼容层匹配（openai_compat.rs）

将 `crates/server/src/openai_compat.rs:178-225` 的模式匹配更新：

```rust
    while let Some(event) = event_rx.recv().await {
        match event {
            rust_agent_core::agent::AgentEvent::TextDelta { content, .. } => {
                final_text.push_str(&content);
            }
            rust_agent_core::agent::AgentEvent::ToolCall {
                id,
                name,
                input,
                parallel_index: _,
                source: _,
            } => {
                tool_calls_collected.push(json!({
                    "id": id.unwrap_or_else(|| format!("call_{}", short_id())),
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": input.to_string(),
                    }
                }));
            }
            rust_agent_core::agent::AgentEvent::TurnEnd {
                api_calls: _,
                token_usage: _,
                source: _,
            } => {
                if !tool_calls_collected.is_empty() {
                    stop_reason = "tool_calls".to_owned();
                }
            }
            rust_agent_core::agent::AgentEvent::Error {
                code,
                message,
                source: _,
            } => {
                // ... 现有逻辑不变
            }
            _ => {}
        }
    }
```

### Step 3: 更新 routes.rs 中的 AgentEvent 构造

首先，更新 `routes.rs` 的现有导入（第 18 行）以包含 `EventSource`：
```rust
use rust_agent_core::agent::{AgentApp, AgentEvent, EventSource};
```

将第 415-419 行：
```rust
broadcaster.send(&session_id, AgentEvent::Error {
    code: "profile_error".to_owned(),
    message: format!("{e:#}"),
    source: EventSource::Main,
});
broadcaster.send(&session_id, AgentEvent::Done { source: EventSource::Main });
```

将第 455-458 行：
```rust
broadcaster.send(&session_id, rust_agent_core::agent::AgentEvent::Error {
    code: "agent_error".to_owned(),
    message: format!("{e:#}"),
    source: EventSource::Main,
});
```

将第 469 行：
```rust
broadcaster.send(&session_id, rust_agent_core::agent::AgentEvent::Done { source: EventSource::Main });
```

将第 1007-1011 行：
```rust
let _ = event_tx
    .send(rust_agent_core::agent::AgentEvent::Error {
        code: "bot_agent_error".to_owned(),
        message: format!("{e:#}"),
        source: EventSource::Main,
    })
    .await;
```

将第 1015-1017 行：
```rust
let _ = event_tx
    .send(rust_agent_core::agent::AgentEvent::Done { source: EventSource::Main })
    .await;
```

### Step 4: 导出 EventSource（lib.rs）

将 `crates/core/src/lib.rs:18`：
```rust
pub use agent::{AgentApp, AgentEvent, EventSource};
```

Run: `cargo check -p rust-agent-server`
Expected: 通过（server crate 编译成功）

---

## Task 5: 更新前端类型

**Files:**
- Modify: `web/src/types/wire.ts`
- Modify: `web/src/types/ui.ts`
- Test: `cd web && npx tsc --noEmit`

### Step 1: 扩展 wire.ts 的 SSEEvent 类型

在 `web/src/types/wire.ts` 中添加 `EventSource` 类型并注入到每个 SSEEvent：

```typescript
// ── 事件来源标识 ──
export type EventSource =
  | { role: 'main' }
  | { role: 'bot'; name: string; call_id: string }

// ── SSE Events ──
export type SSEEvent =
  | { event: 'text_delta'; data: { content: string; source: EventSource } }
  | { event: 'thinking_delta'; data: { content: string; source: EventSource } }
  | {
      event: 'tool_call'
      data: {
        name: string
        input: unknown
        id: string | null
        parallel_index?: ParallelIndex
        source: EventSource
      }
    }
  | {
      event: 'tool_result'
      data: {
        name: string
        output: string
        id: string | null
        parallel_index?: ParallelIndex
        source: EventSource
      }
    }
  | {
      event: 'turn_end'
      data: { api_calls: number; token_usage?: TokenUsage; source: EventSource }
    }
  | { event: 'done'; data: { source: EventSource } }
  | { event: 'error'; data: { code: string; message: string; source: EventSource } }
  | {
      event: 'retrying'
      data: {
        attempt: number
        max_retries: number
        wait_seconds: number
        detail?: string
        source: EventSource
      }
    }
```

### Step 2: 扩展 ui.ts 的 UIToolCall 和 StreamingState

在 `web/src/types/ui.ts` 中：

```typescript
export interface UIToolCall {
  id: string
  name: string
  input: unknown
  output: string | null
  status: ToolStatus
  parallelIndex: { index: number; total: number } | null
  isError?: boolean
  /** Bot 子代理内部的嵌套工具调用 */
  children?: UIToolCall[]
  /** 事件来源标识 */
  source?: EventSource
  /** Bot 内部产生的文本内容 */
  botText?: string
  /** Bot 内部产生的思考内容 */
  botThinking?: string
}

export interface StreamingState {
  active: boolean
  assistantText: string
  thinking: string
  tools: UIToolCall[]
  blockOrder: ('thinking' | 'text' | `tool:${string}`)[]
  error: { code: string; message: string } | null
  retrying: {
    attempt: number
    maxRetries: number
    waitSeconds: number
    detail?: string
  } | null
  apiCalls: number
  tokenUsage: { input: number; output: number } | null
  abort: AbortController
}
```

注意：**移除了 `activeBotName`**。

Run: `cd web && npx tsc --noEmit`
Expected: 通过（类型定义阶段无编译错误）

---

## Task 6: 更新前端 chat store 解析逻辑

**Files:**
- Modify: `web/src/store/chat.ts`
- Test: `cd web && npx tsc --noEmit`

### Step 1: 更新 buildStreamingBlocks 的类型注解

`buildStreamingBlocks` 第 16-25 行的 `tools` 内联类型已过时，改用 `UIToolCall[]`：

```typescript
export function buildStreamingBlocks(
  st: {
    blockOrder: ('thinking' | 'text' | `tool:${string}`)[]
    thinking: string
    assistantText: string
    tools: UIToolCall[]
    error: { code: string; message: string } | null
  },
  finalized: boolean = false,
): UIBlock[] {
```

### Step 2: 重写 SSE 事件处理 switch 语句

将 `web/src/store/chat.ts:88-213` 的 switch 替换：

```typescript
        switch (evt.event) {
          case 'text_delta': {
            const src = evt.data.source
            if (src.role === 'main') {
              if (!target.assistantText && !target.blockOrder.includes('text')) {
                target.blockOrder.push('text')
              }
              target.assistantText += evt.data.content
            } else {
              // Bot 子代理的文本：归属到对应 call_id 的 call_bot 卡片
              let botCard = target.tools.find(
                (t) => t.name === 'call_bot' && t.source?.role === 'bot' && t.source.call_id === src.call_id
              )
              if (!botCard) {
                // text_delta 事件无 input 字段，用 source.name 构造最小 input
                botCard = {
                  id: nanoid(),
                  name: 'call_bot',
                  input: { name: src.name },
                  output: null,
                  status: 'running',
                  parallelIndex: null,
                  source: src,
                  children: [],
                  botText: '',
                  botThinking: '',
                }
                target.tools.push(botCard)
                target.blockOrder.push(`tool:${botCard.id}`)
              }
              botCard.botText = (botCard.botText || '') + evt.data.content
            }
            break
          }
          case 'thinking_delta': {
            const src = evt.data.source
            if (src.role === 'main') {
              if (!target.thinking && !target.blockOrder.includes('thinking')) {
                target.blockOrder.push('thinking')
              }
              target.thinking += evt.data.content
            } else {
              let botCard = target.tools.find(
                (t) => t.name === 'call_bot' && t.source?.role === 'bot' && t.source.call_id === src.call_id
              )
              if (!botCard) {
                // thinking_delta 事件无 input 字段，用 source.name 构造最小 input
                botCard = {
                  id: nanoid(),
                  name: 'call_bot',
                  input: { name: src.name },
                  output: null,
                  status: 'running',
                  parallelIndex: null,
                  source: src,
                  children: [],
                  botText: '',
                  botThinking: '',
                }
                target.tools.push(botCard)
                target.blockOrder.push(`tool:${botCard.id}`)
              }
              botCard.botThinking = (botCard.botThinking || '') + evt.data.content
            }
            break
          }
          case 'tool_call': {
            const tc = {
              id: nanoid(),
              name: evt.data.name,
              input: evt.data.input,
              output: null,
              status: 'running' as const,
              parallelIndex: evt.data.parallel_index ?? null,
              source: evt.data.source,
            }
            const src = evt.data.source
            if (src.role === 'bot' && evt.data.name !== 'call_bot') {
              // Bot 内部的工具调用：挂到对应 call_id 的 call_bot 下
              let parentTool = target.tools.find(
                (t) => t.name === 'call_bot' && t.source?.role === 'bot' && t.source.call_id === src.call_id
              )
              if (parentTool) {
                parentTool.children = parentTool.children || []
                parentTool.children.push(tc)
              } else {
                // 兜底：若 call_bot 卡片未创建，先创建
                parentTool = {
                  id: nanoid(),
                  name: 'call_bot',
                  input: evt.data.input,
                  output: null,
                  status: 'running',
                  parallelIndex: null,
                  source: src,
                  children: [tc],
                  botText: '',
                  botThinking: '',
                }
                target.tools.push(parentTool)
                target.blockOrder.push(`tool:${parentTool.id}`)
              }
            } else {
              target.tools.push(tc)
              target.blockOrder.push(`tool:${tc.id}`)
            }
            break
          }
          case 'tool_result': {
            const src = evt.data.source
            if (src.role === 'bot' && evt.data.name !== 'call_bot') {
              // Bot 内部的 tool_result：按 id 匹配，避免同名工具冲突
              let parentTool = target.tools.find(
                (t) => t.name === 'call_bot' && t.source?.role === 'bot' && t.source.call_id === src.call_id
              )
              if (parentTool && parentTool.children) {
                const childTc = parentTool.children.find(
                  (t) => t.id === evt.data.id && t.output === null
                )
                if (childTc) {
                  childTc.output = evt.data.output
                  childTc.status = 'done'
                }
              }
            } else {
              const tc = target.tools.find(
                (t) => t.name === evt.data.name && t.output === null
              )
              if (tc) {
                tc.output = evt.data.output
                tc.status = 'done'
              }
            }
            break
          }
          case 'turn_end':
            target.apiCalls = evt.data.api_calls
            if (evt.data.token_usage) {
              target.tokenUsage = {
                input: evt.data.token_usage.input_tokens,
                output: evt.data.token_usage.output_tokens,
              }
            }
            break
          case 'error':
            target.error = {
              code: evt.data.code,
              message: evt.data.message,
            }
            break
          case 'retrying':
            target.retrying = {
              attempt: evt.data.attempt,
              maxRetries: evt.data.max_retries,
              waitSeconds: evt.data.wait_seconds,
              detail: evt.data.detail,
            }
            break
          case 'done':
            target.active = false
            break
        }
```

### Step 3: 移除所有 activeBotName 引用

`activeBotName` 在 `web/src/store/chat.ts` 中出现于以下位置，全部移除：

- **Line 112**: `target.activeBotName = 'call_bot';` → 删除
- **Line 116**: `} else if (target.activeBotName) {` → 改为 `} else {`
- **Line 143**: `target.activeBotName = null;` → 删除
- **Line 156**: `} else if (target.activeBotName) {` → 改为 `} else {`
- **Line 545**: `activeBotName: null,`（StreamingState 初始化）→ 删除该行
- **Line 652**: `activeBotName: null,`（StreamingState 初始化）→ 删除该行

同时从 `web/src/types/ui.ts` 的 `StreamingState` 接口定义中移除 `activeBotName` 字段（已在 Task 5 中完成）。

Run: `cd web && npx tsc --noEmit`
Expected: 通过

---

## Task 7: 端到端编译验证

### Step 1: 后端编译

Run: `cargo check --all-targets`
Expected: 0 errors

### Step 2: 前端编译

Run: `cd web && npm run build`
Expected: 0 errors

### Step 3: 提交

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: SSE 事件来源标识

为 AgentEvent 添加 EventSource 类型，区分主 Agent 与 Bot 子代理事件来源。
- 后端：所有事件携带 source 字段，Bot 事件包含 call_id 用于并行区分
- SSE：序列化自动注入 source 到 JSON
- 前端：移除 activeBotName 推断，按 source.call_id 精确归属事件

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## 回滚策略

若发现线上问题：
1. `git revert HEAD`（回滚 commit）
2. 前端旧版本可安全忽略 `source` 字段（JSON 中多出字段不会报错）
3. 过渡期间，前端保留 `source` 字段缺失时的降级逻辑：若事件无 `source` 字段，默认按主 Agent 处理（这样回滚后端后前端仍可工作）
