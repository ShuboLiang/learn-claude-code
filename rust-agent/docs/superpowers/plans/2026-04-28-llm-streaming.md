# LLM 流式响应改造 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 LLM API 调用从非流式改为流式，实现文本逐 token 实时渲染到 CLI。

**Architecture:** 在 API Provider 层新增 `create_message_stream` 方法，返回 `tokio::sync::mpsc::Receiver<StreamChunk>`。`run_agent_loop` 消费 Receiver，边收边发 `AgentEvent::TextDelta`。OpenAI 和 Anthropic 分别实现 SSE 解析，工具调用参数在流结束后聚合。CLI 层无需改动。

**Tech Stack:** Rust, reqwest (stream feature), tokio (mpsc), SSE

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/core/Cargo.toml` | Modify | 添加 `futures = "0.3"`，给 `reqwest` 加 `stream` feature |
| `crates/core/src/api/types.rs` | Modify | 新增 `StreamChunk` 枚举 |
| `crates/core/src/api/streaming.rs` | Create | 通用 SSE 解析逻辑 (`extract_sse_events`) |
| `crates/core/src/api/openai.rs` | Modify | 新增流式 HTTP 请求和 SSE 解析 |
| `crates/core/src/api/anthropic.rs` | Modify | 新增流式 HTTP 请求和 SSE 解析 |
| `crates/core/src/api/mod.rs` | Modify | `LlmProvider` 新增 `create_message_stream` 统一入口 |
| `crates/core/src/agent.rs` | Modify | `run_agent_loop` 改用 `create_message_stream`，实时发 `TextDelta` |

---

### Task 1: 添加依赖

**Files:**
- Modify: `crates/core/Cargo.toml`

- [ ] **Step 1: 添加 `futures` crate 和 reqwest `stream` feature**

在 `crates/core/Cargo.toml` 中，将 `reqwest` 行改为：

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }
```

并在 `[dependencies]` 任意位置插入：

```toml
futures = "0.3"
```

- [ ] **Step 2: 编译验证依赖下载**

Run: `cargo check --package rust-agent-core 2>&1 | head -20`

Expected: 正常下载依赖，无编译错误（可能有既有的 dead_code warning）

- [ ] **Step 3: Commit**

```bash
git add crates/core/Cargo.toml
git commit -m "deps(core): add futures and reqwest stream feature for LLM streaming"
```

---

### Task 2: 定义 StreamChunk 类型

**Files:**
- Modify: `crates/core/src/api/types.rs`

- [ ] **Step 1: 在文件末尾追加 `StreamChunk` 枚举**

在 `crates/core/src/api/types.rs` 第 187 行之后追加：

```rust
/// LLM 流式响应的单个 chunk
#[derive(Clone, Debug)]
pub enum StreamChunk {
    /// 文本增量（逐 token）
    TextDelta(String),
    /// 工具调用开始
    ToolUseStart { id: String, name: String },
    /// 工具调用参数增量（JSON 片段，需聚合后解析）
    ToolUseInput { id: String, input_json: String },
    /// Token 用量（通常在流末尾）
    Usage(TokenUsage),
    /// 停止原因（流结束时）
    Stop(String),
}
```

- [ ] **Step 2: 编译验证类型定义**

Run: `cargo check --package rust-agent-core 2>&1 | grep -E "^error"`

Expected: 无错误输出

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/api/types.rs
git commit -m "feat(api): add StreamChunk enum for streaming LLM responses"
```

---

### Task 3: 编写通用 SSE 解析模块

**Files:**
- Create: `crates/core/src/api/streaming.rs`
- Modify: `crates/core/src/api/mod.rs`

- [ ] **Step 1: 创建 SSE 解析模块**

Create `crates/core/src/api/streaming.rs`:

```rust
//! 通用 SSE (Server-Sent Events) 解析工具

/// 单个 SSE 事件
#[derive(Clone, Debug)]
pub(crate) struct SseEvent {
    pub event: String,
    pub data: String,
}

