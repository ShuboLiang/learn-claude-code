# A2A 协议 v1.0 接口详解

> 来源：基于 Tavily 深度搜索整理，覆盖 A2A Protocol 官方规范、GitHub 仓库 CHANGELOG、官方博客及社区文档。
> **最新稳定版本：v1.0（2026年3月12日发布）**

---

## 一、版本历史

| 版本 | 发布日期 | 说明 |
|------|----------|------|
| v0.1 | 2025年4月 | Google 首次发布 |
| v0.2.x | 2025年5-7月 | 快速迭代，多次 Breaking Changes |
| v0.3 | 2025年7月30日 | 引入 gRPC、Agent Card 签名 |
| **v1.0** | **2026年3月12日** | **首个稳定生产版本** |

v1.0 强调**成熟化而非重新发明**：核心思想保持不变，但去除了粗糙边缘、澄清了模糊区域、更直接地满足企业部署需求。

---

## 二、协议概述

**A2A (Agent-to-Agent)** 是 Google 发起并捐赠给 Linux Foundation 的开放协议，旨在让不同框架、不同厂商构建的 AI Agent 能够安全地互相发现、通信和协作。

### 核心特点
- **传输层**：JSON-RPC over HTTP、**gRPC**、HTTP+JSON/REST
- **安全默认**：强制 TLS 1.2+，支持 OAuth2、mTLS、API Key、OpenID Connect
- **多模态**：支持文本、音频、视频、文件等多种内容类型
- **长任务支持**：SSE 流式传输 + Webhook 推送通知
- **黑盒互操作**：客户端无需了解远程 Agent 内部实现

### 与 MCP 的关系
- **A2A**：水平集成层，Agent 之间的点对点协作
- **MCP**：垂直集成层，Agent 连接外部工具和服务
- 两者互补

---

## 三、三层模型（v1.0 规范架构）

### Layer 1: 规范数据模型
核心实体：
- `AgentCard` — Agent 名片
- `Task` — 任务
- `Message` — 消息
- `Artifact` — 产物
- `Part` — 内容部分

### Layer 2: 抽象操作
核心操作集：
- `SendMessage`
- `SendStreamingMessage`
- `GetTask`
- `ListTasks`
- `CancelTask`
- `SubscribeToTask`
- Push Notification Config 操作
- `GetExtendedAgentCard`

### Layer 3: 协议绑定
将抽象操作映射到具体传输：
- JSON-RPC over HTTP
- gRPC
- HTTP+JSON/REST

---

## 四、核心抽象

### 1. Agent Card（Agent 名片）

自描述 JSON 元数据文档，发布在 `/.well-known/agent.json`，描述 Agent 的身份、能力、技能和认证要求。

**v1.0 变化：**
- Agent Card 以**向后兼容**的方式演进
- Agent 可同时宣告支持 v0.3 和 v1.0，实现渐进式迁移
- 支持 **Authenticated Extended Agent Card**（认证后返回更多详情）

#### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `protocolVersion` | string | 支持的 A2A 协议版本，如 `"1.0.0"` |
| `name` | string | Agent 可读名称 |
| `description` | string | Agent 功能描述 |
| `url` | string | 服务端点 URL |
| `preferredTransport` | string | 首选传输协议 |
| `supportedInterfaces` | array | 支持的接口列表 |
| `provider` | object | 提供者信息 |
| `version` | string | Agent 实现版本 |
| `defaultInputModes` | string[] | 默认输入 MIME 类型 |
| `defaultOutputModes` | string[] | 默认输出 MIME 类型 |
| `capabilities` | object | 能力声明 |
| `skills` | array | 技能列表 |
| `securitySchemes` | object | 安全方案定义 |
| `security` | array | 安全要求组合 |
| `supportsAuthenticatedExtendedCard` | boolean | 是否支持认证后扩展 Card |

#### AgentSkill 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 技能唯一标识 |
| `name` | string | 技能名称 |
| `description` | string | 技能描述 |
| `tags` | string[] | 标签 |
| `examples` | string[] | 示例查询 |
| `inputModes` | string[] | 输入 MIME 类型 |
| `outputModes` | string[] | 输出 MIME 类型 |

#### Agent Card 示例

