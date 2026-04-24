# Token 记录功能实现计划

## 📊 现状分析

### 当前问题

1. **Token 信息丢失**：Anthropic 和 OpenAI 的 API 响应中都包含 token 用量信息，但在转换为 `ProviderResponse` 时被丢弃了
2. **事件信息不足**：`AgentEvent::TurnEnd` 只携带 `api_calls` 次数，没有 token 消耗信息
3. **缺乏累计追踪**：没有跨会话的 token 累计追踪机制
4. **用户不可见**：CLI 界面只显示 "API 调用 N 次"，不显示 token 消耗

### API 返回的 Token 信息

#### Anthropic API

```json
{
  "usage": {
    "input_tokens": 12345,
    "output_tokens": 3200,
    "cache_creation_input_tokens": 5000,
    "cache_read_input_tokens": 7345
  }
}
```

#### OpenAI API

```json
{
  "usage": {
    "prompt_tokens": 12345,
    "completion_tokens": 3200,
    "total_tokens": 15545
  }
}
```

---

## 🏗️ 实现计划

### 第 1 步：定义 `TokenUsage` 类型

**文件**：`crates/core/src/api/types.rs`

```rust
/// Token 用量统计
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// 输入 tokens（prompt）
    pub input_tokens: u64,
    /// 输出 tokens（completion）
    pub output_tokens: u64,
    /// 缓存创建 tokens（Anthropic 专属，OpenAI 为 0）
    pub cache_creation_tokens: u64,
    /// 缓存读取 tokens（Anthropic 专属，OpenAI 为 0）
    pub cache_read_tokens: u64,
}

impl TokenUsage {
    /// 计算总 tokens
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// 是否为空（所有字段都为 0）
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0
            && self.cache_creation_tokens == 0 && self.cache_read_tokens == 0
    }
}
```

---

### 第 2 步：`ProviderResponse` 增加 `usage` 字段

**文件**：`crates/core/src/api/types.rs`

```rust
/// LLM Provider 返回的统一响应
#[derive(Clone, Debug)]
pub struct ProviderResponse {
    /// 回复的内容块列表（文本和/或工具调用）
    pub content: Vec<ResponseContentBlock>,
    /// 停止原因："end_turn"（正常结束）或 "tool_calls"（需要调用工具）
    pub stop_reason: String,
    /// Token 用量统计
    pub usage: TokenUsage,
}
```

---

### 第 3 步：Anthropic 客户端解析 usage

**文件**：`crates/core/src/api/anthropic.rs`

#### 3.1 更新 `MessagesResponse` 结构

```rust
/// Claude Messages API 的响应体（仅 Anthropic provider 内部使用）
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct MessagesResponse {
    /// Claude 回复的内容块列表（包含文本和/或工具调用）
    pub content: Vec<ResponseContentBlock>,
    /// 停止原因："tool_use"（需要调用工具）、"end_turn"（正常结束）等
    pub stop_reason: Option<String>,
    /// Token 用量统计
    pub usage: Option<AnthropicUsage>,
}

/// Anthropic API 的 usage 格式
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnthropicUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}
```

#### 3.2 在 `create_message` 中转换 usage

```rust
pub async fn create_message(
    &self,
    request: &ProviderRequest<'_>,
) -> AgentResult<ProviderResponse> {
    let raw_request = MessagesRequest {
        model: request.model,
        system: request.system,
        messages: request.messages,
        tools: request.tools,
        max_tokens: request.max_tokens,
    };

    let raw_response = self.create_message_raw(&raw_request).await?;

    // 统一 stop_reason：将 Anthropic 的 "tool_use" 映射为 "tool_calls"
    let stop_reason = match raw_response.stop_reason.as_deref() {
        Some("tool_use") => "tool_calls".to_owned(),
        Some(other) => other.to_owned(),
        None => String::new(),
    };

    // 转换 usage
    let usage = raw_response.usage.map(|u| TokenUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_creation_tokens: u.cache_creation_input_tokens,
        cache_read_tokens: u.cache_read_input_tokens,
    }).unwrap_or_default();

    Ok(ProviderResponse {
        content: raw_response.content,
        stop_reason,
        usage,
    })
}
```

---

### 第 4 步：OpenAI 客户端解析 usage

**文件**：`crates/core/src/api/openai.rs`

#### 4.1 更新 `OpenAIResponse` 结构

```rust
/// OpenAI Chat Completions 响应体
#[derive(Deserialize)]
struct OpenAIResponse {
    pub choices: Vec<OpenAIChoice>,
    pub usage: Option<OpenAIUsage>,
}

/// OpenAI API 的 usage 格式
#[derive(Deserialize)]
struct OpenAIUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
```