/// 从缓冲区中提取所有完整的 SSE 事件（按 `\n\n` 分割）
/// 提取后的事件从 buffer 中移除，剩余不完整的片段保留在 buffer 中
pub(crate) fn extract_sse_events(buffer: &mut String) -> Vec<SseEvent> {
    let mut events = Vec::new();
    while let Some(pos) = buffer.find("\n\n") {
        let block: String = buffer.drain(..pos + 2).collect();
        let mut event_name = String::new();
        let mut data_lines = Vec::new();
        for line in block.lines() {
            if line.starts_with("event:") {
                event_name = line[6..].trim().to_owned();
            } else if line.starts_with("data:") {
                data_lines.push(line[5..].trim());
            }
        }
        if !data_lines.is_empty() {
            events.push(SseEvent {
                event: event_name,
                data: data_lines.join("\n"),
            });
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_event() {
        let mut buf = "event: content_block_delta\ndata: {\"text\":\"Hello\"}\n\n".to_owned();
        let events = extract_sse_events(&mut buf);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "content_block_delta");
        assert_eq!(events[0].data, "{\"text\":\"Hello\"}");
        assert!(buf.is_empty());
    }

    #[test]
    fn test_extract_partial_buffer() {
        let mut buf = "event: ping\ndata: {}\n\nevent: content".to_owned();
        let events = extract_sse_events(&mut buf);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "ping");
        assert_eq!(buf, "event: content");
    }
}
```

- [ ] **Step 2: 在 `api/mod.rs` 中声明新模块**

在 `crates/core/src/api/mod.rs` 现有 `mod` 声明区域添加：

```rust
mod streaming;
```

- [ ] **Step 3: 运行 SSE 解析单元测试**

Run: `cargo test --package rust-agent-core streaming::tests -- --nocapture`

Expected: `test_extract_single_event ... ok` 和 `test_extract_partial_buffer ... ok`

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/api/streaming.rs crates/core/src/api/mod.rs
git commit -m "feat(api): add generic SSE parser for streaming responses"
```

---

### Task 4: OpenAI Provider 流式实现

**Files:**
- Modify: `crates/core/src/api/openai.rs`

- [ ] **Step 1: 给 `OpenAIRequest` 添加 `stream` 字段和 `Clone`**

修改 `crates/core/src/api/openai.rs` 第 27-34 行：

```rust
#[derive(Serialize, Clone)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAIToolOwned>>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}
```

- [ ] **Step 2: 添加流式响应专用类型**

在 `OpenAIClient` 类型定义之前（约第 120 行附近），插入：

```rust
/// OpenAI 流式响应 chunk
#[derive(Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAIStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIStreamToolCall>>,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Deserialize, Clone)]
struct OpenAIStreamToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAIStreamFunction>,
}

#[derive(Deserialize, Clone)]
struct OpenAIStreamFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
```

- [ ] **Step 3: 实现 `call_api_stream` 方法**

在 `impl OpenAIClient` 中 `call_api` 方法之后（约第 280 行），添加：

```rust
    /// 调用 OpenAI Chat Completions API（流式）
    async fn call_api_stream(
        &self,
        request: &OpenAIRequest,
    ) -> AgentResult<tokio::sync::mpsc::Receiver<AgentResult<super::types::StreamChunk>>> {
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let mut req = request.clone();
        req.stream = true;

        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header("X-Client-Name", "claude-code")
            .header(USER_AGENT, "claude-code/1.0")
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("调用 OpenAI 流式 API 失败: {}", retry::format_reqwest_error(&e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI 流式 API 错误 {status}: {body}"));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<AgentResult<super::types::StreamChunk>>(64);

        tokio::spawn(async move {
            use futures::StreamExt;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow!("读取流失败: {}", retry::format_reqwest_error(&e)))).await;
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                let events = streaming::extract_sse_events(&mut buffer);
                for event in events {
                    if event.data == "[DONE]" {
                        return;
                    }
                    let delta: OpenAIStreamChunk = match serde_json::from_str(&event.data) {
                        Ok(d) => d,
                        Err(e) => {
                            let _ = tx.send(Err(anyhow!("解析 SSE chunk 失败: {e}"))).await;
                            continue;
                        }
                    };

                    if let Some(choice) = delta.choices.first() {
                        if let Some(content) = &choice.delta.content {
                            if !content.is_empty() {
                                let _ = tx.send(Ok(super::types::StreamChunk::TextDelta(content.clone()))).await;
                            }
                        }
                        if let Some(tool_calls) = &choice.delta.tool_calls {
                            for tc in tool_calls {
                                if let Some(id) = &tc.id {
                                    let name = tc.function.as_ref().and_then(|f| f.name.clone()).unwrap_or_default();
                                    let _ = tx.send(Ok(super::types::StreamChunk::ToolUseStart {
                                        id: id.clone(),
                                        name,
                                    })).await;
                                }
                                if let Some(args) = tc.function.as_ref().and_then(|f| f.arguments.clone()) {
                                    let id = tc.id.clone().unwrap_or_else(|| format!("tc_{}", tc.index));
                                    let _ = tx.send(Ok(super::types::StreamChunk::ToolUseInput {
                                        id,
                                        input_json: args,
                                    })).await;
                                }
                            }
                        }
                        if let Some(reason) = &choice.finish_reason {
                            let _ = tx.send(Ok(super::types::StreamChunk::Stop(reason.clone()))).await;
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
```

