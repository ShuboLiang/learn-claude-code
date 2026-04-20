# A2A 协议服务端设计文档

> 日期: 2026-04-20
> 范围: 为 rust-agent 添加 Google A2A (Agent-to-Agent) 开放协议的服务端支持
> 方案: 方案 A（最小完整实现）

---

## 1. 背景与目标

### 1.1 背景

本项目是一个 Rust 实现的编程助手 agent，架构为三层：
- `crates/core`：agent 核心逻辑（LLM API 调用、工具系统、技能系统、子 agent 并行执行、上下文压缩）
- `crates/server`：HTTP 服务端，提供 OpenAI 兼容 API 和自定义 session API
- `cli/`：TypeScript CLI 客户端

### 1.2 目标

让外部 agent（如 Google ADK、LangChain 等）能够通过 [Google A2A (Agent-to-Agent) 开放协议](https://developers.google.com/agent-to-agent) 发现和调用本 agent 的能力。

**本次范围：服务端角色**（本 agent 被外部 agent 调用）。客户端角色（调用外部 agent）后续再考虑。

### 1.3 关键约束

- **全部工具暴露为 A2A skills**：bash、file ops、search、skill 管理、子 agent 委派等
- **独立 crate + 独立 binary**：不侵入 `crates/server`
- **暂不认证**：MVP 阶段跳过认证，仅用于本地/内网验证
- **直接依赖 core**：复用 `AgentApp`、`AgentEvent`、`AgentToolbox`

---

## 2. 架构 Overview

### 2.1 新增 Crate

```
crates/
├── core/        # 现有：agent 核心逻辑
├── server/      # 现有：OpenAI 兼容 API + session API
└── a2a/         # 新增：A2A 协议服务端
    ├── Cargo.toml
    └── src/
        ├── main.rs          # binary 入口，启动 HTTP server
        ├── lib.rs           # crate 根，暴露公共类型
        ├── types.rs         # A2A 协议数据模型
        ├── agent_card.rs    # 从 AgentToolbox 动态生成 Agent Card
        ├── routes.rs        # axum 路由定义
        ├── handlers.rs      # 请求处理器
        ├── task_runner.rs   # A2A Task → AgentApp 执行映射
        ├── streaming.rs     # AgentEvent → A2A SSE 事件转换
        └── state.rs         # AppState（共享的 task store、agent factory）
```

### 2.2 依赖关系

- `crates/a2a` → `rust-agent-core`（复用 `AgentApp`、`AgentEvent`、`AgentToolbox`、`ContextService`）
- `crates/a2a` 不依赖 `crates/server`

### 2.3 运行时模型

- 独立进程，独立端口（如 `:3001`，现有 server 跑在 `:3000`）
- 每个 A2A task 启动一次 `AgentApp::handle_user_turn` 执行
- Task 之间不共享上下文（MVP 暂不支持跨 task 的 session 记忆）
- 使用内存中的 `DashMap<String, TaskState>` 跟踪任务生命周期

### 2.4 核心映射关系

| A2A 概念 | 本项目映射 |
|---|---|
| Agent Card skills | `AgentToolbox::tool_schemas()` 动态生成 |
| Task | 一次 `handle_user_turn` 调用 |
| Message (user → agent) | `user_input` 参数 + 文件读取 |
| Message (agent → user) | `AgentEvent::TextDelta` + `ToolCall`/`ToolResult` |
| Artifact | 工具结果中的 `<persisted-output>` 或文件路径 |
| Streaming SSE | `AgentEvent` 流 → A2A 标准 SSE 事件 |

---

## 3. 数据模型

### 3.1 Agent Card

```rust
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    pub capabilities: Capabilities,
    pub authentication: Option<AuthConfig>,      // MVP: null
    pub default_input_modes: Vec<String>,        // ["text"]
    pub default_output_modes: Vec<String>,       // ["text", "file"]
    pub skills: Vec<Skill>,
}

pub struct Capabilities {
    pub streaming: bool,            // true
    pub push_notifications: bool,   // false（MVP 不支持）
    pub state_transition_history: bool, // false（MVP 不支持）
}

pub struct Skill {
    pub id: String,                 // 对应工具名
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub examples: Vec<String>,
    pub input_modes: Vec<String>,
    pub output_modes: Vec<String>,
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
}
```

### 3.2 Task

```rust
pub struct Task {
    pub id: String,
    pub session_id: Option<String>,
    pub status: TaskStatus,
    pub history: Vec<Message>,
    pub artifacts: Vec<Artifact>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum TaskStatus {
    Submitted,
    Working,
    InputRequired,   // MVP 不实现
    Completed,
    Failed,
    Canceled,
}
```

### 3.3 Message / Part

```rust
pub struct Message {
    pub role: Role,    // "user" | "agent"
    pub parts: Vec<Part>,
}

pub enum Part {
    Text { text: String },
    File { file: FileContent },
    Data { data: serde_json::Value },
}

pub struct FileContent {
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub bytes: Option<String>,      // base64
    pub uri: Option<String>,        // 文件路径或 URL
}
```

### 3.4 Artifact

```rust
pub struct Artifact {
    pub name: Option<String>,
    pub description: Option<String>,
    pub parts: Vec<Part>,
    pub metadata: Option<serde_json::Value>,
    pub index: u32,
    pub append: Option<bool>,
}
```

---

## 4. HTTP 端点

### 4.1 `GET /.well-known/agent.json`

返回 Agent Card。启动时从 `AgentApp::from_env()` 初始化 agent，读取工具 schema 动态生成 skills 列表并**缓存**。

### 4.2 `POST /tasks/send` — 同步/阻塞模式

**请求体**：
```json
{
  "id": "task-123",
  "sessionId": "optional-session",
  "message": {
    "role": "user",
    "parts": [{ "type": "text", "text": "请分析这个项目的架构" }]
  },
  "metadata": {}
}
```

**行为**：
1. 验证 `id` 唯一性（若已存在返回 409）
2. 创建 `Task { status: Submitted, ... }`
3. 启动 `AgentApp`，调用 `handle_user_turn`
4. 阻塞等待执行完成
5. 返回完整 Task（status = Completed 或 Failed）

**超时**：5 分钟，超时返回 `Failed` + 超时错误信息。

### 4.3 `POST /tasks/sendSubscribe` — 流式模式（SSE）

**响应**：`text/event-stream`

**SSE 事件序列**：

```
event: task-status
data: {"id":"task-123","status":{"state":"working"},"final":false}

event: task-message
data: {"id":"task-123","message":{"role":"agent","parts":[{"type":"text","text":"我来分析..."}]},"final":false}

event: task-artifact
data: {"id":"task-123","artifact":{"name":"architecture.md","parts":[...]},"final":false}

event: task-status
data: {"id":"task-123","status":{"state":"completed"},"final":true}
```

**状态流转**：
- `Submitted` → `Working`（agent 开始执行）
- `Working` → `Completed`（agent 正常结束）
- `Working` → `Failed`（agent 报错或超时）
- `Working` → `Canceled`（收到取消请求）

### 4.4 `GET /tasks/{taskId}`

查询任务当前状态。已完成/失败的任务返回缓存结果；运行中的任务返回当前快照。

### 4.5 `POST /tasks/{taskId}/cancel`

取消正在运行的任务。

**实现方式**：软取消。从 task store 中移除任务，停止向 SSE 客户端推送新事件。agent 实际仍在后台运行直到自然结束（单个 task 有轮数上限，资源泄漏可控）。

---

## 5. Skills 映射

### 5.1 工具 → Skill 对照表

| 内部工具名 | A2A Skill ID | 说明 |
|---|---|---|
| `bash` | `bash` | 直接暴露 |
| `read_file` | `read_file` | 直接暴露 |
| `write_file` | `write_file` | 直接暴露 |
| `search` | `search` | 直接暴露 |
| `glob` | `glob` | 直接暴露 |
| `grep` | `grep` | 直接暴露 |
| `task` | `delegate_task` | 重命名，避免与 A2A task 概念混淆 |
| `todo` | `todo` | 直接暴露 |
| `compact` | `compact` | 直接暴露 |
| `load_skill` | `load_skill` | 直接暴露 |
| `search_skillhub` | `search_skillhub` | 直接暴露 |
| `install_skill` | `install_skill` | 直接暴露 |

**Skill 的 `input_schema`** 直接复用现有工具的 JSON Schema。

### 5.2 User Input 提取

A2A 请求 `message.parts` 的处理策略（MVP）：

- `TextPart` → 直接作为 `user_input`
- `FilePart` (有 `uri`) → 读取文件，在 `user_input` 中附加文件内容说明
- `FilePart` (有 `bytes`) → 返回 400 Bad Request（暂不支持）
- `DataPart` → 返回 400 Bad Request（暂不支持）

### 5.3 Task 执行

```rust
let mut ctx = ContextService::new();
let result = agent.handle_user_turn(&mut ctx, &user_input, event_tx).await;
```

**每个 task 新建 `ContextService`，不保留跨 task 状态。** 外部 agent 若需多轮对话，需自行在 message 中携带历史上下文。

---

## 6. Streaming 映射

### 6.1 AgentEvent → A2A SSE

| AgentEvent | SSE 事件 | payload |
|---|---|---|
| `TextDelta(text)` | `task-message` | `{ role: "agent", parts: [{type:"text", text}] }` |
| `ToolCall { name, input }` | `task-message` | 格式化为描述文本 |
| `ToolResult { name, output }` | `task-message` + 可能 `task-artifact` | 文本摘要 + 文件检测 |
| `TurnEnd { api_calls }` | `task-status` | `{ state: "working" }`（中间轮次） |
| — | `task-status` | `{ state: "completed", final: true }`（最终） |

**注意**：一次 task 可能经历多个 `TurnEnd`（多轮工具调用），只有最后一轮结束后才发 `completed`。

### 6.2 Artifact 生成

**检测时机**：在 `ToolResult` 事件中实时检测，流式模式下外部 agent 可实时看到文件产出。

**检测规则**：
1. 输出包含 `<persisted-output path="...">` → 生成 FilePart Artifact
2. `write_file` 成功 → 生成 FilePart Artifact（文件名来自 tool input）
3. 长文本（> 1000 字符）→ 可保留在 message 中，不强制生成 artifact

---

## 7. 错误处理

| 场景 | HTTP 状态码 | 错误码 |
|---|---|---|
| Task ID 冲突 | `409 Conflict` | `task_exists` |
| Task 未找到 | `404 Not Found` | `task_not_found` |
| Agent 初始化失败 | `503 Service Unavailable` | `agent_init_failed` |
| Agent 执行异常 | SSE final `task-status` | `failed`（state + message） |
| 不支持的消息格式 | `400 Bad Request` | `unsupported_part_type` |
| 请求 JSON 无效 | `400 Bad Request` | `invalid_request` |

错误响应体格式：
```json
{
  "error": {
    "code": "task_not_found",
    "message": "Task task-123 does not exist"
  }
}
```

---

## 8. 共享状态

```rust
pub struct AppState {
    pub tasks: Arc<DashMap<String, TaskState>>,
    pub agent_card: AgentCard,    // 启动时生成并缓存
}

pub enum TaskState {
    Running {
        task: Task,
    },
    Completed(Task),
    Failed {
        task: Task,
        error: String,
    },
    Canceled,
}
```

---

## 9. 测试策略

### 9.1 单元测试

- `types.rs`：序列化/反序列化 roundtrip
- `agent_card.rs`：验证从 mock tool schemas 正确生成 skills
- `streaming.rs`：验证 AgentEvent 到 SSE 事件的转换

### 9.2 集成测试

启动 a2a server，用 `reqwest` 调用：
- `/.well-known/agent.json` 返回有效 JSON 且包含预期 skills
- `/tasks/send` 能完成简单任务（如 `glob *.rs`）
- `/tasks/sendSubscribe` 返回正确 SSE 事件序列和最终 `completed`

### 9.3 互操作测试

使用 Google ADK 客户端或 LangChain A2A adapter 连接验证端到端流程。

---

## 10. 后续可扩展（不在 MVP 范围）

1. **认证**：Bearer Token、OAuth2
2. **`InputRequired` 状态**：支持外部 agent 多轮交互
3. **跨 task session 记忆**：复用 `ContextService` 上下文
4. **硬取消**：在 core 的 agent loop 中增加取消检查点
5. **`FilePart(bytes)` 和 `DataPart`** 支持
6. **Push Notifications**
7. **A2A 客户端角色**：让本 agent 能发现和调用外部 A2A agent

---

## 11. 决策记录

| 决策 | 选择 | 理由 |
|---|---|---|
| 独立 crate | ✅ | 与现有 server 解耦，A2A 协议升级更易维护 |
| 独立 binary | ✅ | 独立进程，独立端口，部署灵活 |
| 启动时生成 Agent Card | ✅ | 工具集运行时不变，减少请求开销 |
| 每个 task 新建 ContextService | ✅ | 符合 A2A stateless task 语义 |
| `task` 工具重命名 `delegate_task` | ✅ | 避免与 A2A task 概念混淆 |
| 软取消 | ✅ | core 暂无取消机制，改动最小 |
| MVP 跳过 `InputRequired` | ✅ | 降低复杂度，单轮 task 完全可用 |
| MVP 跳过 `FilePart(bytes)`/`DataPart` | ✅ | 大多数客户端初始只发文本 |
| 同步任务超时 5 分钟 | ✅ | 合理上限，防止无限阻塞 |