#### 4.2 在 `convert_response` 中转换 usage

```rust
/// 将 OpenAI 响应转换为统一的 ProviderResponse
fn convert_response(response: OpenAIResponse) -> ProviderResponse {
    let mut content_blocks = Vec::new();
    let mut stop_reason = String::new();

    if let Some(choice) = response.choices.first() {
        stop_reason = match choice.finish_reason.as_deref() {
            Some("tool_calls") | Some("function_call") => "tool_calls".to_owned(),
            Some("stop") => "end_turn".to_owned(),
            Some(other) => other.to_owned(),
            None => String::new(),
        };

        // 提取文本内容
        if let Some(text) = &choice.message.content
            && !text.is_empty()
        {
            content_blocks.push(ResponseContentBlock::Text { text: text.clone() });
        }

        // 提取工具调用
        if let Some(calls) = &choice.message.tool_calls {
            for call in calls {
                let input: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                content_blocks.push(ResponseContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    input,
                });
            }
        }
    }

    // 转换 usage
    let usage = response.usage.map(|u| TokenUsage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    }).unwrap_or_default();

    ProviderResponse {
        content: content_blocks,
        stop_reason,
        usage,
    }
}
```

---

### 第 5 步：创建 `TokenTracker` 模块

**文件**：`crates/core/src/infra/token_tracker.rs`（新建）

```rust
//! Token 用量追踪器
//!
//! 提供会话级和全局级的 token 用量统计，支持按模型分组。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use serde::{Serialize, Deserialize};

use crate::api::types::TokenUsage;

/// Token 用量快照（用于序列化和展示）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenSnapshot {
    /// 总输入 tokens
    pub total_input: u64,
    /// 总输出 tokens
    pub total_output: u64,
    /// 总缓存创建 tokens
    pub total_cache_creation: u64,
    /// 总缓存读取 tokens
    pub total_cache_read: u64,
    /// 所有 tokens 总和
    pub total_all: u64,
    /// 按模型分组的用量
    pub by_model: HashMap<String, TokenUsage>,
}

/// Token 追踪器（线程安全）
#[derive(Clone)]
pub struct TokenTracker {
    inner: Arc<Mutex<TokenTrackerInner>>,
}

struct TokenTrackerInner {
    total_input: u64,
    total_output: u64,
    total_cache_creation: u64,
    total_cache_read: u64,
    by_model: HashMap<String, TokenUsage>,
}

impl TokenTracker {
    /// 创建新的追踪器
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TokenTrackerInner {
                total_input: 0,
                total_output: 0,
                total_cache_creation: 0,
                total_cache_read: 0,
                by_model: HashMap::new(),
            })),
        }
    }

    /// 记录一次 API 调用的 token 用量
    pub fn record(&self, model: &str, usage: &TokenUsage) {
        let mut inner = self.inner.lock().unwrap();
        inner.total_input += usage.input_tokens;
        inner.total_output += usage.output_tokens;
        inner.total_cache_creation += usage.cache_creation_tokens;
        inner.total_cache_read += usage.cache_read_tokens;

        *inner.by_model.entry(model.to_owned()).or_default() += usage;
    }

    /// 获取当前快照
    pub fn snapshot(&self) -> TokenSnapshot {
        let inner = self.inner.lock().unwrap();
        let total_all = inner.total_input + inner.total_output
            + inner.total_cache_creation + inner.total_cache_read;

        TokenSnapshot {
            total_input: inner.total_input,
            total_output: inner.total_output,
            total_cache_creation: inner.total_cache_creation,
            total_cache_read: inner.total_cache_read,
            total_all,
            by_model: inner.by_model.clone(),
        }
    }

    /// 重置追踪器（新会话时调用）
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.total_input = 0;
        inner.total_output = 0;
        inner.total_cache_creation = 0;
        inner.total_cache_read = 0;
        inner.by_model.clear();
    }
}

impl Default for TokenTracker {
    fn default() -> Self {
        Self::new()
    }
}

// 实现 TokenUsage 的加法运算
impl std::ops::AddAssign<&TokenUsage> for TokenUsage {
    fn add_assign(&mut self, rhs: &TokenUsage) {
        self.input_tokens += rhs.input_tokens;
        self.output_tokens += rhs.output_tokens;
        self.cache_creation_tokens += rhs.cache_creation_tokens;
        self.cache_read_tokens += rhs.cache_read_tokens;
    }
}
```

---

### 第 6 步：集成到 `AgentApp`