```json
{
  "protocolVersion": "1.0.0",
  "name": "Flight Booking Agent",
  "description": "Agent that helps users book flights.",
  "url": "https://booking.example.com/a2a",
  "preferredTransport": "JSONRPC",
  "supportedInterfaces": [
    {
      "url": "https://booking.example.com/a2a",
      "protocolBinding": "JSONRPC"
    },
    {
      "url": "https://booking.example.com/grpc",
      "protocolBinding": "GRPC"
    }
  ],
  "provider": {
    "organization": "Example Corp",
    "url": "https://example.com"
  },
  "version": "1.2.0",
  "defaultInputModes": ["text/plain", "application/json"],
  "defaultOutputModes": ["text/plain", "application/json"],
  "capabilities": {
    "streaming": true,
    "pushNotifications": true
  },
  "skills": [
    {
      "id": "book-flight",
      "name": "Flight Booking",
      "description": "Book flights between destinations",
      "tags": ["travel", "booking"],
      "examples": ["Book a flight from NYC to LA"]
    }
  ],
  "securitySchemes": {
    "oauth2": {
      "type": "oauth2",
      "flows": {
        "clientCredentials": {
          "tokenUrl": "https://auth.example.com/token",
          "scopes": {
            "booking:write": "Book flights"
          }
        }
      }
    }
  },
  "security": [
    {"oauth2": ["booking:write"]}
  ],
  "supportsAuthenticatedExtendedCard": false
}
```

---

### 2. Task（任务）

核心工作单元，由客户端创建，由远程 Agent 管理状态。

#### Task 生命周期状态

| 状态 | 说明 |
|------|------|
| `submitted` | 任务已提交 |
| `working` | 远程 Agent 正在处理 |
| `input-required` | 需要客户端提供更多信息 |
| `completed` | 任务已完成 |
| `canceled` | 任务已取消 |
| `failed` | 任务失败 |
| `unknown` | 未知状态 |

#### Task 关键字段（v1.0 新增）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 任务唯一 ID（**服务端生成**） |
| `sessionId` | string | 会话 ID |
| `status` | TaskStatus | 当前状态 |
| `artifacts` | Artifact[] | 任务产出 |
| `history` | Message[] | 消息历史 |
| `createdAt` | timestamp | **v1.0 新增**：创建时间 |
| `lastModified` | timestamp | **v1.0 新增**：最后修改时间 |
| `extensions` | Extension[] | **v1.0 新增**：扩展数组 |
| `metadata` | object | 附加元数据 |

**v1.0 重要变化：**
- 任务 ID **必须由服务端生成**，客户端不应自行创建任务 ID
- 新增 `createdAt` 和 `lastModified` 时间戳
- 新增 `extensions[]` 数组支持扩展

---

### 3. Message（消息）

通信原子单元。

| 字段 | 类型 | 说明 |
|------|------|------|
| `messageId` | string | 消息唯一 ID |
| `role` | string | `"user"` 或 `"agent"` |
| `parts` | Part[] | 消息内容 |
| `taskId` | string | 关联任务 ID |
| `contextId` | string | 上下文 ID |
| `metadata` | object | 附加元数据 |

#### Part 类型

| 类型 | 说明 |
|------|------|
| `TextPart` | 文本 `{ "kind": "text", "text": "..." }` |
| `FilePart` | 文件 `{ "kind": "file", "file": { "name": "...", "mimeType": "...", "bytes": "base64..." } }` |
| `DataPart` | 结构化 JSON `{ "kind": "data", "data": { ... } }` |

---

### 4. Artifact（产物）

任务执行的最终输出。

| 字段 | 类型 | 说明 |
|------|------|------|
| `artifactId` | string | 产物 ID |
| `name` | string | 产物名称 |
| `parts` | Part[] | 产物内容 |
| `metadata` | object | 附加元数据 |

---

## 五、v1.0 API 方法

### 方法对照表（v0.3 → v1.0）

| v0.3 名称 | v1.0 名称 | 传输绑定 | 说明 |
|-----------|-----------|----------|------|
| `message/send` | `SendMessage` | JSON-RPC: `POST` / gRPC: `SendMessage` / REST: `POST /v1/message:send` | 发送消息（同步） |
| `message/stream` | `SendStreamingMessage` | JSON-RPC: SSE / gRPC: `SendStreamingMessage` / REST: `POST /v1/message:stream` | 流式发送消息 |
| `tasks/get` | `GetTask` | JSON-RPC: `POST` / gRPC: `GetTask` / REST: `GET /v1/tasks/{id}` | 获取任务状态 |
| — | `ListTasks` | JSON-RPC: `POST` / gRPC: `ListTasks` / REST: `GET /v1/tasks` | **v1.0 新增**：列出任务 |
| `tasks/cancel` | `CancelTask` | JSON-RPC: `POST` / gRPC: `CancelTask` / REST: `POST /v1/tasks/{id}:cancel` | 取消任务 |
| `tasks/resubscribe` | `SubscribeToTask` | JSON-RPC: SSE / gRPC: `SubscribeToTask` / REST: `POST /v1/tasks/{id}:subscribe` | 重新订阅任务流 |
| `tasks/pushNotificationConfig/set` | `CreateTaskPushNotificationConfig` | — | 设置推送通知配置 |
| — | `GetExtendedAgentCard` | — | 获取认证后的扩展 Agent Card |

---

### 1. `SendMessage` — 发送消息（同步）