- [ ] **Step 4: 实现 `create_message_stream` 公开方法**

在 `impl OpenAIClient` 的 `create_message` 方法之后（约第 300 行），添加：

```rust
    /// 发送消息并以流式方式获取回复
    pub async fn create_message_stream(
        &self,
        request: &ProviderRequest<'_>,
    ) -> AgentResult<tokio::sync::mpsc::Receiver<AgentResult<super::types::StreamChunk>>> {
        let openai_request = OpenAIRequest {
            model: request.model.to_owned(),
            messages: convert_messages(request.system, request.messages),
            tools: convert_tools(request.tools),
            max_tokens: request.max_tokens,
            stream: true,
        };
        self.call_api_stream(&openai_request).await
    }
```

- [ ] **Step 5: 添加 `use futures::StreamExt;` 到文件顶部**

在 `crates/core/src/api/openai.rs` 的 `use` 语句区域添加：

```rust
use futures::StreamExt;
```

- [ ] **Step 6: 编译验证**

Run: `cargo check --package rust-agent-core 2>&1 | grep -E "^error"`

Expected: 无错误输出

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/api/openai.rs
git commit -m "feat(openai): implement streaming response with SSE parsing"
```


---

### Task 5: Anthropic Provider 流式实现

**Files:**
- Modify: `crates/core/src/api/anthropic.rs`
- Modify: `crates/core/src/api/types.rs`

- [ ] **Step 1: 给 `MessagesRequest` 添加 `stream` 字段**

修改 `crates/core/src/api/types.rs` 第 54-66 行的 `MessagesRequest`：

```rust
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MessagesRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub messages: &'a [ApiMessage],
    pub tools: &'a [Value],
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
}
```

- [ ] **Step 2: 实现 `create_message_stream` 方法**

在 `AnthropicClient` 的 `create_message` 方法之后（约第 180 行），添加：

```rust
    /// 调用 Claude Messages API（流式），返回逐 chunk 的 Receiver
    pub async fn create_message_stream(
        &self,
        request: &MessagesRequest<'_>,
    ) -> AgentResult<tokio::sync::mpsc::Receiver<AgentResult<super::types::StreamChunk>>> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));

        // 构造带 stream=true 的 owned 请求体（避免生命周期问题）
        let body = serde_json::json!({
            "model": request.model,
            "system": request.system,
            "messages": request.messages,
            "tools": request.tools,
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        let response = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("调用 Anthropic 流式 API 失败: {}", retry::format_reqwest_error(&e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Anthropic 流式 API 错误 {status}: {body}"));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<AgentResult<super::types::StreamChunk>>(64);

        tokio::spawn(async move {
            use futures::StreamExt;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow!("读取流失败: {}", retry::format_reqwest_error(&e)))).await;
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                let events = streaming::extract_sse_events(&mut buffer);
                for event in events {
                    if event.data.trim().is_empty() {
                        continue;
                    }
                    let json: serde_json::Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = tx.send(Err(anyhow!("解析 SSE JSON 失败: {e}"))).await;
                            continue;
                        }
                    };

                    match event.event.as_str() {
                        "content_block_delta" => {
                            if let Some(delta) = json.get("delta") {
                                if let Some(text) = delta.get("text_delta").and_then(|t| t.as_str()) {
                                    let _ = tx.send(Ok(super::types::StreamChunk::TextDelta(text.to_owned()))).await;
                                }
                                if let Some(partial) = delta.get("input_json_delta").and_then(|p| p.get("partial_json")).and_then(|p| p.as_str()) {
                                    if let Some(index) = json.get("index").and_then(|i| i.as_u64()) {
                                        let id = format!("tool_use_{}", index);
                                        let _ = tx.send(Ok(super::types::StreamChunk::ToolUseInput {
                                            id,
                                            input_json: partial.to_owned(),
                                        })).await;
                                    }
                                }
                            }
                        }
                        "content_block_start" => {
                            if let Some(block) = json.get("content_block") {
                                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("").to_owned();
                                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("").to_owned();
                                    let _ = tx.send(Ok(super::types::StreamChunk::ToolUseStart { id, name })).await;
                                }
                            }
                        }
                        "message_delta" => {
                            if let Some(delta) = json.get("delta") {
                                if let Some(reason) = delta.get("stop_reason").and_then(|r| r.as_str()) {
                                    let _ = tx.send(Ok(super::types::StreamChunk::Stop(reason.to_owned()))).await;
                                }
                            }
                            if let Some(usage) = json.get("usage") {
                                if let Ok(u) = serde_json::from_value::<AnthropicUsage>(usage.clone()) {
                                    let tu = TokenUsage {
                                        input_tokens: u.input_tokens,
                                        output_tokens: u.output_tokens,
                                        cache_read_tokens: u.cache_read_input_tokens,
                                        cache_creation_tokens: u.cache_creation_input_tokens,
                                    };
                                    let _ = tx.send(Ok(super::types::StreamChunk::Usage(tu))).await;
                                }
                            }
                        }
                        "message_stop" | "ping" => {}
                        _ => {}
                    }
                }
            }
        });

        Ok(rx)
    }