**文件**：`crates/core/src/agent.rs`

#### 6.1 更新 `AgentApp` 结构

```rust
#[derive(Clone)]
pub struct AgentApp {
    client: crate::api::LlmProvider,
    workspace_root: PathBuf,
    skills: Arc<RwLock<SkillLoader>>,
    skill_dirs: Vec<PathBuf>,
    model: String,
    max_tokens: u32,
    tool_extension: Option<Arc<dyn ToolExtension>>,
    identity: AgentIdentity,
    token_tracker: Arc<crate::infra::token_tracker::TokenTracker>,  // 新增
}
```

#### 6.2 在 `from_env` 中初始化追踪器

```rust
pub async fn from_env() -> AgentResult<Self> {
    // ... 现有代码 ...

    Ok(Self {
        client: info.provider,
        workspace_root,
        skills: Arc::new(RwLock::new(skills)),
        skill_dirs,
        model,
        max_tokens,
        tool_extension: None,
        identity,
        token_tracker: Arc::new(crate::infra::token_tracker::TokenTracker::new()),  // 新增
    })
}
```

#### 6.3 在 `run_agent_loop` 中记录 token

```rust
let response = match self.client.create_message(&request).await {
    Ok(resp) => resp,
    Err(e) => {
        // ... 错误处理 ...
    }
};

// 记录 token 用量
self.token_tracker.record(&self.model, &response.usage);

api_call_count += 1;
```

#### 6.4 更新 `AgentEvent::TurnEnd`

```rust
#[derive(Clone, Debug)]
pub enum AgentEvent {
    TextDelta(String),
    ToolCall {
        name: String,
        input: serde_json::Value,
        parallel_index: Option<(usize, usize)>,
    },
    ToolResult {
        name: String,
        output: String,
        parallel_index: Option<(usize, usize)>,
    },
    TurnEnd {
        api_calls: usize,
        token_usage: Option<crate::infra::token_tracker::TokenSnapshot>,  // 新增
    },
    Done,
    Error {
        code: String,
        message: String,
    },
}
```

#### 6.5 在发送 `TurnEnd` 事件时包含 token 信息

```rust
if stop_reason != "tool_calls" {
    let text = response.final_text();
    if config.emit_events {
        if !text.is_empty() {
            let _ = event_tx.send(AgentEvent::TextDelta(text)).await;
        }
        let _ = event_tx
            .send(AgentEvent::TurnEnd {
                api_calls: api_call_count,
                token_usage: Some(self.token_tracker.snapshot()),  // 新增
            })
            .await;
    }
    return Ok(response.final_text());
}
```

---

### 第 7 步：更新模块导出

**文件**：`crates/core/src/infra/mod.rs`

```rust
pub mod circuit_breaker;
pub mod compact;
pub mod config;
pub mod logging;
pub mod storage;
pub mod todo;
pub mod token_tracker;  // 新增
pub mod utils;
pub mod workspace;
```

**文件**：`crates/core/src/lib.rs`

```rust
pub use infra::token_tracker::{TokenTracker, TokenSnapshot};  // 新增
```

---

### 第 8 步：更新 CLI 显示

**文件**：`cli/src/app.tsx`

在 `turn_end` 事件处理中显示 token 用量：

```typescript
case 'turn_end': {
  const reply = currentReplyRef.current;
  setCurrentReply('');
  currentReplyRef.current = '';
  if (reply) {
    setMessages(prev => [...prev, { role: 'assistant', content: reply }]);
  }
  const apiCalls = event.data?.api_calls;
  const tokenUsage = event.data?.token_usage;

  let statusText = `── 完成，API 调用 ${apiCalls} 次`;
  if (tokenUsage) {
    const { total_input, total_output, total_cache_creation, total_cache_read } = tokenUsage;
    const total = total_input + total_output + total_cache_creation + total_cache_read;
    statusText += ` │ 输入 ${formatNumber(total_input)} tokens │ 输出 ${formatNumber(total_output)} tokens`;
    if (total_cache_creation > 0 || total_cache_read > 0) {
      statusText += ` │ 缓存 ${formatNumber(total_cache_creation + total_cache_read)} tokens`;
    }
    statusText += ` │ 总计 ${formatNumber(total)} tokens`;
  }
  statusText += ' ──';

  setMessages(prev => [...prev, { role: 'system', content: statusText }]);
  break;
}

// 格式化数字（添加千位分隔符）
function formatNumber(num: number): string {
  return num.toLocaleString();
}
```

---

### 第 9 步：更新测试 mock 数据