**v1.0 变化：**
- ✅ 操作名从 `message/send` 重命名为 `SendMessage`
- ✅ 更精确地规范了 Task vs Message 返回语义

**JSON-RPC 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": "req-001",
  "method": "SendMessage",
  "params": {
    "message": {
      "role": "user",
      "parts": [
        { "kind": "text", "text": "Book a flight from NYC to LA tomorrow" }
      ]
    },
    "sessionId": "session-123",
    "metadata": {}
  }
}
```

**响应（任务完成）：**
```json
{
  "jsonrpc": "2.0",
  "id": "req-001",
  "result": {
    "task": {
      "id": "task-abc-123",
      "status": {
        "state": "completed",
        "message": {
          "role": "agent",
          "parts": [{ "kind": "text", "text": "Flight booked successfully!" }]
        }
      },
      "artifacts": [
        {
          "artifactId": "art-001",
          "name": "booking-confirmation",
          "parts": [
            { "kind": "data", "data": { "flightNumber": "AA123", "price": 299 } }
          ]
        }
      ],
      "createdAt": "2026-03-12T10:00:00Z",
      "lastModified": "2026-03-12T10:05:00Z"
    }
  }
}
```

---

### 2. `SendStreamingMessage` — 流式发送消息

**v1.0 Breaking Changes：**
- ✅ 操作名从 `message/stream` 重命名为 `SendStreamingMessage`
- ⚠️ **流事件不再使用 `kind` 字段区分类型**
  - 使用 JSON 成员名称来区分 `TaskStatusUpdateEvent` 和 `TaskArtifactUpdateEvent`
- ⚠️ **移除了 `final` 布尔字段**
  - 利用协议绑定特定的流关闭机制来检测完成
- ✅ 允许多个并发流；所有流接收相同的有序事件

**SSE 响应流（v1.0）：**
```
event: taskStatusUpdate
data: {"jsonrpc":"2.0","result":{"taskStatusUpdate":{"taskId":"task-xyz","contextId":"ctx-123","status":{"state":"working"}}}}

event: taskArtifactUpdate
data: {"jsonrpc":"2.0","result":{"taskArtifactUpdate":{"taskId":"task-xyz","contextId":"ctx-123","artifact":{"artifactId":"art-001","parts":[{"kind":"text","text":"Section 1: Overview..."}]},"index":0}}}

event: taskStatusUpdate
data: {"jsonrpc":"2.0","result":{"taskStatusUpdate":{"taskId":"task-xyz","contextId":"ctx-123","status":{"state":"completed"}}}}
```

**注意：** v1.0 中通过 `taskStatusUpdate` / `taskArtifactUpdate` 包装对象来区分事件类型，不再依赖 `kind` 字段。

---

### 3. `GetTask` — 获取任务状态

**v1.0 变化：**
- ✅ 操作名从 `tasks/get` 重命名为 `GetTask`
- ✅ 新增 `createdAt` 和 `lastModified` 时间戳字段
- ✅ 更精确地规范了历史记录包含行为
- ✅ Task 对象新增 `extensions[]` 数组
- ✅ 明确认证/授权范围：服务端**必须**只返回调用者可见的任务

**JSON-RPC 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": "req-003",
  "method": "GetTask",
  "params": {
    "id": "task-abc-123",
    "historyLength": 10
  }
}
```

---

### 4. `ListTasks` — 列出任务

**v1.0 新增方法**

v0.3 中不存在此操作。v1.0 新增用于分页查询任务列表。

- 支持分页（`pageToken`、`pageSize`）
- 支持多租户（`tenant` 字段）

---

### 5. `CancelTask` — 取消任务

**v1.0 变化：**
- ✅ 操作名从 `tasks/cancel` 重命名为 `CancelTask`

**JSON-RPC 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": "req-004",
  "method": "CancelTask",
  "params": {
    "id": "task-abc-123"
  }
}
```

---

### 6. `SubscribeToTask` — 订阅任务更新

**v1.0 变化：**
- ✅ 操作名从 `tasks/resubscribe` 重命名为 `SubscribeToTask`
- 用于恢复丢失的 SSE 连接，继续接收任务实时更新

**JSON-RPC 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": "req-005",
  "method": "SubscribeToTask",
  "params": {
    "id": "task-abc-123",
    "historyLength": 10
  }
}
```

---

## 六、v1.0 主要 Breaking Changes 汇总

### 1. 操作重命名
所有操作从 `snake_case` 风格（如 `tasks/send`）改为 **PascalCase** 风格（如 `SendMessage`）。

### 2. 流事件格式变更
- 移除 `kind` 字段
- 使用包装对象名称区分事件类型（`taskStatusUpdate` / `taskArtifactUpdate`）
- 新增 `index` 字段表示产物在数组中的位置