```

- [ ] **Step 3: 添加 `use futures::StreamExt;` 到文件顶部**

在 `crates/core/src/api/anthropic.rs` 的 `use` 区域添加：

```rust
use futures::StreamExt;
```

- [ ] **Step 4: 编译验证**

Run: `cargo check --package rust-agent-core 2>&1 | grep -E "^error"`

Expected: 无错误输出

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/api/anthropic.rs crates/core/src/api/types.rs
git commit -m "feat(anthropic): implement streaming response with SSE parsing"
```

---

### Task 6: LlmProvider 统一流式接口

**Files:**
- Modify: `crates/core/src/api/mod.rs`

- [ ] **Step 1: 在 `LlmProvider` 中新增 `create_message_stream` 方法**

修改 `crates/core/src/api/mod.rs` 中 `impl LlmProvider`（约第 23-33 行），在 `create_message` 之后添加：

```rust
    /// 发送消息并以流式方式获取逐 chunk 响应
    pub async fn create_message_stream(
        &self,
        request: &ProviderRequest<'_>,
    ) -> AgentResult<tokio::sync::mpsc::Receiver<AgentResult<StreamChunk>>> {
        match self {
            LlmProvider::Anthropic(client) => client.create_message_stream(request).await,
            LlmProvider::OpenAI(client) => client.create_message_stream(request).await,
        }
    }
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --package rust-agent-core 2>&1 | grep -E "^error"`

Expected: 无错误输出

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/api/mod.rs
git commit -m "feat(api): expose unified create_message_stream on LlmProvider"
```

---

### Task 7: 改造 run_agent_loop 消费流式响应

**Files:**
- Modify: `crates/core/src/agent.rs`

- [ ] **Step 1: 替换 LLM 调用为流式接口**

定位 `crates/core/src/agent.rs` 约第 378-414 行，将原 `create_message` 调用及后续处理替换为：

```rust
            // 流式调用 LLM
            let mut rx = match self.client.create_message_stream(&request).await {
                Ok(rx) => rx,
                Err(e) => {
                    eprintln!("[Agent] create_message_stream 失败！错误: {e:#}");
                    if config.emit_events {
                        let _ = event_tx
                            .send(AgentEvent::Error {
                                code: "llm_api_error".to_owned(),
                                message: format!("{e:#}"),
                            })
                            .await;
                    }
                    return Err(e);
                }
            };

            let mut accumulated_text = String::new();
            let mut tool_calls_builders: Vec<(String, String, String)> = Vec::new(); // (id, name, input_json)
            let mut stop_reason = String::new();
            let mut usage = crate::api::types::TokenUsage::default();

            while let Some(chunk_result) = rx.recv().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[Agent] 流 chunk 解析失败: {e:#}");
                        continue;
                    }
                };

                match chunk {
                    crate::api::types::StreamChunk::TextDelta(text) => {
                        accumulated_text.push_str(&text);
                        if config.emit_events {
                            let _ = event_tx.send(AgentEvent::TextDelta(text)).await;
                        }
                    }
                    crate::api::types::StreamChunk::ToolUseStart { id, name } => {
                        tool_calls_builders.push((id, name, String::new()));
                    }
                    crate::api::types::StreamChunk::ToolUseInput { id, input_json } => {
                        if let Some(builder) = tool_calls_builders.iter_mut().find(|(tid, _, _)| tid == &id) {
                            builder.2.push_str(&input_json);
                        }
                    }
                    crate::api::types::StreamChunk::Usage(u) => usage = u,
                    crate::api::types::StreamChunk::Stop(reason) => stop_reason = reason,
                }
            }

            api_call_count += 1;

            // 聚合工具调用参数为 JSON Value
            let tool_call_blocks: Vec<ResponseContentBlock> = tool_calls_builders
                .into_iter()
                .map(|(id, name, input_json)| {
                    let input = serde_json::from_str(&input_json).unwrap_or(serde_json::Value::Null);
                    ResponseContentBlock::ToolUse { id, name, input }
                })
                .collect();

            let mut content_blocks: Vec<ResponseContentBlock> = Vec::new();
            if !accumulated_text.is_empty() {
                content_blocks.push(ResponseContentBlock::Text { text: accumulated_text.clone() });
            }
            content_blocks.extend(tool_call_blocks);

            let response = ProviderResponse {
                content: content_blocks,
                stop_reason: stop_reason.clone(),
                usage,
            };

            ctx.push(ApiMessage::assistant_blocks(&response.content)?);

            if stop_reason != "tool_calls" && !stop_reason.is_empty() {
                let text = if accumulated_text.trim().is_empty() {
                    "（本轮未生成可见回复，但已执行相关工具操作）".to_owned()
                } else {
                    accumulated_text
                };
                if config.emit_events {
                    let _ = event_tx
                        .send(AgentEvent::TurnEnd {
                            api_calls: api_call_count,
                            token_usage: Some(self.token_tracker.snapshot().total),
                        })
                        .await;
                }
                return Ok(text);
            }
