# Core LLM Streaming 设计文档

## 1. 背景与现状

当前 `rust_agent_core` 的 LLM API 调用是**阻塞/非流式**的：

- `AnthropicClient` 和 `OpenAIClient` 均使用 `response.bytes().await` 读取完整 HTTP 响应体
- `AgentApp::handle_user_turn` 阻塞等待完整 `ProviderResponse` 返回后，一次性发送 `AgentEvent::TextDelta`（整段文本）
- Server SSE 通道里实际上只有一个大的 `text_delta` 事件，中间全靠 keep-alive 维持连接

这导致：
- **首 token 延迟高**：用户必须等模型生成完整回复后才能看到第一个字
- **SSE 名不副实**：虽然协议是 SSE，但体验上和普通 HTTP JSON 响应没有本质区别

## 2. 目标

将 Core 改造为**真流式**：对 Anthropic 和 OpenAI 同时启用 upstream streaming，边收 token 边通过 SSE 推送给客户端，实现打字机效果。

**约束**：
- 两个 Provider（Anthropic + OpenAI）同时支持
- 非流式场景（compact、命令处理等）保持向后兼容
- `AgentEvent` 的公开签名尽量稳定
- 流式中途失败不重试（已发出的 token 无法撤回）

## 3. 核心抽象

### 3.1 `LlmStreamChunk`（`api/types.rs`）

Provider 层把上游 SSE 解析为与 Provider 无关的统一数据块：

```rust
#[derive(Clone, Debug)]
pub enum LlmStreamChunk {
    /// 文本增量（真正的 token delta，不是完整文本）
    TextDelta(String),

    /// 工具调用开始（收到 tool name 和 id）
    ToolUseStart {
        id: String,
        name: String,
    },

    /// 工具调用参数增量（JSON 片段）
    ToolUseDelta {
        id: String,
        input_json_delta: String,
    },

    /// 工具调用结束（参数已完整）
    ToolUseEnd {
        id: String,
    },

    /// Token 用量（通常在流末尾出现）
    Usage(TokenUsage),

    /// 流正常结束
    Done,
}
```

### 3.2 Provider 接口扩展（`api/mod.rs`）

```rust
use futures::stream::BoxStream;

impl LlmProvider {
    /// 原有阻塞 API，保留用于 compact / 命令处理等非流式场景
    pub async fn create_message(
        &self,
        request: &ProviderRequest<'_>,
        retry_notifier: Option<&RetryNotifier>,
        cancel: Option<&CancelFlag>,
    ) -> AgentResult<ProviderResponse>;

    /// 新增流式 API
    pub async fn stream_message(
        &self,
        request: &ProviderRequest<'_>,
        retry_notifier: Option<&RetryNotifier>,
        cancel: Option<&CancelFlag>,
    ) -> AgentResult<BoxStream<'static, AgentResult<LlmStreamChunk>>> {
        match self {
            Self::Anthropic(c) => c.stream_message(request, retry_notifier, cancel).await,
            Self::OpenAI(c) => c.stream_message(request, retry_notifier, cancel).await,
        }
    }
}
```

**设计点**：
- `stream_message` 返回 `AgentResult<BoxStream<...>>`：外层 `Result` 表示"连接是否成功建立"，内层 `Result` 表示"流中的某个 chunk 是否解析出错"
- 旧 `create_message` 完全不动，非 SSE 场景零影响

## 4. Provider 层流式解析

### 4.1 Anthropic

请求体增加 `"stream": true`。响应为 SSE，事件类型：

| SSE 事件 | 映射到 `LlmStreamChunk` |
|---------|------------------------|
| `message_start` | 忽略 |
| `content_block_start` (type=text) | 忽略 |
| `content_block_start` (type=tool_use) | `ToolUseStart { id, name }` |
| `content_block_delta` (type=text_delta) | `TextDelta(text)` |
| `content_block_delta` (type=input_json_delta) | `ToolUseDelta { id, input_json_delta }` |
| `content_block_stop` (text block) | 忽略 |
| `content_block_stop` (tool_use block) | `ToolUseEnd { id }` |
| `message_delta` | 提取 `stop_reason` 和 `usage`，不发 chunk |
| `message_stop` | `Done` |

解析器维护一个 `current_tool: Option<ToolUseBuilder>` 状态，在 `content_block_delta/input_json_delta` 时累积 `partial_json`。

