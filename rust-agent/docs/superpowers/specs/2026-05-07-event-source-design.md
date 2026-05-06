# SSE 事件来源标识设计文档

## 背景

当前后端 SSE 事件流中，主 Agent 与 Bot 子代理产生的事件共用同一个通道（`event_tx`），前端无法区分事件来源。前端只能通过 `activeBotName` 状态机推断，存在以下缺陷：

1. **并行 call_bot 归属错误**：多个 Bot 同时运行时，`activeBotName` 只能记录一个，子工具调用可能挂到错误的 call_bot 下。
2. **text_delta/thinking_delta 无法区分来源**：主 Agent 和 Bot 的输出全部累加到 `assistantText`，前端无法区分。
3. **状态机推断不可靠**：事件到达顺序受网络影响，`activeBotName` 的推断逻辑存在竞态风险。

## 目标

给每个 SSE 事件添加**不可变、类型安全**的来源标识，使前端能精确区分事件来自主 Agent 还是哪个 Bot 子代理（包括并行的多个 Bot）。

## 设计

### 1. 新增 `EventSource` 类型

```rust
/// 事件来源标识：区分事件由主 Agent 还是某个 Bot 子代理产生
#[derive(Clone, Debug, Serialize)]
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

- `Default` 返回 `Main`，便于向后兼容测试代码。
- 使用 `#[serde(tag = "role", rename_all = "snake_case")]`，前端可直接按 `role` 字段分支。
- `call_id` 使用 nanoid 生成，确保并行 Bot 间唯一。

### 2. 修改 `AgentEvent` 枚举

每个变体添加 `source: EventSource`：

```rust
pub enum AgentEvent {
    TextDelta { content: String, source: EventSource },
    ThinkingDelta { content: String, source: EventSource },
    ToolCall { id: Option<String>, name: String, input: Value, parallel_index: Option<(usize, usize)>, source: EventSource },
    ToolResult { id: Option<String>, name: String, output: String, parallel_index: Option<(usize, usize)>, source: EventSource },
    TurnEnd { api_calls: usize, token_usage: Option<TokenUsage>, source: EventSource },
    Done { source: EventSource },
    Error { code: String, message: String, source: EventSource },
    Retrying { attempt: u32, max_retries: u32, wait_seconds: u64, detail: String, source: EventSource },
}
```

**原则**：`source` 是事件的固有属性，必须在构造时指定，不允许默认值（除测试外）。

### 3. 后端事件构造点改动

#### 3.1 `run_agent_loop` 签名

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
) -> AgentResult<String>
```

所有 `event_tx.send(...)` 调用都需带上 `source.clone()`。

#### 3.2 主 Agent 入口（`handle_user_turn`）

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
        EventSource::Main, // 主 Agent
    )
    .await;
```

#### 3.3 Bot 子代理入口（`run_bot`）

每个并行 Bot 调用分配唯一 `call_id`（nanoid），创建 `EventSource::Bot` 传入：

```rust
for tc in &bot_calls {
    let bot_name = tc.input.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    let bot_task = tc.input.get("task").and_then(Value::as_str).unwrap_or_default().to_owned();
    let call_id = nanoid::nanoid!();
    let source = EventSource::Bot { name: bot_name.clone(), call_id };

    let app = self.clone();
    let event_tx = Arc::clone(event_tx);
    let sid = parent_session_id.unwrap_or("").to_owned();
    bot_handles.push(tokio::spawn(async move {
        app.run_bot(&bot_name, &bot_task, &event_tx, &sid, source).await
    }));
}
```

`run_bot` 签名同步修改：

```rust
async fn run_bot(
    &self,
    bot_name: &str,
    task: &str,
    event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    parent_session_id: &str,
    source: EventSource, // 新增
) -> AgentResult<String>
```

### 4. SSE 序列化变化

`crates/server/src/sse.rs` 在每个事件 JSON 中注入 `"source"` 字段：

```rust
AgentEvent::TextDelta { content, source } => {
    Event::default()
        .event("text_delta")
        .data(json!({ "content": content, "source": source }).to_string())
}
// 其他变体同理...
```

输出示例：

```json
// 主 Agent 的 text_delta
{
  "content": "正在分析...",
  "source": { "role": "main" }
}

// Bot "代码审查Bot" 的 tool_call
{
  "name": "read_file",
  "input": {"file_path": "src/api.rs"},
  "source": { "role": "bot", "name": "代码审查Bot", "call_id": "abc123" }
}
```

### 5. 前端改动

#### 5.1 `web/src/store/chat.ts`

- **移除** `activeBotName` 状态推断逻辑。
- 每个事件直接读取 `evt.data.source`：
  - `role === 'main'` → 主 Agent 消息，累加到 `assistantText`。
  - `role === 'bot'` → 查找或创建对应 `call_id` 的 call_bot 容器，事件归属到该容器下。

#### 5.2 `UIToolCall` 类型扩展

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
  source?: { role: 'main' } | { role: 'bot'; name: string; call_id: string }
  /** Bot 内部产生的文本块（text_delta / thinking_delta） */
  botText?: string
  botThinking?: string
}
```

#### 5.3 `StreamingState` 扩展

```typescript
export interface StreamingState {
  // ...现有字段...
  /** 当前活跃的 Bot 来源（已被 source 字段取代，待移除） */
  // activeBotName: string | null;  ← 移除
}
```

### 6. 错误处理与边界情况

| 场景 | 处理策略 |
|------|---------|
| 测试代码未传 `source` | `EventSource::default()` 返回 `Main`，向后兼容 |
| 并行 Bot 事件乱序 | 前端通过 `call_id` 精确匹配归属，不受顺序影响 |
| Bot 嵌套调用（违规） | `run_agent_loop` 已禁止 `call_bot`，如 LLM 违规，事件来源仍正确标识 |
| 浏览器刷新后重放 | `SessionBroadcaster` 缓存的 `AgentEvent` 已包含 `source`，重放正确 |

## 影响范围

### 后端文件

- `crates/core/src/agent.rs` — `AgentEvent` 定义、所有构造点、`run_agent_loop`、`run_bot` 签名
- `crates/server/src/sse.rs` — 序列化注入 `source`

### 前端文件

- `web/src/store/chat.ts` — 解析逻辑替换
- `web/src/types/ui.ts` — 类型扩展

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 修改量大（30+ 处 AgentEvent 构造） | 使用 IDE 重构 + 编译器检查，分步验证 |
| 前端 `activeBotName` 移除后其他依赖 | 全局搜索 `activeBotName`，逐一替换 |
| SSE 协议变更导致旧前端不兼容 | 本次改动只增字段，旧前端可忽略 `source` 字段，保持运行 |

## 验收标准

1. 编译通过：`cargo check --all-targets` 和 `npm run build` 无错误。
2. 并行 call_bot 时，每个 Bot 的 `text_delta`、`tool_call`、`tool_result` 都携带正确的 `call_id`。
3. 前端能精确区分主 Agent 和多个并行 Bot 的事件，不再依赖 `activeBotName` 推断。
4. 浏览器刷新后重放历史事件，来源标识正确。
