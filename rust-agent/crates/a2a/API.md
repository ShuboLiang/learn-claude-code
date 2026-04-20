# rust-agent-a2a API 文档

> **协议**: A2A (Agent-to-Agent Protocol) v1.0  
> **传输绑定**: HTTP+JSON/REST  
> **版本**: 0.1.0  
> **Base URL**: `http://localhost:3001`（可通过环境变量 `A2A_BASE_URL` 配置）  
> **默认端口**: `3001`（可通过环境变量 `A2A_PORT` 配置）  

---

## 目录

- [概述](#概述)
- [认证](#认证)
- [HTTP 头](#http-头)
- [数据模型](#数据模型)
  - [AgentCard](#agentcard)
  - [Task](#task)
  - [Message](#message)
  - [Part](#part)
  - [TaskStatus](#taskstatus)
  - [Artifact](#artifact)
  - [SSE 流式响应](#sse-流式响应)
- [端点列表](#端点列表)
  - [GET /.well-known/agent.json](#get-well-knownagentjson)
  - [POST /message:send](#post-messagesend)
  - [POST /message:stream](#post-messagestream)
  - [GET /tasks/{taskId}](#get-taskstaskid)
  - [POST /tasks/{taskId}/cancel](#post-taskstaskidcancel)
  - [GET /tasks](#get-tasks)
  - [POST /tasks/{taskId}/subscribe](#post-taskstaskidsubscribe)
  - [GET /extendedAgentCard](#get-extendedagentcard)
  - [Push Notification 端点](#push-notification-端点)
- [错误处理](#错误处理)
- [代码示例](#代码示例)
- [限制与注意事项](#限制与注意事项)

---

## 概述

`rust-agent-a2a` 是一个基于 Rust 的 **A2A v1.0 (Agent-to-Agent) 协议服务器实现**。它遵循 [A2A Protocol Specification](https://a2a-protocol.org/latest/specification/) 的 HTTP+JSON/REST 传输绑定，允许其他 Agent 客户端通过标准 HTTP API 与本 Agent 进行交互，支持同步调用和 Server-Sent Events (SSE) 流式响应。

所有端点均开启全跨域 (CORS permissive)，便于浏览器端和各类客户端直接调用。

---

## 认证

**当前版本 (MVP) 暂不实现认证。**

`AgentCard.securitySchemes` 和 `security` 为 `null`。调用任何端点均无需 Bearer Token、API Key 或 OAuth2。后续版本将补充认证机制。

---

## HTTP 头

### A2A-Version

客户端**应当**在每次请求中发送 `A2A-Version: 1.0` 头。如果发送了不支持的版本，服务端将返回 `VersionNotSupportedError`。

---

## 数据模型

### AgentCard

Agent 的元数据信息卡，用于服务发现和能力声明。

```json
{
  "name": "rust-agent",
  "description": "A Rust-based programming assistant with tool execution capabilities.",
  "url": "http://localhost:3001",
  "version": "0.1.0",
  "capabilities": {
    "streaming": true,
    "pushNotifications": false,
    "stateTransitionHistory": false
  },
  "defaultInputModes": ["text/plain"],
  "defaultOutputModes": ["text/plain"],
  "skills": [
    {
      "id": "delegate_task",
      "name": "delegate_task",
      "description": "...",
      "tags": [],
      "examples": [],
      "inputModes": ["text/plain"],
      "outputModes": ["text/plain"]
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `string` | Agent 名称 |
| `description` | `string` | Agent 功能描述 |
| `url` | `string` | 服务地址 |
| `provider` | `AgentProvider` | 提供商信息（可选） |
| `version` | `string` | 服务版本号 |
| `documentationUrl` | `string` | 文档地址（可选） |
| `capabilities` | `Capabilities` | 能力声明 |
| `authentication` | `object \| null` | 认证配置（可选） |
| `defaultInputModes` | `string[]` | 默认输入 MIME 类型 |
| `defaultOutputModes` | `string[]` | 默认输出 MIME 类型 |
| `skills` | `Skill[]` | Agent 支持的工具/技能列表 |

### Task

任务是 A2A 协议的核心概念，代表一次与 Agent 的交互。

```json
{
  "id": "task-uuid-server-generated",
  "contextId": "ctx-uuid",
  "status": {
    "state": "completed",
    "message": null,
    "timestamp": "2026-04-20T10:00:00.000Z"
  },
  "history": [
    {
      "messageId": "msg-1",
      "role": "user",
      "parts": [{ "text": "写一个 Rust 函数" }]
    },
    {
      "messageId": "msg-2",
      "taskId": "task-uuid-server-generated",
      "role": "agent",
      "parts": [{ "text": "好的，这是函数..." }]
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | `string` | **服务端生成的** 任务唯一标识 (UUID) |
| `contextId` | `string` | 会话/上下文标识（服务端生成） |
| `status` | `TaskStatus` | 当前任务状态 |
| `artifacts` | `Artifact[]` | 产出物列表（无产出物时省略） |
| `history` | `Message[]` | 消息历史（无历史时省略） |
| `metadata` | `object` | 可选元数据 |

### Message

对话消息。多轮对话时，在 `taskId` 字段中引用已有任务。

```json
{
  "messageId": "msg-1",
  "contextId": "ctx-abc",
  "taskId": "task-uuid",
  "role": "user",
  "parts": [{ "text": "你好" }],
  "metadata": null,
  "extensions": null
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `messageId` | `string` | ✅ | 客户端生成的消息唯一标识 (UUID) |
| `contextId` | `string` | ❌ | 可选上下文 ID |
| `taskId` | `string` | ❌ | 引用已有任务 ID（多轮对话时使用） |
| `role` | `"user" \| "agent"` | ✅ | 消息发送者角色 |
| `parts` | `Part[]` | ✅ | 消息内容片段 |
| `metadata` | `object` | ❌ | 可选元数据 |
| `extensions` | `string[]` | ❌ | 扩展字段 |

### Part

消息内容片段。**通过成员存在性（member presence）判别类型**，**没有 `type` 或 `kind` 字段**。

**Text Part**
```json
{ "text": "你好，世界" }
```

**File Part (by URL)**
```json
{
  "filename": "example.rs",
  "mediaType": "text/x-rust",
  "url": "file:///tmp/example.rs"
}
```

**File Part (inline bytes)**
```json
{
  "filename": "diagram.png",
  "mediaType": "image/png",
  "raw": "iVBORw0KGgo..."
}
```

**Data Part**
```json
{
  "data": { "key": "value" },
  "mediaType": "application/json"
}
```

| Part 类型 | 支持状态 | 说明 |
|-----------|----------|------|
| `text` | ✅ 完全支持 | 纯文本内容（`text` 字段存在） |
| `file` (url) | ✅ 支持 | 通过 URI/URL 引用的文件（`url` 字段存在） |
| `file` (raw) | ❌ 暂不支持 | 直接嵌入 Base64 字节（`raw` 字段存在） |
| `data` | ❌ 暂不支持 | 结构化数据对象（`data` 字段存在） |

### TaskStatus

任务状态。

```json
{
  "state": "completed",
  "message": null,
  "timestamp": "2026-04-20T10:00:00.000Z"
}
```

| 状态值 | 说明 |
|--------|------|
| `submitted` | 已提交 |
| `working` | 执行中 |
| `input-required` | 等待用户输入（MVP 不支持） |
| `completed` | 已完成 |
| `failed` | 执行失败 |
| `canceled` | 已取消 |
| `rejected` | 已拒绝 |
| `auth-required` | 需要认证（MVP 不支持） |

### Artifact

任务产出物，用于返回 Agent 生成的文件或结果。

```json
{
  "artifactId": "art-001",
  "name": "output.md",
  "description": "生成的文档",
  "parts": [
    { "url": "/tmp/output.md" }
  ],
  "metadata": {},
  "extensions": []
}
```

**Artifact 字段**

| 字段 | 类型 | 说明 |
|------|------|------|
| `artifactId` | `string` | **必填**，产出物唯一标识（任务内唯一） |
| `name` | `string` | 可读名称 |
| `description` | `string` | 可读描述 |
| `parts` | `Part[]` | **必填**，内容片段 |
| `metadata` | `object` | 可选元数据 |
| `extensions` | `string[]` | 扩展 URI 列表 |

### SSE 流式响应

流式端点 `/message:stream` 和 `/tasks/{taskId}/subscribe` 返回 `Content-Type: text/event-stream`。每个 SSE 事件的数据字段包含一个 `StreamResponse` 包装器，**不使用 `event:` 字段区分事件类型**。

```
data: {"task":{"id":"task-001","status":{"state":"submitted"},...}}

data: {"statusUpdate":{"taskId":"task-001","contextId":"ctx-001","status":{"state":"working"}}}

data: {"message":{"messageId":"msg-2","role":"agent","parts":[{"text":"正在处理..."}]}}

data: {"artifactUpdate":{"taskId":"task-001","contextId":"ctx-001","artifact":{"artifactId":"art-001","name":"out.md","parts":[{"url":"/tmp/out.md"}]}}}

data: {"statusUpdate":{"taskId":"task-001","contextId":"ctx-001","status":{"state":"completed"}}}
```

**StreamResponse 字段**

| 字段 | 类型 | 说明 |
|------|------|------|
| `task` | `Task` | 初始任务快照 |
| `message` | `Message` | Agent 输出消息 |
| `statusUpdate` | `TaskStatusUpdateEvent` | 状态变更事件 |
| `artifactUpdate` | `TaskArtifactUpdateEvent` | 产出物事件 |

终端状态更新（`completed`、`failed`、`canceled`）表示流结束。

---

## 端点列表

### GET /.well-known/agent.json

获取 Agent 的信息卡（Agent Card），用于服务发现和能力声明。

**认证**: 不需要

**请求参数**: 无

**成功响应 (200 OK)**

返回 `AgentCard` JSON。

---

### POST /message:send

发送消息。如果是新任务，服务端会自动生成 `taskId` 和 `contextId`；如果是多轮对话，在 `message.taskId` 中传入已有任务 ID。

**认证**: 不需要

**请求体**

```json
{
  "message": {
    "messageId": "msg-1",
    "role": "user",
    "parts": [{ "text": "写一个 Hello World Rust 程序" }]
  },
  "configuration": {
    "returnImmediately": false,
    "acceptedOutputModes": ["text/plain"],
    "historyLength": 10
  },
  "metadata": null
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message` | `Message` | ✅ | 用户消息 |
| `configuration` | `SendMessageConfiguration` | ❌ | 发送配置 |
| `metadata` | `object` | ❌ | 可选元数据 |

**SendMessageConfiguration**

| 字段 | 类型 | 说明 |
|------|------|------|
| `returnImmediately` | `boolean` | `true` 时非阻塞返回，`false`（默认）时阻塞等待完成 |
| `acceptedOutputModes` | `string[]` | 客户端接受的输出 MIME 类型 |
| `historyLength` | `number` | 返回的历史消息数量限制 |
| `taskPushNotificationConfig` | `object` | 推送通知配置 |

**多轮对话跟进**

```json
{
  "message": {
    "messageId": "msg-2",
    "taskId": "550e8400-e29b-41d4-a716-446655440000",
    "role": "user",
    "parts": [
      { "text": "添加注释说明" }
    ]
  }
}
```

**成功响应 (200 OK)**

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "contextId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "status": {
    "state": "completed",
    "message": null,
    "timestamp": "2026-04-20T10:00:05.123Z"
  },
  "history": [
    {
      "messageId": "msg-agent-1",
      "taskId": "550e8400-e29b-41d4-a716-446655440000",
      "role": "agent",
      "parts": [{ "text": "```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```" }]
    }
  ],
  "metadata": null
}
```

**错误响应**

- `400 Bad Request` — 不支持的 Part 类型
  ```json
  { "error": { "code": "ContentTypeNotSupportedError", "message": "Data parts are not supported in MVP" } }
  ```
- `404 Not Found` — 多轮对话时引用的 `taskId` 不存在
  ```json
  { "error": { "code": "TaskNotFoundError", "message": "Task xxx not found" } }
  ```
- `405 Method Not Allowed` — 向终端状态任务发送跟进消息
  ```json
  { "error": { "code": "UnsupportedOperationError", "message": "Task is already in a terminal state" } }
  ```

**说明**:
- 新任务会自动创建新的上下文，服务端生成 `taskId` + `contextId`。
- 多轮对话会复用已有任务的上下文，**无需**客户端发送历史消息。
- 同步调用默认阻塞直到 Agent 完成执行。
- `returnImmediately: true` 立即返回状态为 `working` 的任务，后台继续执行。

---

### POST /message:stream

发送消息并通过 Server-Sent Events (SSE) 流式接收 Agent 的执行过程。

**认证**: 不需要

**请求体**: 与 `POST /message:send` 相同。多轮对话同样通过 `message.taskId` 实现。

**响应**: `Content-Type: text/event-stream`

**SSE 流示例**

```
data: {"task":{"id":"task-001","status":{"state":"submitted"},...}}

data: {"statusUpdate":{"taskId":"task-001","status":{"state":"working","message":null,"timestamp":"2026-04-20T10:00:01.000Z"}}}

data: {"message":{"messageId":"msg-2","taskId":"task-001","role":"agent","parts":[{"text":"正在编写 Rust 代码..."}]}}

data: {"artifactUpdate":{"taskId":"task-001","artifact":{"name":"main.rs","parts":[{"url":"/tmp/main.rs"}],"index":0}}}

data: {"statusUpdate":{"taskId":"task-001","status":{"state":"completed","message":null,"timestamp":"2026-04-20T10:00:05.000Z"}}}
```

**错误响应**

- `400 Bad Request` — 不支持的 Part 类型
- `404 Not Found` — 多轮时引用的 `taskId` 不存在

---

### GET /tasks/{taskId}

查询指定任务的当前状态。

**认证**: 不需要

**路径参数**

| 参数 | 类型 | 说明 |
|------|------|------|
| `taskId` | `string` | 任务 ID（服务端生成的 UUID） |

**成功响应 (200 OK)**

返回当前任务的 `Task` 对象快照。

**错误响应**

- `404 Not Found` — 任务不存在
  ```json
  { "error": { "code": "TaskNotFoundError", "message": "Task xxx not found" } }
  ```

---

### POST /tasks/{taskId}/cancel

取消指定任务。

**认证**: 不需要

**路径参数**

| 参数 | 类型 | 说明 |
|------|------|------|
| `taskId` | `string` | 任务 ID |

**请求体** (可选)

```json
{
  "metadata": { "reason": "user request" }
}
```

**成功响应 (200 OK)**

返回更新后的 `Task` 对象，状态为 `canceled`。

**错误响应**

- `404 Not Found` — 任务不存在
- `409 Conflict` — 任务已处于终端状态，无法取消
  ```json
  { "error": { "code": "TaskNotCancelableError", "message": "Task xxx is already in a terminal state" } }
  ```

**说明**: 当前实现为软取消。任务状态会被标记为 `canceled`，但 Agent 的后台执行线程可能仍会继续运行直到自然结束（到达 turn 限制）。SSE 流会停止推送新事件。

---

### GET /tasks

列出任务，支持过滤和分页。

**认证**: 不需要

**查询参数 (ListTasksRequest)**

| 字段 | 类型 | 说明 |
|------|------|------|
| `contextId` | `string` | 按上下文 ID 过滤 |
| `status` | `TaskState` | 按状态过滤 |
| `statusTimestampAfter` | `string` | 只返回该时间之后更新的任务 |
| `pageSize` | `number` | 每页任务数（默认 20） |
| `historyLength` | `number` | 返回的历史消息数量限制 |
| `includeArtifacts` | `boolean` | 是否包含产出物 |
| `pageToken` | `string` | 分页令牌 |

**成功响应 (200 OK)**

```json
{
  "tasks": [ ... ],
  "pageSize": 20,
  "totalSize": 1,
  "nextPageToken": ""
}
```

---

### POST /tasks/{taskId}/subscribe

订阅任务的实时更新流。

**认证**: 不需要

**路径参数**

| 参数 | 类型 | 说明 |
|------|------|------|
| `taskId` | `string` | 任务 ID |

**响应**: `Content-Type: text/event-stream`

流的第一个事件是当前 `Task` 快照。

**错误响应**

- `404 Not Found` — 任务不存在
- `405 Method Not Allowed` — 任务已处于终端状态，或服务器不支持流式订阅

---

### GET /extendedAgentCard

获取扩展 Agent 信息卡。

**认证**: 不需要

**成功响应 (200 OK)**

返回 `ExtendedAgentCard` JSON（包含基础 AgentCard + provider 信息）。

**错误响应**

- `405 Method Not Allowed` — 当前 Agent 未启用扩展信息卡
  ```json
  { "error": { "code": "UnsupportedOperationError", "message": "Extended agent card is not supported" } }
  ```

---

### Push Notification 端点

当前版本**不支持**推送通知。以下端点均返回 `PushNotificationNotSupportedError`：

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/tasks/{taskId}/pushNotificationConfigs` | 创建推送配置 |
| `GET` | `/tasks/{taskId}/pushNotificationConfigs` | 列出推送配置 |
| `GET` | `/tasks/{taskId}/pushNotificationConfigs/{configId}` | 获取推送配置 |
| `DELETE` | `/tasks/{taskId}/pushNotificationConfigs/{configId}` | 删除推送配置 |

---

## 错误处理

### 通用错误响应格式

```json
{
  "error": {
    "code": "ErrorCode",
    "message": "人类可读的错误描述",
    "details": null
  }
}
```

### 标准错误码列表

| HTTP 状态 | 错误码 | 说明 | 常见场景 |
|-----------|--------|------|----------|
| `400` | `ContentTypeNotSupportedError` | 不支持的 Part 类型 | 使用了 `data` part 或 `file` raw bytes |
| `400` | `InvalidRequestError` | 请求参数错误 | `contextId` 与任务不匹配 |
| `400` | `VersionNotSupportedError` | A2A 版本不支持 | `A2A-Version` 头不是 `1.0` |
| `400` | `PushNotificationNotSupportedError` | 推送通知不支持 | 访问推送通知端点 |
| `404` | `TaskNotFoundError` | 任务不存在 | 查询/跟进/取消不存在的任务 |
| `405` | `UnsupportedOperationError` | 不支持的操作 | 向终端状态任务发送跟进、获取未启用的扩展卡 |
| `409` | `TaskNotCancelableError` | 任务不可取消 | 取消已完成的终端状态任务 |
| `500` | — | 内部服务器错误 | 未预料到的异常 |

### Agent 执行错误

Agent 执行过程中的错误（如工具调用失败）**不会**以 HTTP 错误返回。而是将任务状态设置为 `failed`，错误信息放在 `status.message` 中，同时将错误文本加入 `history`。

```json
{
  "id": "task-001",
  "status": {
    "state": "failed",
    "message": {
      "messageId": "err-1",
      "role": "agent",
      "parts": [{ "text": "Tool execution error: ..." }]
    },
    "timestamp": "2026-04-20T10:00:00.000Z"
  },
  "history": [ ... ]
}
```

---

## 代码示例

### cURL

#### 获取 Agent Card

```bash
curl -s http://localhost:3001/.well-known/agent.json | jq .
```

#### 同步发送消息（新任务）

```bash
curl -X POST http://localhost:3001/message:send \
  -H "Content-Type: application/json" \
  -H "A2A-Version: 1.0" \
  -d '{
    "message": {
      "messageId": "msg-hello-001",
      "role": "user",
      "parts": [{ "text": "你好，请介绍你自己" }]
    }
  }'
```

#### 流式发送消息 (SSE)

```bash
curl -N -X POST http://localhost:3001/message:stream \
  -H "Content-Type: application/json" \
  -H "A2A-Version: 1.0" \
  -H "Accept: text/event-stream" \
  -d '{
    "message": {
      "messageId": "msg-stream-001",
      "role": "user",
      "parts": [{ "text": "写一个快速排序" }]
    }
  }'
```

#### 多轮对话跟进

```bash
# 第1轮：创建任务，服务端返回 taskId 和 contextId
curl -X POST http://localhost:3001/message:send \
  -H "Content-Type: application/json" \
  -H "A2A-Version: 1.0" \
  -d '{
    "message": {
      "messageId": "msg-1",
      "role": "user",
      "parts": [{ "text": "写一个快速排序" }]
    }
  }'

# 第2轮：跟进，在 message.taskId 中引用已有任务
curl -X POST http://localhost:3001/message:send \
  -H "Content-Type: application/json" \
  -H "A2A-Version: 1.0" \
  -d '{
    "message": {
      "messageId": "msg-2",
      "taskId": "550e8400-e29b-41d4-a716-446655440000",
      "role": "user",
      "parts": [{ "text": "用 Rust 写" }]
    }
  }'
```

#### 取消任务

```bash
curl -X POST http://localhost:3001/tasks/550e8400-e29b-41d4-a716-446655440000/cancel \
  -H "A2A-Version: 1.0"
```

#### 查询任务状态

```bash
curl -s http://localhost:3001/tasks/550e8400-e29b-41d4-a716-446655440000 \
  -H "A2A-Version: 1.0" | jq .
```

#### 列出任务

```bash
curl -G http://localhost:3001/tasks \
  -H "A2A-Version: 1.0"
```

### JavaScript / TypeScript

```typescript
const BASE_URL = "http://localhost:3001";
const HEADERS = {
  "Content-Type": "application/json",
  "A2A-Version": "1.0"
};

// 1. 获取 Agent Card
async function getAgentCard() {
  const res = await fetch(`${BASE_URL}/.well-known/agent.json`);
  return res.json();
}

// 2. 同步发送消息（新任务）
async function sendMessage(messageId: string, text: string) {
  const res = await fetch(`${BASE_URL}/message:send`, {
    method: "POST",
    headers: HEADERS,
    body: JSON.stringify({
      message: {
        messageId,
        role: "user",
        parts: [{ text }]
      }
    })
  });
  return res.json();
}

// 3. 多轮对话跟进
async function sendFollowup(messageId: string, taskId: string, text: string) {
  const res = await fetch(`${BASE_URL}/message:send`, {
    method: "POST",
    headers: HEADERS,
    body: JSON.stringify({
      message: {
        messageId,
        taskId,
        role: "user",
        parts: [{ text }]
      }
    })
  });
  return res.json();
}

// 4. 流式发送消息 (SSE)
async function sendMessageStream(messageId: string, text: string, taskId?: string) {
  const message: any = {
    messageId,
    role: "user",
    parts: [{ text }]
  };
  if (taskId) message.taskId = taskId;

  const res = await fetch(`${BASE_URL}/message:stream`, {
    method: "POST",
    headers: HEADERS,
    body: JSON.stringify({ message })
  });

  const reader = res.body!.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });

    const lines = buffer.split("\n");
    buffer = lines.pop() || "";

    for (const line of lines) {
      if (line.startsWith("data:")) {
        const data = JSON.parse(line.slice(5).trim());
        console.log("Data:", data);
        if (["completed","failed","canceled"].includes(data.statusUpdate?.state)) console.log("--- Stream End ---");
      }
    }
  }
}

// 5. 取消任务
async function cancelTask(taskId: string) {
  const res = await fetch(`${BASE_URL}/tasks/${taskId}/cancel`, {
    method: "POST",
    headers: { "A2A-Version": "1.0" }
  });
  return res.json();
}
```

### Python

```python
import json
import requests

BASE_URL = "http://localhost:3001"
HEADERS = {
    "Content-Type": "application/json",
    "A2A-Version": "1.0"
}

# 1. 获取 Agent Card
def get_agent_card():
    resp = requests.get(f"{BASE_URL}/.well-known/agent.json")
    resp.raise_for_status()
    return resp.json()

# 2. 同步发送消息（新任务）
def send_message(message_id: str, text: str):
    resp = requests.post(
        f"{BASE_URL}/message:send",
        headers=HEADERS,
        json={
            "message": {
                "messageId": message_id,
                "role": "user",
                "parts": [{"text": text}]
            }
        }
    )
    resp.raise_for_status()
    return resp.json()

# 3. 多轮对话跟进
def send_followup(message_id: str, task_id: str, text: str):
    resp = requests.post(
        f"{BASE_URL}/message:send",
        headers=HEADERS,
        json={
            "message": {
                "messageId": message_id,
                "taskId": task_id,
                "role": "user",
                "parts": [{"text": text}]
            }
        }
    )
    resp.raise_for_status()
    return resp.json()

# 4. 流式发送消息 (SSE)
def send_message_stream(message_id: str, text: str, task_id: str = None):
    payload = {
        "message": {
            "messageId": message_id,
            "role": "user",
            "parts": [{"text": text}]
        }
    }
    if task_id:
        payload["message"]["taskId"] = task_id

    resp = requests.post(
        f"{BASE_URL}/message:stream",
        headers=HEADERS,
        json=payload,
        stream=True
    )
    resp.raise_for_status()

    for line in resp.iter_lines(decode_unicode=True):
        if line.startswith("data:"):
            data = json.loads(line[5:].strip())
            print(f"Data: {data}")
            if data.get("statusUpdate", {}).get("state") in ("completed", "failed", "canceled"):
                print("--- Stream End ---")

# 5. 取消任务
def cancel_task(task_id: str):
    resp = requests.post(
        f"{BASE_URL}/tasks/{task_id}/cancel",
        headers={"A2A-Version": "1.0"}
    )
    resp.raise_for_status()
    return resp.json()

# 6. 查询任务
def get_task(task_id: str):
    resp = requests.get(
        f"{BASE_URL}/tasks/{task_id}",
        headers={"A2A-Version": "1.0"}
    )
    resp.raise_for_status()
    return resp.json()

# 7. 列出任务
def list_tasks():
    resp = requests.get(
        f"{BASE_URL}/tasks",
        headers={"A2A-Version": "1.0"}
    )
    resp.raise_for_status()
    return resp.json()
```

---

## 限制与注意事项

1. **Task ID 服务端生成**: 创建新任务时，**不要**在请求中提供 `taskId`。服务端会自动生成 UUID 并返回。
2. **多轮对话**: 在 `message.taskId` 中传入已有任务 ID 即可复用上下文。**不要**自己发送历史消息。
3. **无持久化**: 所有任务状态保存在内存中（`DashMap`）。服务重启后所有任务丢失。
4. **无认证 (MVP)**: 当前版本无需认证即可访问所有端点。
5. **Part 类型限制**:
   - `file` 类型的 `raw` 字段（内联 Base64 字节）暂不支持，仅支持通过 `url` 引用文件。
   - `data` 类型暂不支持。
6. **状态限制**: `input-required` 状态在 MVP 中未实现，多轮对话请使用 `message.taskId` 复用机制。
7. **取消机制**: 软取消。任务状态会被标记为 `canceled`，但后台执行可能仍会继续一小段时间。
8. **跨域**: 所有端点均开启全 permissive CORS，允许任意来源访问。
9. **取消路径差异**: 由于 Axum 0.8 路由限制，取消端点使用 `/tasks/{taskId}/cancel` 而非官方规范的 `/tasks/{taskId}:cancel`（功能完全等价）。
10. **超时**: 同步任务有执行时间限制（以 Agent 核心配置为准）。