SSE 解析实现：使用 `reqwest::Response::bytes_stream()` 获取 byte stream，再用自研的 SSE line parser（按 `\n\n` 分割，提取 `event:` 和 `data:` 行）解析为事件。不引入额外的 `eventsource-stream` 依赖，保持依赖树精简。

### 4.2 OpenAI

请求体增加 `"stream": true`，可选 `"stream_options": {"include_usage": true}`。

OpenAI 的 stream chunk 格式：

```json
{"choices":[{"delta":{"content":"Hello"},"index":0}]}
{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file"}}]}},"index":0}]}
{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":\"x\"}"}}]}},"index":0}]}
{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}
```

解析器按 `tool_calls[].index` 维护 `HashMap<usize, ToolUseBuilder>`。

SSE 解析实现：与 Anthropic 相同，使用 `bytes_stream()` + 自研 SSE line parser。

- `delta.tool_calls[].id` 出现 → 新工具开始 → `ToolUseStart`
- `delta.tool_calls[].function.arguments` 出现 → 参数增量 → `ToolUseDelta`
- `finish_reason: "tool_calls"` 出现 → 所有已累积工具发 `ToolUseEnd`
- `finish_reason: "stop"` → 无工具，stream 结束
- `usage` 出现 → `Usage(TokenUsage)`
- `[DONE]` → `Done`

### 4.3 OpenAI 兼容后端兜底

| 问题 | 兜底策略 |
|------|---------|
| 后端不支持 `stream: true`，返回完整 JSON | `stream_message` 检测到响应 Content-Type 不是 `text/event-stream` 时，fallback 到阻塞解析，然后将完整文本作为单个 `TextDelta` + `Done` 发出 |
| 后端 stream 不返回 `usage` | `stream_usage` 保持 `TokenUsage::default()` |
| 后端 `tool_calls` 一次性返回完整参数（非 delta） | `ToolUseDelta` 接收整个 JSON 字符串作为增量，`ToolUseEnd` 由 `finish_reason` 触发 |
| 后端 `finish_reason` 为 null | 视为 `"end_turn"` |
| SSE 格式差异（如多余空行、`[DONE]` 后还有数据） | SSE 解析器忽略空行，以 `data: ` 前缀提取 JSON，遇到 `[DONE]` 后忽略后续非错误行 |

### 4.4 重试策略

流式模式下**仅连接建立阶段可重试**：
- `send().await` 失败（超时、连接错误）→ 指数退避重试
- HTTP 429/5xx → 重试
- **一旦 `response.bytes_stream()` 开始消费，中间失败不重试**，直接 propagate error

原因：已发出的 token 无法撤回，重试会导致客户端看到重复文本。

## 5. Agent 层改造

### 5.1 `AgentEvent` 微调

给 `ToolCall` 和 `ToolResult` 增加 `id` 字段，用于流式模式下去重/关联：

```rust
pub enum AgentEvent {
    TextDelta(String),
    ToolCall {
        id: Option<String>,        // 新增
        name: String,
        input: serde_json::Value,
        parallel_index: Option<(usize, usize)>,
    },
    ToolResult {
        id: Option<String>,        // 新增
        name: String,
        output: String,
        parallel_index: Option<(usize, usize)>,
    },
    TurnEnd { api_calls: usize, token_usage: Option<TokenUsage> },
    Done,
    Error { code: String, message: String },
    Retrying { attempt: u32, max_retries: u32, wait_seconds: u64, detail: String },
}
```

**适配**：所有现有 `AgentEvent::ToolCall { name, input, parallel_index }` 构造点改为 `ToolCall { id: None, name, input, parallel_index }`。

### 5.2 `run_agent_loop` 核心改造

当前阻塞调用（约第 500-566 行）：

```rust
let response = self.client.create_message(&request, ...).await?;
api_call_count += 1;
let stop_reason = response.stop_reason.clone();
ctx.push(ApiMessage::assistant_blocks(&response.content)?);
if config.emit_events {
    let text = response.final_text();
    if !text.is_empty() { event_tx.send(AgentEvent::TextDelta(text)).await; }
}
```

改造后：