**文件**：`crates/core/src/api/anthropic.rs`

更新测试中的 mock 响应，添加 `usage` 字段：

```rust
async fn should_retry_429_with_retry_after_and_succeed() {
    let (url, counter) = mock_server(vec![
        (
            429,
            r#"{"error":{"type":"rate_limit_error","message":"Rate limited"}}"#,
            Some("1"),
        ),
        (
            200,
            r#"{
              "content":[{"type":"text","text":"hello"}],
              "stop_reason":"end_turn",
              "usage":{
                "input_tokens":100,
                "output_tokens":50,
                "cache_creation_input_tokens":0,
                "cache_read_input_tokens":0
              }
            }"#,
            None,
        ),
    ])
    .await;
    // ... 测试代码 ...
}
```

---

## 📁 涉及文件清单

| 文件                                     | 操作 | 说明                                       |
| ---------------------------------------- | ---- | ------------------------------------------ |
| `crates/core/src/api/types.rs`           | 修改 | 新增 `TokenUsage`，修改 `ProviderResponse` |
| `crates/core/src/api/anthropic.rs`       | 修改 | 解析 Anthropic usage，更新 mock 数据       |
| `crates/core/src/api/openai.rs`          | 修改 | 解析 OpenAI usage                          |
| `crates/core/src/infra/token_tracker.rs` | 新建 | Token 追踪器模块                           |
| `crates/core/src/infra/mod.rs`           | 修改 | 注册新模块                                 |
| `crates/core/src/agent.rs`               | 修改 | 集成追踪器，更新 `AgentEvent`              |
| `crates/core/src/lib.rs`                 | 修改 | 导出新类型                                 |
| `cli/src/app.tsx`                        | 修改 | 显示 token 信息                            |

---

## ⚠️ 风险与注意事项

### 1. 向后兼容性

- `AgentEvent::TurnEnd` 增加字段，server 端 SSE 序列化需要同步更新
- 现有客户端可能不识别新字段，需要确保 `token_usage` 为 `Option` 类型

### 2. 测试更新

- 现有的 `anthropic.rs` 测试用 mock 响应不含 `usage` 字段，需要更新 mock 数据
- 需要添加新的测试用例验证 token 追踪功能

### 3. OpenAI 兼容 API

- 部分第三方 API（如 Ollama）可能不返回 `usage` 字段，需要 `Option` 处理
- 在 `TokenUsage` 中提供 `is_empty()` 方法判断是否有有效数据

### 4. 子代理 Token

- 子代理的 token 也应被计入主追踪器
- 需要确保 `run_subagent` 中也调用 `token_tracker.record()`

### 5. 线程安全

- `TokenTracker` 使用 `Arc<Mutex<>>` 确保线程安全
- 在高频调用场景下需要考虑性能影响

---

## 🎯 预期效果

### CLI 界面显示示例

```
── 完成，API 调用 3 次 │ 输入 12,450 tokens │ 输出 3,200 tokens │ 总计 15,650 tokens ──
```

对于支持缓存的模型（如 Claude）：

```
── 完成，API 调用 2 次 │ 输入 8,000 tokens │ 输出 2,500 tokens │ 缓存 5,000 tokens │ 总计 15,500 tokens ──
```

### API 响应示例

```json
{
  "event": "turn_end",
  "data": {
    "api_calls": 3,
    "token_usage": {
      "total_input": 12450,
      "total_output": 3200,
      "total_cache_creation": 5000,
      "total_cache_read": 7345,
      "total_all": 27995,
      "by_model": {
        "claude-sonnet-4-20250514": {
          "input_tokens": 12450,
          "output_tokens": 3200,
          "cache_creation_tokens": 5000,
          "cache_read_tokens": 7345
        }
      }
    }
  }
}
```

---

## 📝 实施检查清单

- [ ] 定义 `TokenUsage` 类型（`api/types.rs`）
- [ ] `ProviderResponse` 增加 `usage` 字段
- [ ] Anthropic 客户端解析 usage
- [ ] OpenAI 客户端解析 usage
- [ ] 创建 `TokenTracker` 模块（`infra/token_tracker.rs`）
- [ ] 集成到 `AgentApp`，更新 `AgentEvent::TurnEnd`
- [ ] 更新模块导出（`infra/mod.rs`、`lib.rs`）
- [ ] 更新 CLI 显示 token 信息
- [ ] 更新测试 mock 数据
- [ ] 添加单元测试验证 token 追踪功能
- [ ] 测试子代理 token 计入
- [ ] 测试 OpenAI 兼容 API（如 Ollama）的兼容性