### 3. 移除 `final` 字段
- `TaskStatusUpdateEvent` 中的 `final` 布尔字段被移除
- 流完成通过**协议绑定的流关闭机制**检测

### 4. Task 新增字段
- `createdAt` / `lastModified` 时间戳
- `extensions[]` 扩展数组

### 5. OAuth 2.0 安全更新
- 移除已弃用的流程：
  - ❌ `ImplicitOAuthFlow`（因令牌泄露风险）
  - ❌ `PasswordOAuthFlow`（因凭证暴露风险）
- 对齐 OAuth 2.0 Security BCP（最佳当前实践）

### 6. 推送通知配置重构
- `PushNotificationConfig` → `TaskPushNotificationConfig`
- 相关操作重命名

### 7. Agent Card 字段变更
- `supportsAuthenticatedExtendedCard` / `supportsExtendedAgentCard` 相关调整
- `AgentCapabilities` 重构

### 8. 多租户支持（v1.0 新增）
- `ListTasks` 等操作支持 `tenant` 字段

---

## 七、传输方式对比

| 特性 | JSON-RPC over HTTP | gRPC | REST |
|------|-------------------|------|------|
| 同步请求 | ✅ POST | ✅ Unary | ✅ |
| 流式响应 | ✅ SSE | ✅ Server Streaming | ✅ SSE |
| 双向流式 | ❌ | ✅ Bidirectional | ❌ |
| 推送通知 | ✅ Webhook | ✅ | ✅ Webhook |
| 性能 | 中等 | 高 | 中等 |
| 易用性 | 高 | 中等 | 高 |

---

## 八、安全机制

### 1. 传输安全
- 所有通信必须通过 HTTPS（TLS 1.2+）

### 2. 认证
- **OAuth2**：`Authorization: Bearer <token>`
- **API Key**：`X-API-Key: <key>`
- **mTLS**：客户端证书
- **OpenID Connect**：标准 OIDC 流程

### 3. Agent Card 签名
- 支持 JWS（JSON Web Signature）数字签名
- v1.0 中签名能力更完善

### 4. 授权
- 基于身份的最小权限原则
- 按技能粒度控制访问
- 认证/授权范围明确：服务端只返回调用者可见的任务

---

## 九、错误处理

标准 JSON-RPC 错误码 + A2A 特定错误码：

| 错误码 | 说明 |
|--------|------|
| `-32700` | Parse error |
| `-32600` | Invalid Request |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal error |
| `-32000` ~ `-32009` | A2A 协议特定错误 |

---

## 十、官方 SDK 支持

| 语言 | SDK 状态 |
|------|----------|
| Python | ✅ 官方支持 |
| JavaScript/TypeScript | ✅ 官方支持 |
| Java | ✅ 官方支持（Quarkus 集成） |
| C#/.NET | ✅ 官方支持 |
| Go | ✅ 官方支持 |

**v1.0 SDK 特性：**
- 官方 SDK 确保 v1.0 Agent 与旧版本无缝兼容
- 提供 `a2acompat` 兼容包支持向后兼容

---

## 十一、迁移指南（v0.3 → v1.0）

### 迁移检查清单

1. **盘点旧假设**
   - 检查流式完成逻辑中是否依赖 `final` 字段
   - 检查是否使用 `kind` 字段区分事件类型

2. **更新操作映射**
   - 将所有 `tasks/send` 改为 `SendMessage`
   - 将所有 `message/stream` 改为 `SendStreamingMessage`
   - 将所有 `tasks/get` 改为 `GetTask`
   - 将所有 `tasks/cancel` 改为 `CancelTask`
   - 将所有 `tasks/resubscribe` 改为 `SubscribeToTask`

3. **更新流事件处理**
   - 移除 `kind` 字段判断逻辑
   - 改用 `taskStatusUpdate` / `taskArtifactUpdate` 包装对象名称判断
   - 使用流关闭机制替代 `final` 字段检测完成

4. **重新生成 Schema/SDK 契约**
   - 从最新的 protobuf 定义生成

5. **端到端测试**
   - 轮询、流式、推送通知流程

6. **文档兼容性保证**
   - 为客户端按版本记录兼容性策略

### Agent Card 兼容性
- v1.0 Agent Card **向后兼容**
- Agent 可同时宣告支持 v0.3 和 v1.0
- 客户端可渐进式迁移，无需一次性切换

---

## 十二、关键资源

- **官方文档**：https://a2a-protocol.org/
- **v1.0 发布公告**：https://a2a-protocol.org/latest/announcing-1.0/
- **v1.0 变更详情**：https://a2a-protocol.org/latest/whats-new-v1/
- **GitHub 仓库**：https://github.com/a2aproject/A2A
- **CHANGELOG**：https://github.com/a2aproject/A2A/blob/main/CHANGELOG.md
- **社区文档**：https://agent2agent.info/

---

*文档整理时间：2025年*