```rust
// 1. 发起流式请求
let stream = match self.client.stream_message(&request, Some(&retry_tx), Some(&cancelled)).await {
    Ok(s) => s,
    Err(e) => {
        drop(retry_tx);
        let code = if e.downcast_ref::<crate::api::error::LlmApiError>().map(|e| e.is_rate_limited()).unwrap_or(false) {
            "rate_limited"
        } else {
            "llm_api_error"
        };
        if config.emit_events {
            let _ = event_tx.send(AgentEvent::Error { code: code.to_owned(), message: format!("{e:#}") }).await;
        }
        return Err(e);
    }
};

api_call_count += 1;

// 2. 流式遍历，实时转发
let mut current_text = String::new();
let mut tool_accumulators: Vec<ToolUseAccumulator> = Vec::new();  // ToolUseAccumulator { id, name, input_json }
let mut stop_reason = String::new();
let mut stream_usage = TokenUsage::default();

use futures::StreamExt;
let mut stream = stream;

while let Some(chunk_result) = stream.next().await {
    // 客户端断开检查
    if event_tx.is_closed() {
        warn!("[Agent] 客户端已断开，终止 stream");
        return Ok("客户端已断开连接".to_owned());
    }

    let chunk = match chunk_result {
        Ok(c) => c,
        Err(e) => {
            error!("[Agent] stream 中断: {e:#}");
            if config.emit_events {
                let _ = event_tx.send(AgentEvent::Error {
                    code: "stream_error".to_owned(),
                    message: format!("流式响应中断: {e:#}"),
                }).await;
            }
            return Err(e);
        }
    };

    match chunk {
        LlmStreamChunk::TextDelta(text) => {
            current_text.push_str(&text);
            if config.emit_events && !text.is_empty() {
                let _ = event_tx.send(AgentEvent::TextDelta(text)).await;
            }
        }
        LlmStreamChunk::ToolUseStart { id, name } => {
            tool_accumulators.push(ToolUseAccumulator { id: id.clone(), name: name.clone(), input_json: String::new() });
            if config.emit_events {
                let _ = event_tx.send(AgentEvent::ToolCall {
                    id: Some(id), name, input: serde_json::json!({}), parallel_index: None,
                }).await;
            }
        }
        LlmStreamChunk::ToolUseDelta { id, input_json_delta } => {
            if let Some(tool) = tool_accumulators.iter_mut().find(|t| t.id == id) {
                tool.input_json.push_str(&input_json_delta);
            }
        }
        LlmStreamChunk::ToolUseEnd { id } => {
            if config.emit_events {
                if let Some(tool) = tool_accumulators.iter().find(|t| t.id == id) {
                    let input = serde_json::from_str(&tool.input_json)
                        .unwrap_or_else(|_| serde_json::Value::String(tool.input_json.clone()));
                    let _ = event_tx.send(AgentEvent::ToolCall {
                        id: Some(tool.id.clone()),
                        name: tool.name.clone(),
                        input,
                        parallel_index: None,
                    }).await;
                }
            }
        }
        LlmStreamChunk::Usage(usage) => { stream_usage = usage; }
        LlmStreamChunk::Done => {}
    }
}

// 3. stream 结束后组装
// stop_reason 的提取：
// - Anthropic: 从 message_delta 事件中提取，映射 "tool_use" -> "tool_calls"
// - OpenAI: 从最后一个 chunk 的 finish_reason 提取
drop(retry_tx);
self.token_tracker.record(&self.model, &stream_usage);

let mut assistant_blocks: Vec<ResponseContentBlock> = Vec::new();
if !current_text.is_empty() {
    assistant_blocks.push(ResponseContentBlock::Text { text: current_text.clone() });
}
for tool in &tool_accumulators {
    let input = serde_json::from_str(&tool.input_json)
        .unwrap_or_else(|_| serde_json::Value::String(tool.input_json.clone()));
    assistant_blocks.push(ResponseContentBlock::ToolUse {
        id: tool.id.clone(), name: tool.name.clone(), input,
    });
}
ctx.push(ApiMessage::assistant_blocks(&assistant_blocks)?);
```

### 5.3 后续逻辑（stop_reason 判定）

```rust
if stop_reason != "tool_calls" {
    let text = current_text;
    let text = if text.trim().is_empty() {
        "（本轮未生成可见回复，但已执行相关工具操作）".to_owned()
    } else { text };
    if config.emit_events {
        let _ = event_tx.send(AgentEvent::TurnEnd {
            api_calls: api_call_count,
            token_usage: Some(self.token_tracker.snapshot().total),
        }).await;
    }
    return Ok(text);
}

// stop_reason == "tool_calls"
// 提取 tool_calls、分类、dispatch、发送 ToolResult -> 逻辑与当前完全一致
```

