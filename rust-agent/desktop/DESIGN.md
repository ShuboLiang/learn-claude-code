# rust-agent 桌面应用设计方案

> 为 `rust-agent/cli` 构建跨平台桌面客户端的设计草案。

---

## 1. 现状分析

| 组件 | 技术栈 | 说明 |
|---|---|---|
| CLI | Ink (React for Terminal) + TS | 当前交互入口，功能完整但限于终端 |
| Server | Rust (axum) + SSE | 提供 `/sessions`、`/messages` 等流式 API |
| Web | Next.js 16 + React 19 + Tailwind 4 | 现有文档/教程站点，**无聊天 UI** |

后端 API 已就绪：
- `POST /sessions` — 创建会话
- `POST /sessions/:id/messages` — SSE 流式对话
- `POST /sessions/:id/clear` — 清空上下文
- `DELETE /sessions/:id` — 删除会话

---

## 2. 方案对比

### 方案 A：Tauri + React（推荐）

**架构**
```
┌──────────────────────────────┐
│ Tauri Desktop App            │
│ ┌────────────────────────┐   │
│ │ React Frontend         │   │
│ │ (WebView2 / WKWebView) │   │
│ └────────────────────────┘   │
│           ↑ Tauri IPC / HTTP │
│ ┌────────────────────────┐   │
│ │ Rust Core (Tauri)      │   │
│ │ - 窗口/托盘/快捷键管理  │   │
│ │ - 可选：内嵌 server     │   │
│ └────────────────────────┘   │
│           ↑ HTTP             │
│ ┌────────────────────────┐   │
│ │ rust-agent-server      │   │
│ │ (子进程 or 内嵌 crate)  │   │
│ └────────────────────────┘   │
└──────────────────────────────┘
```

**优点**
- 包体积极小（Windows ~5MB，含 WebView2 依赖由系统提供）
- 启动速度快，内存占用低
- 与项目 Rust 后端天然契合，可直接 `cargo` 内嵌 server
- 前端用 React + Tailwind，与现有 `web/` 技术栈一致
- 原生能力（系统托盘、全局快捷键、本地文件访问）通过 Tauri API 即可实现

**缺点**
- Tauri v2 相对 Electron 生态较小（但足够成熟）
- WebView 渲染一致性需测试（不同系统 WebView 内核差异）

---

### 方案 B：Electron + Next.js

**优点**
- 生态最成熟，文档和插件丰富
- Chromium 内核，前端兼容性最好
- 可直接复用 Next.js 构建产物

**缺点**
- 包体积大（>150MB），启动慢
- 内存占用高（每个窗口独立 Chromium）
- 和项目 Rust 后端只能通过 HTTP/子进程通信，集成感弱

---

### 方案 C：纯 Web（PWA）

**优点**
- 零额外成本，浏览器即可运行
- 更新最灵活

**缺点**
- 无法真正"桌面化"（无系统托盘、无全局快捷键、无本地文件系统）
- 需用户手动启动后端 server

---

## 3. 推荐方案：Tauri + React（方案 A）

### 3.1 目录结构

```
rust-agent/desktop/
├── src/                    # React 前端源码
│   ├── main.tsx            # 入口
│   ├── App.tsx             # 根组件
│   ├── components/
│   │   ├── Chat.tsx        # 对话主区域
│   │   ├── MessageList.tsx # 消息列表（用户/助手/工具）
│   │   ├── MessageItem.tsx # 单条消息渲染（Markdown + 代码高亮）
│   │   ├── InputBox.tsx    # 输入框（支持多行、Shift+Enter）
│   │   ├── Sidebar.tsx     # 会话列表侧边栏
│   │   ├── SessionItem.tsx # 单个会话项
│   │   └── Settings.tsx    # 设置面板
│   ├── hooks/
│   │   ├── useSse.ts       # SSE 流式对话 hook
│   │   ├── useSessions.ts  # 会话管理 hook
│   │   └── useSettings.ts  # 本地配置持久化
│   └── api/
│       └── client.ts       # 后端 HTTP API 封装（复用 cli/src/api.ts 逻辑）
├── src-tauri/              # Tauri Rust 侧
│   ├── src/
│   │   ├── main.rs         # 入口
│   │   ├── lib.rs
│   │   └── server.rs       # server 子进程管理（可选）
│   ├── Cargo.toml
│   └── tauri.conf.json
├── package.json
├── vite.config.ts          # Vite 构建（Tauri 推荐）
└── tailwind.config.ts
```

### 3.2 启动模式（二选一）

| 模式 | 说明 | 适用场景 |
|---|---|---|
| **子进程模式** | Tauri 启动时 `spawn` 后端 `rust-agent-server` 可执行文件，自动分配端口 | 独立发布，用户无感知 |
| **内嵌模式** | 把 `crates/server` 和 `crates/core` 作为 Tauri 的 Rust 依赖，直接调用 axum Router | 深度集成，单进程，共享配置 |

**建议先实现「子进程模式」** —— 复用 CLI 的 `index.tsx` 启动逻辑，快速可用，后期可迁移到内嵌模式。

### 3.3 核心功能