```

注意：
- `TextDelta` 在 `while` 循环中**逐 chunk 实时发送**给 CLI
- 流结束后用 `tool_calls_builders` 聚合工具调用参数
- 手动构造 `ProviderResponse` 推入上下文（供后续工具调用解析使用）
- `stop_reason` 为空时继续执行（某些 provider 可能不发送 Stop chunk）

- [ ] **Step 2: 编译验证完整项目**

Run: `cargo check --package rust-agent-core 2>&1 | grep -E "^error"`

Expected: 无错误输出

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/agent.rs
git commit -m "feat(agent): consume streaming LLM responses in run_agent_loop"
```

---

### Task 8: 全量编译与测试

**Files:**
- 无新文件

- [ ] **Step 1: 全量编译**

Run: `cargo build --package rust-agent-core 2>&1 | tail -10`

Expected: `Finished dev profile` 且无错误

- [ ] **Step 2: 运行单元测试**

Run: `cargo test --package rust-agent-core 2>&1 | tail -20`

Expected: 所有现有测试通过，新增 SSE 解析测试通过

- [ ] **Step 3: Commit**

```bash
git commit --allow-empty -m "test: verify streaming build passes all tests"
```

---

### Task 9: 端到端验证（手动）

**Files:**
- 无代码改动

- [ ] **Step 1: 启动 server**

Run: `cargo run --package rust-agent-server 2>&1`

Expected: Server 启动，监听默认端口

- [ ] **Step 2: CLI 发送测试消息**

在另一个终端启动 CLI，发送一条消息。

Expected: 文本逐字出现在 CLI 中，不再"蹦"出来

- [ ] **Step 3: Commit 验证记录**

```bash
git commit --allow-empty -m "chore: verify streaming E2E with CLI"
```

---

## Self-Review

### 1. Spec coverage

| 需求 | 对应 Task |
|------|-----------|
| LLM API 从非流式改为流式 | Task 4, 5, 6 |
| 文本逐 token 实时渲染到 CLI | Task 7 (`TextDelta` 在流中实时发送) |
| OpenAI 兼容 API 支持 | Task 4 |
| Anthropic Claude API 支持 | Task 5 |
| 工具调用参数聚合 | Task 7 (`tool_calls_builders`) |
| CLI 无需改动 | 架构设计说明 |
| 编译通过 + 测试通过 | Task 8 |

**Gap:** 无。

### 2. Placeholder scan

- 无 "TBD" / "TODO" / "implement later"
- 无 "Add appropriate error handling"
- 所有步骤包含实际代码和命令
- 无 "Similar to Task N"

### 3. Type consistency

- `StreamChunk` 定义在 Task 2，在 Task 4/5/6/7 中一致使用 `super::types::StreamChunk`
- `TokenUsage` 在 `StreamChunk::Usage` 和 `ProviderResponse` 中类型一致
- `tool_calls_builders` 使用 `(String, String, String)` 三元组，在 Task 7 中一致
- OpenAI `finish_reason` 映射到 `StreamChunk::Stop`，Anthropic `stop_reason` 同样映射到 `StreamChunk::Stop`

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-28-llm-streaming.md`.**

**Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints for review

**Which approach?**