**关键变化总结**：
- `AgentEvent::TextDelta` 语义从"完整文本"变为"token delta"，所有 SSE 消费者自动获得打字机效果
- 上下文组装从 `response.content` 改为 `current_text + tool_accumulators`
- `token_tracker` 从 `resp.usage` 改为 `stream_usage`
- ToolCall 在 `ToolUseStart` 时即发（input 为空），在 `ToolUseEnd` 时更新为完整参数

## 6. Server / 消费者层适配

### 6.1 `sse.rs`

`ToolCall` 和 `ToolResult` 的 JSON 增加 `id` 字段：

```rust
AgentEvent::ToolCall { id, name, input, parallel_index } => {
    let mut data = json!({ "name": name, "input": input });
    if let Some(id) = id { data["id"] = json!(id); }
    if let Some((idx, total)) = parallel_index { data["parallel_index"] = json!({ "index": idx, "total": total }); }
    Event::default().event("tool_call").data(data.to_string())
}
AgentEvent::ToolResult { id, name, output, parallel_index } => {
    let mut data = json!({ "name": name, "output": output });
    if let Some(id) = id { data["id"] = json!(id); }
    if let Some((idx, total)) = parallel_index { data["parallel_index"] = json!({ "index": idx, "total": total }); }
    Event::default().event("tool_result").data(data.to_string())
}
```

### 6.2 `openai_compat.rs`

解构 `ToolCall` 时加 `id` 字段：

```rust
rust_agent_core::agent::AgentEvent::ToolCall { id, name, input, parallel_index: _ } => {
    tool_calls_collected.push(json!({
        "id": id.unwrap_or_else(|| format!("call_{}", short_id())),
        "type": "function",
        "function": { "name": name, "arguments": input.to_string() }
    }));
}
```

## 7. 错误处理与边界情况

| 场景 | 处理 |
|------|------|
| 流式中途网络断开/解析错误 | 已发出的 token 保留，Agent 层返回 `stream_error`，不发 `TurnEnd` |
| 后端不支持 streaming | Fallback 到阻塞解析，完整文本作为单个 `TextDelta` 发出 |
| 后端不返回 `usage` | `stream_usage` 为 0，不影响功能 |
| tool_use 参数 JSON 解析失败 | 回退为原始字符串，`dispatch` 层自然报错 |
| 客户端 SSE 断开 | `event_tx.is_closed()` 检测后立即 `drop(stream)`，节约 API 配额 |
| 流式连接建立失败 | 按现有指数退避重试 |

## 8. 测试策略

### 8.1 Provider 解析器单元测试

- Anthropic parser：覆盖 text-only、tool-use、mixed 场景
- OpenAI parser：覆盖 `delta.content`、`delta.tool_calls`、多 index tool_calls
- 边界：空 stream、畸形 JSON、缺失 `finish_reason`

### 8.2 Agent 层 Mock 测试

用 `tokio_stream::iter` 构造 mock `LlmStreamChunk` stream，注入到 `run_agent_loop`，断言：
- `AgentEvent` 的**顺序**正确：`TextDelta` -> `ToolCall`(begin) -> `ToolCall`(end) -> `ToolResult` -> `TurnEnd`
- 上下文组装正确：`current_text + tool_accumulators` 等价于旧 `response.content`
- `token_tracker` 正确累加

### 8.3 集成测试

- Mock HTTP server 返回预设 SSE stream
- 端到端调用 `handle_user_turn`，验证 SSE 输出

### 8.4 兼容性测试

针对常见 OpenAI 兼容后端准备样本 SSE：Ollama、DeepSeek、vLLM。

## 9. 影响范围

| 文件 | 改动类型 |
|------|---------|
| `crates/core/src/api/types.rs` | 新增 `LlmStreamChunk` |
| `crates/core/src/api/mod.rs` | 新增 `stream_message()` |
| `crates/core/src/api/anthropic.rs` | 新增 `stream_message()` + SSE 解析器 |
| `crates/core/src/api/openai.rs` | 同上 + 兼容兜底 |
| `crates/core/src/agent.rs` | `run_agent_loop` 核心循环改造 + `AgentEvent` 加 `id` |
| `crates/server/src/sse.rs` | `ToolCall`/`ToolResult` 映射加 `id` |
| `crates/server/src/openai_compat.rs` | `ToolCall` 解构加 `id` |
| `crates/a2a/src/executor.rs` | 如有 `ToolCall` 解构，加 `id` |

**非流式场景（compact、命令处理）完全不受影响**，继续使用 `create_message`。