#### 对话界面
- 左侧边栏：会话列表（新建、切换、删除、重命名）
- 右侧主区：
  - 消息气泡（用户 / 助手 / 工具调用 / 工具结果 / 系统提示）
  - Markdown 渲染 + 代码块语法高亮 + 复制按钮
  - 流式打字机效果（SSE `text_delta`）
  - 输入框：支持多行（Shift+Enter）、发送按钮、ESC 中断

#### 全局交互
- **系统托盘**：右键菜单（显示窗口 / 隐藏 / 退出）
- **全局快捷键**：如 `Ctrl+Shift+Space` 呼出/隐藏悬浮窗
- **本地存储**：
  - 会话历史（`tauri::api::path::app_local_data_dir`）
  - 用户设置（API Key、模型、主题）

#### 设置面板
- LLM Provider / API Key / Base URL / Model
- 主题切换（Light / Dark / System）
- 快捷键自定义
- 数据管理（导出/清空历史）

### 3.4 通信设计

```typescript
// src/api/client.ts
// 复用 cli/src/api.ts 的核心逻辑，适配浏览器 fetch + EventSource

class ApiClient {
  private baseUrl: string;
  
  async createSession(): Promise<Session> { /* ... */ }
  
  async *sendMessage(sessionId: string, content: string, signal?: AbortSignal)
    : AsyncGenerator<ServerEvent> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}/messages`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ content }),
      signal,
    });
    // SSE 解析（同 cli/src/api.ts sendMessage）
  }
  
  async clearSession(sessionId: string): Promise<void> { /* ... */ }
}
```

### 3.5 状态管理

推荐 **Zustand**（轻量，无样板）， Store 划分：

```typescript
// stores/sessionStore.ts
interface SessionStore {
  sessions: Session[];
  activeSessionId: string | null;
  messages: Record<string, Message[]>; // sessionId -> messages
  isLoading: boolean;
  abortController: AbortController | null;
  
  createSession: () => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  abort: () => void;
  loadHistory: () => void;
}
```

### 3.6 技术栈

| 层 | 选型 | 理由 |
|---|---|---|
| 桌面框架 | Tauri v2 | 轻量、Rust 原生、跨平台 |
| 前端框架 | React 19 | 与现有 web/ 一致 |
| 构建工具 | Vite 6 | Tauri 官方推荐，极速 HMR |
| 样式 | Tailwind CSS 4 | 与现有 web/ 一致 |
| 组件库 | shadcn/ui 或 Radix | 无样式预设，高度定制 |
| Markdown | react-markdown + highlight.js | 代码高亮 |
| 状态管理 | Zustand | 轻量，适合桌面端 |
| 图标 | Lucide React | 与现有 web/ 一致 |

---

## 4. 开发计划（MVP → 完整）

### Phase 1：MVP（1-2 周）
- [ ] 初始化 Tauri + Vite + React + Tailwind 项目骨架
- [ ] 复用 `cli/src/api.ts` 逻辑，实现 SSE 对话流
- [ ] 基础聊天 UI：消息列表 + 输入框 + 流式输出
- [ ] 子进程模式启动后端 server
- [ ] 支持 ESC 中断、Shift+Enter 多行

### Phase 2：会话管理（1 周）
- [ ] 侧边栏会话列表（新建、切换、删除）
- [ ] 会话标题自动总结（首条消息前 N 字）
- [ ] 本地持久化（SQLite 或 JSON 文件）

### Phase 3： polish（1 周）
- [ ] Markdown 渲染 + 代码高亮 + 复制
- [ ] 系统托盘 + 全局快捷键
- [ ] 设置面板（API Key、模型、主题）
- [ ] 错误处理（401 提示、网络断开）

### Phase 4：进阶（可选）
- [ ] 内嵌 server 模式（替换子进程）
- [ ] 多工作区 / 多 Profile 切换
- [ ] 插件系统（MCP 工具可视化配置）

---

## 5. 与现有代码的复用关系

| 现有代码 | 复用方式 |
|---|---|
| `cli/src/api.ts` | 逻辑直接迁移到 `desktop/src/api/client.ts`（去掉 Ink 依赖，浏览器 fetch 完全兼容） |
| `cli/src/app.tsx` | 状态机逻辑参考（SSE 事件处理、AbortController 中断） |
| `crates/server` | 作为子进程或内嵌 crate 复用，零改动 |
| `web/` | 技术栈对齐（React 19 + Tailwind 4），UI 组件可部分借鉴 |

---

## 6. 决策点

1. **是否采用 Tauri？** 如无特殊偏好（如必须 Electron 插件），推荐 Tauri。
2. **server 启动模式？** 建议先「子进程模式」，快速验证；稳定后考虑「内嵌模式」。
3. **是否复用 `web/` 的 Next.js？** 不建议。Next.js 的 SSR/路由对桌面端是负担，Vite + React 更轻量。
4. **会话历史存在哪里？** 建议 Tauri 的 `app_local_data_dir` + SQLite（via `sqlx` 或 `rusqlite`）。

---

*方案待确认，确认后即可进入开发阶段。*
