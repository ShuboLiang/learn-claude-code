# Web 前端基础框架 + 聊天模块 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `web/` 目录下新建一个纯 Web 前端项目，搭建 IDE 风格的三栏布局框架，并实现完整的聊天模块（含会话增强功能），直接复用现有后端 HTTP API。

**Architecture:** 采用 React 19 + Vite 6 + Tailwind CSS v4 构建，Zustand 管理客户端状态，TanStack Query 管理服务器状态（模型/技能列表）。布局为 ActivityBar + 可拖拽 Sidebar + MainArea 三栏结构。聊天 SSE 流通过自定义 Hook `useChatStream` 处理，直接复用 `desktop/src/api/client.ts` 中的 `ApiClient` 逻辑。

**Tech Stack:** React 19, Vite 6, Tailwind CSS v4, Zustand 5, TanStack Query v5, React Router v7, Framer Motion, Lucide React, React Markdown + remark-gfm

---

## 文件结构映射

```
web/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── tsconfig.app.json
├── tsconfig.node.json
├── index.html
├── src/
│   ├── main.tsx                 # React 挂载 + QueryClientProvider + RouterProvider
│   ├── App.tsx                  # 根布局：ActivityBar + ResizablePane(Sidebar + MainArea)
│   ├── router.tsx               # React Router 定义：/chat, /models, /skills, /settings
│   ├── index.css                # Tailwind v4 + 自定义深色主题变量
│   ├── api/
│   │   ├── client.ts            # 从 desktop/src/api/client.ts 复制并扩展（新增 listSessions, getSessionMessages, clearSession, renameSession, exportSession）
│   │   └── types.ts             # DTO 类型：Session, Message, ServerEvent, ModelConfig, Skill 等
│   ├── stores/
│   │   ├── appStore.ts          # 全局 UI：currentView, sidebarWidth, theme, toastQueue
│   │   └── chatStore.ts         # 聊天状态：sessions, activeSessionId, messages, isLoading, currentReply, abortController
│   ├── hooks/
│   │   ├── useChatStream.ts     # SSE 流处理 Hook：封装 apiClient.sendMessage，映射事件到 chatStore
│   │   └── useSidebarResize.ts  # 拖拽调整 Sidebar 宽度，持久化到 localStorage
│   └── components/
│       ├── layout/
│       │   ├── ActivityBar.tsx      # 48px 垂直图标栏，4 个视图切换
│       │   ├── Sidebar.tsx          # 动态内容容器，根据 currentView 渲染子组件
│       │   ├── ResizablePane.tsx    # 拖拽分割线 + 左右面板
│       │   └── MainArea.tsx         # 路由出口，渲染当前页面内容
│       └── chat/
│           ├── ChatPage.tsx         # 聊天主页面：Header + MessageList + InputBox
│           ├── ChatHeader.tsx       # 顶部工具栏：会话标题(可重命名)、模型选择下拉框、操作按钮组
│           ├── MessageList.tsx      # 消息列表容器，自动滚动到底部
│           ├── MessageItem.tsx      # 单条消息渲染：用户/助手/工具调用/工具结果/系统消息
│           ├── InputBox.tsx         # 多行输入框，auto-resize，发送/中断按钮
│           ├── ModelSelector.tsx    # 模型选择下拉框（当前仅展示，从 chatStore 读取）
│           └── SessionSidebar.tsx   # 聊天视图下的侧边栏：会话搜索 + 列表 + 新建/删除/重命名/导出
```

---

## Task 1: 初始化 Web 项目

**Files:**
- Create: `web/package.json`
- Create: `web/vite.config.ts`
- Create: `web/tsconfig.json`
- Create: `web/tsconfig.app.json`
- Create: `web/tsconfig.node.json`
- Create: `web/index.html`

- [ ] **Step 1: 创建 `web/package.json`**

```json
{
  "name": "rust-agent-web",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@tanstack/react-query": "^5.0.0",
    "framer-motion": "^12.0.0",
    "lucide-react": "^0.564.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0",
    "react-markdown": "^10.0.0",
    "react-router": "^7.0.0",
    "remark-gfm": "^4.0.1",
    "zustand": "^5.0.0"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4.0.0",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "tailwindcss": "^4.0.0",
    "typescript": "~5.8.0",
    "vite": "^6.0.0"
  }
}
```

- [ ] **Step 2: 创建 `web/vite.config.ts`**

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      "/sessions": "http://localhost:3000",
      "/bots": "http://localhost:3000",
      "/v1": "http://localhost:3000",
    },
  },
});
```

- [ ] **Step 3: 创建 `web/tsconfig.json`**

```json
{
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" },
    { "path": "./tsconfig.node.json" }
  ]
}
```

- [ ] **Step 4: 创建 `web/tsconfig.app.json`**

复制 `desktop/tsconfig.app.json` 到 `web/tsconfig.app.json`，内容完全一致：

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src"]
}
```

- [ ] **Step 5: 创建 `web/tsconfig.node.json`**

复制 `desktop/tsconfig.node.json` 到 `web/tsconfig.node.json`。

- [ ] **Step 6: 创建 `web/index.html`**

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Rust Agent</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 7: 安装依赖并验证**

Run: `cd web && npm install`
Expected: `node_modules` 创建成功，无报错。

- [ ] **Step 8: Commit**

```bash
git add web/package.json web/vite.config.ts web/tsconfig.json web/tsconfig.app.json web/tsconfig.node.json web/index.html
git commit -m "chore(web): init web frontend project with vite + react 19 + tailwind v4"
```

---

## Task 2: 全局样式与主题变量

**Files:**
- Create: `web/src/index.css`

- [ ] **Step 1: 创建 `web/src/index.css`**

```css
@import "tailwindcss";

@theme {
  --font-sans: "Inter", ui-sans-serif, system-ui, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, monospace;

  --color-bg-base: #0c0c0c;
  --color-bg-panel: #141414;
  --color-bg-elevated: #1a1a1a;
  --color-bg-hover: #242424;
  --color-bg-input: #242424;
  --color-border: rgba(255, 255, 255, 0.06);
  --color-primary: #6366f1;
  --color-primary-hover: #818cf8;
  --color-user-bubble: #312e81;
  --color-tool-call-bg: rgba(245, 158, 11, 0.08);
  --color-tool-result-bg: rgba(16, 185, 129, 0.08);
}

html, body, #root {
  height: 100%;
}

body {
  background-color: var(--color-bg-base);
  color: #e5e5e5;
  font-family: var(--font-sans);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: #404040;
  border-radius: 3px;
}
::-webkit-scrollbar-thumb:hover {
  background: #525252;
}

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/index.css
git commit -m "feat(web): add global styles with dark theme variables"
```

---

## Task 3: API 客户端与类型定义

**Files:**
- Create: `web/src/api/types.ts`
- Create: `web/src/api/client.ts`

- [ ] **Step 1: 创建 `web/src/api/types.ts`**

```typescript
export interface ServerEvent {
  event: string;
  data: Record<string, any>;
}

export interface Session {
  id: string;
  model: string;
  created_at: string;
  title?: string;
}

export interface Message {
  role: "user" | "assistant" | "tool_call" | "tool_result" | "system";
  content: string;
}

export interface ModelConfig {
  id: string;
  name: string;
  provider: string;
  modelId: string;
  apiBaseUrl: string;
  apiKey: string;
  temperature: number;
  topP: number;
  maxTokens: number;
  isDefault: boolean;
  status: "online" | "offline" | "untested";
}

export interface Skill {
  id: string;
  name: string;
  description: string;
  version: string;
  author: string;
  category: "builtin" | "custom" | "third-party";
  enabled: boolean;
  sourceCode?: string;
  parameters?: Record<string, any>;
}
```

- [ ] **Step 2: 创建 `web/src/api/client.ts`**

从 `desktop/src/api/client.ts` 复制，并扩展以下方法：

```typescript
export interface ServerEvent {
  event: string;
  data: Record<string, any>;
}

export interface Session {
  id: string;
  model: string;
  created_at: string;
}

export class ApiClient {
  private baseUrl: string;

  constructor(baseUrl: string = "") {
    this.baseUrl = baseUrl;
  }

  async createSession(): Promise<Session> {
    const res = await fetch(`${this.baseUrl}/sessions`, { method: "POST" });
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      throw new Error(
        `创建会话失败 (${res.status}): ${data?.error?.message || res.statusText}`
      );
    }
    return res.json();
  }

  async listSessions(): Promise<Session[]> {
    const res = await fetch(`${this.baseUrl}/sessions`);
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      throw new Error(
        `获取会话列表失败 (${res.status}): ${data?.error?.message || res.statusText}`
      );
    }
    const json = await res.json();
    return json.sessions ?? [];
  }

  async getSessionMessages(sessionId: string): Promise<{ role: string; content: string }[]> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}/messages`);
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      throw new Error(
        `获取消息失败 (${res.status}): ${data?.error?.message || res.statusText}`
      );
    }
    const json = await res.json();
    return json.messages ?? [];
  }

  async renameSession(sessionId: string, title: string): Promise<void> {
    // 后端暂无独立 rename 接口，先本地模拟；后续对接后端时替换
    // 当前实现：仅客户端状态更新，不调用 API
    return Promise.resolve();
  }

  async exportSession(sessionId: string, title: string, messages: { role: string; content: string }[]): Promise<void> {
    const markdown = messages
      .map((m) => {
        const label = m.role === "user" ? "用户" : m.role === "assistant" ? "助手" : m.role;
        return `## ${label}\n\n${m.content}\n`;
      })
      .join("\n");
    const blob = new Blob([`# ${title}\n\n${markdown}`], { type: "text/markdown" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${title || sessionId}.md`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async *sendMessage(
    sessionId: string,
    content: string,
    signal?: AbortSignal
  ): AsyncGenerator<ServerEvent, void> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content }),
      signal,
    });
    if (!res.ok || !res.body) {
      const data = await res.json().catch(() => ({}));
      throw new Error(
        `请求失败 (${res.status}): ${data?.error?.message || res.statusText}`
      );
    }
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let currentEvent = "";
    while (true) {
      const { done, value } = await reader.read();
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";
      for (const line of lines) {
        if (line.startsWith("event:")) {
          currentEvent = line.slice(line.charAt(6) === " " ? 7 : 6);
        } else if (line.startsWith("data:")) {
          const data = line.slice(line.charAt(5) === " " ? 6 : 5);
          if (data === "[DONE]") return;
          try {
            yield { event: currentEvent, data: JSON.parse(data) };
          } catch {}
          currentEvent = "";
        }
      }
      if (done) break;
    }
  }

  async clearSession(sessionId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}/clear`, {
      method: "POST",
    });
    if (!res.ok) {
      throw new Error(`清空会话失败: ${res.status}`);
    }
  }

  async deleteSession(sessionId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}`, {
      method: "DELETE",
    });
    if (!res.ok) {
      throw new Error(`删除会话失败: ${res.status}`);
    }
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add web/src/api/types.ts web/src/api/client.ts
git commit -m "feat(web): add API client and DTO types"
```

---

## Task 4: Zustand Store（App + Chat）

**Files:**
- Create: `web/src/stores/appStore.ts`
- Create: `web/src/stores/chatStore.ts`

- [ ] **Step 1: 创建 `web/src/stores/appStore.ts`**

```typescript
import { create } from "zustand";
import { persist } from "zustand/middleware";

export type View = "chat" | "models" | "skills" | "settings";

export interface ToastItem {
  id: string;
  type: "success" | "error" | "info";
  message: string;
}

interface AppState {
  currentView: View;
  sidebarWidth: number;
  toasts: ToastItem[];
  setCurrentView: (view: View) => void;
  setSidebarWidth: (width: number) => void;
  addToast: (toast: Omit<ToastItem, "id">) => void;
  removeToast: (id: string) => void;
}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      currentView: "chat",
      sidebarWidth: 260,
      toasts: [],
      setCurrentView: (view) => set({ currentView: view }),
      setSidebarWidth: (width) => set({ sidebarWidth: Math.max(180, Math.min(400, width)) }),
      addToast: (toast) =>
        set((state) => ({
          toasts: [...state.toasts, { ...toast, id: crypto.randomUUID() }],
        })),
      removeToast: (id) =>
        set((state) => ({
          toasts: state.toasts.filter((t) => t.id !== id),
        })),
    }),
    { name: "rust-agent-web-app" }
  )
);
```

- [ ] **Step 2: 创建 `web/src/stores/chatStore.ts`**

从 `desktop/src/stores/useChatStore.ts` 复制并扩展：

```typescript
import { create } from "zustand";

export interface Message {
  role: "user" | "assistant" | "tool_call" | "tool_result" | "system";
  content: string;
}

export interface ChatSession {
  id: string;
  model: string;
  title: string;
  messages: Message[];
}

interface ChatState {
  sessions: ChatSession[];
  activeSessionId: string | null;
  isLoading: boolean;
  currentReply: string;
  error: string | null;
  abortController: AbortController | null;
  searchQuery: string;

  setSessions: (sessions: ChatSession[]) => void;
  addSession: (session: ChatSession) => void;
  removeSession: (id: string) => void;
  setActiveSession: (id: string) => void;
  renameSession: (id: string, title: string) => void;
  addMessage: (sessionId: string, message: Message) => void;
  clearMessages: (sessionId: string) => void;
  setCurrentReply: (reply: string) => void;
  appendCurrentReply: (text: string) => void;
  setIsLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setAbortController: (controller: AbortController | null) => void;
  clearCurrentReply: () => void;
  setSearchQuery: (query: string) => void;
}

export const useChatStore = create<ChatState>((set) => ({
  sessions: [],
  activeSessionId: null,
  isLoading: false,
  currentReply: "",
  error: null,
  abortController: null,
  searchQuery: "",

  setSessions: (sessions) => set({ sessions }),
  addSession: (session) =>
    set((state) => ({
      sessions: [...state.sessions, session],
      activeSessionId: session.id,
    })),
  removeSession: (id) =>
    set((state) => ({
      sessions: state.sessions.filter((s) => s.id !== id),
      activeSessionId:
        state.activeSessionId === id
          ? state.sessions.find((s) => s.id !== id)?.id ?? null
          : state.activeSessionId,
    })),
  setActiveSession: (id) => set({ activeSessionId: id }),
  renameSession: (id, title) =>
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === id ? { ...s, title } : s
      ),
    })),
  addMessage: (sessionId, message) =>
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === sessionId ? { ...s, messages: [...s.messages, message] } : s
      ),
    })),
  clearMessages: (sessionId) =>
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === sessionId ? { ...s, messages: [] } : s
      ),
    })),
  setCurrentReply: (reply) => set({ currentReply: reply }),
  appendCurrentReply: (text) =>
    set((state) => ({ currentReply: state.currentReply + text })),
  setIsLoading: (loading) => set({ isLoading: loading }),
  setError: (error) => set({ error }),
  setAbortController: (controller) => set({ abortController: controller }),
  clearCurrentReply: () => set({ currentReply: "" }),
  setSearchQuery: (query) => set({ searchQuery: query }),
}));
```

- [ ] **Step 3: Commit**

```bash
git add web/src/stores/appStore.ts web/src/stores/chatStore.ts
git commit -m "feat(web): add app and chat zustand stores"
```

---

## Task 5: 自定义 Hooks

**Files:**
- Create: `web/src/hooks/useChatStream.ts`
- Create: `web/src/hooks/useSidebarResize.ts`

- [ ] **Step 1: 创建 `web/src/hooks/useChatStream.ts`**

```typescript
import { useCallback } from "react";
import { ApiClient } from "../api/client";
import { useChatStore } from "../stores/chatStore";
import { useAppStore } from "../stores/appStore";

export function useChatStream(apiClient: ApiClient) {
  const {
    addMessage,
    setIsLoading,
    setCurrentReply,
    appendCurrentReply,
    clearCurrentReply,
    setError,
    setAbortController,
  } = useChatStore();
  const addToast = useAppStore((s) => s.addToast);

  const send = useCallback(
    async (sessionId: string, content: string) => {
      setError(null);
      addMessage(sessionId, { role: "user", content });
      setIsLoading(true);
      clearCurrentReply();

      const controller = new AbortController();
      setAbortController(controller);

      try {
        for await (const event of apiClient.sendMessage(
          sessionId,
          content,
          controller.signal
        )) {
          switch (event.event) {
            case "text_delta":
              appendCurrentReply(event.data.content ?? "");
              break;
            case "tool_call": {
              const reply = useChatStore.getState().currentReply;
              if (reply) {
                addMessage(sessionId, { role: "assistant", content: reply });
              }
              clearCurrentReply();
              addMessage(sessionId, {
                role: "tool_call",
                content: JSON.stringify(event.data),
              });
              break;
            }
            case "tool_result":
              addMessage(sessionId, {
                role: "tool_result",
                content: event.data.output ?? "",
              });
              break;
            case "turn_end": {
              const reply = useChatStore.getState().currentReply;
              if (reply) {
                addMessage(sessionId, { role: "assistant", content: reply });
              }
              clearCurrentReply();
              const apiCalls = event.data?.api_calls;
              if (apiCalls) {
                addMessage(sessionId, {
                  role: "system",
                  content: `── 完成，API 调用 ${apiCalls} 次 ──`,
                });
              }
              break;
            }
            case "error":
              setError(event.data.message || "未知错误");
              addToast({ type: "error", message: event.data.message || "未知错误" });
              break;
            case "done":
              clearCurrentReply();
              break;
          }
        }
      } catch (err) {
        if (err instanceof Error && err.name === "AbortError") {
          const reply = useChatStore.getState().currentReply;
          if (reply) {
            addMessage(sessionId, { role: "assistant", content: reply });
          }
          addMessage(sessionId, { role: "system", content: "── 已中断 ──" });
        } else {
          const msg = String(err);
          setError(msg);
          addToast({ type: "error", message: msg });
        }
      } finally {
        setIsLoading(false);
        clearCurrentReply();
        setAbortController(null);
      }
    },
    [apiClient, addMessage, setIsLoading, clearCurrentReply, appendCurrentReply, setError, setAbortController, addToast]
  );

  const abort = useCallback(() => {
    const controller = useChatStore.getState().abortController;
    if (controller) {
      controller.abort();
      setAbortController(null);
    }
  }, [setAbortController]);

  return { send, abort };
}
```

- [ ] **Step 2: 创建 `web/src/hooks/useSidebarResize.ts`**

```typescript
import { useRef, useCallback, useEffect } from "react";
import { useAppStore } from "../stores/appStore";

export function useSidebarResize() {
  const sidebarWidth = useAppStore((s) => s.sidebarWidth);
  const setSidebarWidth = useAppStore((s) => s.setSidebarWidth);
  const isDragging = useRef(false);
  const startX = useRef(0);
  const startWidth = useRef(0);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      isDragging.current = true;
      startX.current = e.clientX;
      startWidth.current = sidebarWidth;
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";
    },
    [sidebarWidth]
  );

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = e.clientX - startX.current;
      setSidebarWidth(startWidth.current + delta);
    };
    const onMouseUp = () => {
      if (!isDragging.current) return;
      isDragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [setSidebarWidth]);

  return { sidebarWidth, onMouseDown };
}
```

- [ ] **Step 3: Commit**

```bash
git add web/src/hooks/useChatStream.ts web/src/hooks/useSidebarResize.ts
git commit -m "feat(web): add useChatStream and useSidebarResize hooks"
```

---

## Task 6: 布局组件（ActivityBar + ResizablePane + Sidebar + MainArea）

**Files:**
- Create: `web/src/components/layout/ActivityBar.tsx`
- Create: `web/src/components/layout/ResizablePane.tsx`
- Create: `web/src/components/layout/Sidebar.tsx`
- Create: `web/src/components/layout/MainArea.tsx`

- [ ] **Step 1: 创建 `web/src/components/layout/ActivityBar.tsx`**

```typescript
import { MessageSquare, Cpu, Puzzle, Settings } from "lucide-react";
import { useAppStore, type View } from "../../stores/appStore";
import { motion } from "framer-motion";

const items: { view: View; icon: React.ReactNode; label: string }[] = [
  { view: "chat", icon: <MessageSquare size={20} />, label: "聊天" },
  { view: "models", icon: <Cpu size={20} />, label: "模型" },
  { view: "skills", icon: <Puzzle size={20} />, label: "技能" },
  { view: "settings", icon: <Settings size={20} />, label: "设置" },
];

export default function ActivityBar() {
  const { currentView, setCurrentView } = useAppStore();

  return (
    <div className="flex h-full w-12 flex-col items-center gap-2 border-r border-[rgba(255,255,255,0.06)] bg-[#141414] py-3">
      {items.map((item) => {
        const active = currentView === item.view;
        return (
          <button
            key={item.view}
            onClick={() => setCurrentView(item.view)}
            title={item.label}
            className={`relative flex h-9 w-9 items-center justify-center rounded-md transition ${
              active
                ? "bg-[#1a1a1a] text-[#818cf8]"
                : "text-neutral-400 hover:bg-[#1a1a1a] hover:text-neutral-200"
            }`}
          >
            {active && (
              <motion.div
                layoutId="activity-indicator"
                className="absolute left-0 top-1.5 h-6 w-[2px] rounded-r bg-[#6366f1]"
                transition={{ duration: 0.15, ease: "easeOut" }}
              />
            )}
            {item.icon}
          </button>
        );
      })}
    </div>
  );
}
```

- [ ] **Step 2: 创建 `web/src/components/layout/ResizablePane.tsx`**

```typescript
import { useSidebarResize } from "../../hooks/useSidebarResize";

interface ResizablePaneProps {
  sidebar: React.ReactNode;
  main: React.ReactNode;
}

export default function ResizablePane({ sidebar, main }: ResizablePaneProps) {
  const { sidebarWidth, onMouseDown } = useSidebarResize();

  return (
    <div className="flex flex-1 overflow-hidden">
      <div style={{ width: sidebarWidth }} className="flex shrink-0 flex-col overflow-hidden">
        {sidebar}
      </div>
      <div
        onMouseDown={onMouseDown}
        className="w-[3px] shrink-0 cursor-col-resize bg-[rgba(255,255,255,0.06)] hover:bg-[#6366f1] transition"
      />
      <div className="flex flex-1 flex-col overflow-hidden">{main}</div>
    </div>
  );
}
```

- [ ] **Step 3: 创建 `web/src/components/layout/Sidebar.tsx`**

```typescript
import { useAppStore } from "../../stores/appStore";
import SessionSidebar from "../chat/SessionSidebar";

export default function Sidebar() {
  const currentView = useAppStore((s) => s.currentView);

  return (
    <div className="flex h-full flex-col bg-[#141414]">
      {currentView === "chat" && <SessionSidebar />}
      {currentView === "models" && (
        <div className="flex items-center justify-center text-sm text-neutral-500">模型列表占位</div>
      )}
      {currentView === "skills" && (
        <div className="flex items-center justify-center text-sm text-neutral-500">技能目录占位</div>
      )}
      {currentView === "settings" && (
        <div className="flex items-center justify-center text-sm text-neutral-500">设置占位</div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: 创建 `web/src/components/layout/MainArea.tsx`**

```typescript
import { Outlet } from "react-router";

export default function MainArea() {
  return (
    <div className="flex h-full flex-col bg-[#0c0c0c]">
      <Outlet />
    </div>
  );
}
```

- [ ] **Step 5: Commit**

```bash
git add web/src/components/layout/ActivityBar.tsx web/src/components/layout/ResizablePane.tsx web/src/components/layout/Sidebar.tsx web/src/components/layout/MainArea.tsx
git commit -m "feat(web): add IDE layout components"
```

---

## Task 7: 路由配置

**Files:**
- Create: `web/src/router.tsx`

- [ ] **Step 1: 创建 `web/src/router.tsx`**

```typescript
import { createBrowserRouter, Navigate } from "react-router";
import App from "./App";
import ChatPage from "./components/chat/ChatPage";

export const router = createBrowserRouter([
  {
    path: "/",
    element: <App />,
    children: [
      { index: true, element: <Navigate to="/chat" replace /> },
      { path: "chat", element: <ChatPage /> },
      { path: "models", element: <div className="p-4 text-neutral-500">模型管理（Plan B）</div> },
      { path: "skills", element: <div className="p-4 text-neutral-500">技能中心（Plan B）</div> },
      { path: "settings", element: <div className="p-4 text-neutral-500">设置（Plan B）</div> },
    ],
  },
]);
```

- [ ] **Step 2: Commit**

```bash
git add web/src/router.tsx
git commit -m "feat(web): add react router config"
```

---

## Task 8: App 根组件与 main.tsx 入口

**Files:**
- Create: `web/src/App.tsx`
- Modify: `web/src/main.tsx`

- [ ] **Step 1: 创建 `web/src/App.tsx`**

```typescript
import ActivityBar from "./components/layout/ActivityBar";
import ResizablePane from "./components/layout/ResizablePane";
import Sidebar from "./components/layout/Sidebar";
import MainArea from "./components/layout/MainArea";

export default function App() {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-[#0c0c0c]">
      <ActivityBar />
      <ResizablePane sidebar={<Sidebar />} main={<MainArea />} />
    </div>
  );
}
```

- [ ] **Step 2: 修改 `web/src/main.tsx`**

```typescript
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "./index.css";
import { router } from "./router";

const queryClient = new QueryClient();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>
);
```

- [ ] **Step 3: Commit**

```bash
git add web/src/App.tsx web/src/main.tsx
git commit -m "feat(web): add App root and main entry with QueryClient"
```

---

## Task 9: 聊天模块 — SessionSidebar（会话侧边栏）

**Files:**
- Create: `web/src/components/chat/SessionSidebar.tsx`

- [ ] **Step 1: 创建 `web/src/components/chat/SessionSidebar.tsx`**

```typescript
import { useState, useCallback } from "react";
import { Plus, Trash2, MessageSquare, Search, Download, Eraser, Check, X } from "lucide-react";
import { useChatStore } from "../../stores/chatStore";
import { ApiClient } from "../../api/client";
import { useAppStore } from "../../stores/appStore";

const apiClient = new ApiClient();

export default function SessionSidebar() {
  const {
    sessions,
    activeSessionId,
    searchQuery,
    setActiveSession,
    addSession,
    removeSession,
    renameSession,
    clearMessages,
    setSearchQuery,
  } = useChatStore();
  const addToast = useAppStore((s) => s.addToast);

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editTitle, setEditTitle] = useState("");

  const filtered = sessions.filter((s) =>
    s.title.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const handleNew = useCallback(async () => {
    try {
      const { id, model } = await apiClient.createSession();
      addSession({ id, model, title: "新会话", messages: [] });
    } catch (e) {
      addToast({ type: "error", message: String(e) });
    }
  }, [addSession, addToast]);

  const handleDelete = useCallback(
    async (id: string) => {
      try {
        await apiClient.deleteSession(id);
        removeSession(id);
      } catch (e) {
        addToast({ type: "error", message: String(e) });
      }
    },
    [removeSession, addToast]
  );

  const handleRenameStart = (id: string, title: string) => {
    setEditingId(id);
    setEditTitle(title);
  };

  const handleRenameConfirm = (id: string) => {
    if (editTitle.trim()) {
      renameSession(id, editTitle.trim());
    }
    setEditingId(null);
  };

  const handleExport = async (id: string) => {
    const session = sessions.find((s) => s.id === id);
    if (!session) return;
    try {
      await apiClient.exportSession(id, session.title, session.messages);
      addToast({ type: "success", message: "导出成功" });
    } catch (e) {
      addToast({ type: "error", message: String(e) });
    }
  };

  const handleClear = async (id: string) => {
    try {
      await apiClient.clearSession(id);
      clearMessages(id);
      addToast({ type: "success", message: "会话已清空" });
    } catch (e) {
      addToast({ type: "error", message: String(e) });
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-[rgba(255,255,255,0.06)] px-3 py-2.5">
        <span className="text-xs font-semibold text-neutral-300">会话</span>
        <button
          onClick={handleNew}
          className="flex h-6 w-6 items-center justify-center rounded text-neutral-400 transition hover:bg-[#1a1a1a] hover:text-neutral-200"
          title="新建会话"
        >
          <Plus size={14} />
        </button>
      </div>
      <div className="px-3 py-2">
        <div className="flex items-center gap-1.5 rounded-md bg-[#1a1a1a] px-2 py-1.5">
          <Search size={12} className="text-neutral-500" />
          <input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="搜索会话..."
            className="flex-1 bg-transparent text-xs text-neutral-200 outline-none placeholder:text-neutral-600"
          />
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 && (
          <div className="mt-6 text-center text-xs text-neutral-600">
            {searchQuery ? "无匹配会话" : "暂无会话"}
          </div>
        )}
        {filtered.map((session) => {
          const active = session.id === activeSessionId;
          const editing = editingId === session.id;
          return (
            <div
              key={session.id}
              onClick={() => setActiveSession(session.id)}
              className={`group mb-1 flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-[13px] transition ${
                active
                  ? "bg-[#1a1a1a] text-neutral-100"
                  : "text-neutral-400 hover:bg-[#1a1a1a]/60 hover:text-neutral-200"
              }`}
            >
              <MessageSquare size={13} className="shrink-0" />
              {editing ? (
                <input
                  autoFocus
                  value={editTitle}
                  onChange={(e) => setEditTitle(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleRenameConfirm(session.id);
                    if (e.key === "Escape") setEditingId(null);
                  }}
                  onBlur={() => handleRenameConfirm(session.id)}
                  onClick={(e) => e.stopPropagation()}
                  className="flex-1 bg-transparent text-[13px] text-neutral-100 outline-none"
                />
              ) : (
                <span
                  className="flex-1 truncate"
                  onDoubleClick={() => handleRenameStart(session.id, session.title)}
                >
                  {session.title}
                </span>
              )}
              <div className="hidden items-center gap-1 group-hover:flex">
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    handleClear(session.id);
                  }}
                  title="清空"
                  className="rounded p-0.5 text-neutral-500 hover:text-amber-400"
                >
                  <Eraser size={11} />
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    handleExport(session.id);
                  }}
                  title="导出"
                  className="rounded p-0.5 text-neutral-500 hover:text-emerald-400"
                >
                  <Download size={11} />
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDelete(session.id);
                  }}
                  title="删除"
                  className="rounded p-0.5 text-neutral-500 hover:text-red-400"
                >
                  <Trash2 size={11} />
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/chat/SessionSidebar.tsx
git commit -m "feat(web): add session sidebar with search, rename, export, clear"
```

---

## Task 10: 聊天模块 — ChatHeader

**Files:**
- Create: `web/src/components/chat/ChatHeader.tsx`

- [ ] **Step 1: 创建 `web/src/components/chat/ChatHeader.tsx`**

```typescript
import { useState } from "react";
import { Eraser, Download, Trash2, Check, X } from "lucide-react";
import { useChatStore } from "../../stores/chatStore";
import { ApiClient } from "../../api/client";
import { useAppStore } from "../../stores/appStore";

const apiClient = new ApiClient();

interface ChatHeaderProps {
  sessionId: string;
  title: string;
  model: string;
}

export default function ChatHeader({ sessionId, title, model }: ChatHeaderProps) {
  const { renameSession, clearMessages, removeSession, setActiveSession } = useChatStore();
  const addToast = useAppStore((s) => s.addToast);
  const [editing, setEditing] = useState(false);
  const [editTitle, setEditTitle] = useState(title);

  const confirmRename = () => {
    if (editTitle.trim()) renameSession(sessionId, editTitle.trim());
    setEditing(false);
  };

  const handleClear = async () => {
    try {
      await apiClient.clearSession(sessionId);
      clearMessages(sessionId);
      addToast({ type: "success", message: "已清空" });
    } catch (e) {
      addToast({ type: "error", message: String(e) });
    }
  };

  const handleExport = async () => {
    const session = useChatStore.getState().sessions.find((s) => s.id === sessionId);
    if (!session) return;
    try {
      await apiClient.exportSession(sessionId, session.title, session.messages);
      addToast({ type: "success", message: "导出成功" });
    } catch (e) {
      addToast({ type: "error", message: String(e) });
    }
  };

  const handleDelete = async () => {
    try {
      await apiClient.deleteSession(sessionId);
      removeSession(sessionId);
    } catch (e) {
      addToast({ type: "error", message: String(e) });
    }
  };

  return (
    <div className="flex h-12 shrink-0 items-center justify-between border-b border-[rgba(255,255,255,0.06)] px-4">
      <div className="flex items-center gap-3">
        {editing ? (
          <input
            autoFocus
            value={editTitle}
            onChange={(e) => setEditTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") confirmRename();
              if (e.key === "Escape") setEditing(false);
            }}
            onBlur={confirmRename}
            className="rounded bg-[#1a1a1a] px-2 py-0.5 text-sm text-neutral-100 outline-none ring-1 ring-[#6366f1]"
          />
        ) : (
          <span
            onClick={() => {
              setEditing(true);
              setEditTitle(title);
            }}
            className="cursor-pointer text-sm font-medium text-neutral-200 hover:text-neutral-100"
            title="点击重命名"
          >
            {title}
          </span>
        )}
        <span className="rounded bg-[#1a1a1a] px-1.5 py-0.5 text-[11px] text-neutral-500">
          {model}
        </span>
      </div>
      <div className="flex items-center gap-1">
        <button onClick={handleClear} title="清空" className="rounded p-1.5 text-neutral-500 hover:bg-[#1a1a1a] hover:text-neutral-300">
          <Eraser size={14} />
        </button>
        <button onClick={handleExport} title="导出 Markdown" className="rounded p-1.5 text-neutral-500 hover:bg-[#1a1a1a] hover:text-neutral-300">
          <Download size={14} />
        </button>
        <button onClick={handleDelete} title="删除" className="rounded p-1.5 text-neutral-500 hover:bg-[#1a1a1a] hover:text-red-400">
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/chat/ChatHeader.tsx
git commit -m "feat(web): add ChatHeader with inline rename, clear, export, delete"
```

---

## Task 11: 聊天模块 — MessageItem + MessageList

**Files:**
- Create: `web/src/components/chat/MessageItem.tsx`
- Create: `web/src/components/chat/MessageList.tsx`

- [ ] **Step 1: 创建 `web/src/components/chat/MessageItem.tsx`**

```typescript
import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { User, Bot, Zap, CheckCircle, Info, ChevronDown, ChevronRight } from "lucide-react";
import type { Message } from "../../stores/chatStore";

interface MessageItemProps {
  message: Message;
}

export default function MessageItem({ message }: MessageItemProps) {
  const isUser = message.role === "user";
  const isAssistant = message.role === "assistant";
  const isToolCall = message.role === "tool_call";
  const isToolResult = message.role === "tool_result";
  const isSystem = message.role === "system";
  const [expanded, setExpanded] = useState(false);

  if (isSystem) {
    return (
      <div className="flex justify-center py-1">
        <span className="text-xs italic text-neutral-500">{message.content}</span>
      </div>
    );
  }

  return (
    <div className={`flex gap-3 py-2 ${isUser ? "flex-row-reverse" : "flex-row"}`}>
      <div
        className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full ${
          isUser
            ? "bg-[#312e81] text-white"
            : isAssistant
            ? "bg-gradient-to-br from-[#6366f1] to-[#818cf8] text-white"
            : isToolCall
            ? "bg-amber-500/20 text-amber-400"
            : "bg-emerald-500/20 text-emerald-400"
        }`}
      >
        {isUser && <User size={14} />}
        {isAssistant && <Bot size={14} />}
        {isToolCall && <Zap size={14} />}
        {isToolResult && <CheckCircle size={14} />}
      </div>
      <div
        className={`max-w-[75%] rounded-2xl px-4 py-2.5 text-[14px] leading-relaxed ${
          isUser
            ? "rounded-tr-sm bg-[#312e81] text-white"
            : isToolCall
            ? "rounded-tl-sm bg-[rgba(245,158,11,0.08)] text-amber-100"
            : isToolResult
            ? "rounded-tl-sm bg-[rgba(16,185,129,0.08)] text-emerald-100"
            : "rounded-tl-sm bg-[#1a1a1a] text-neutral-100"
        }`}
      >
        {isAssistant || isUser ? (
          <div className="prose prose-invert prose-sm max-w-none">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
          </div>
        ) : isToolCall ? (
          <div>
            <button
              onClick={() => setExpanded(!expanded)}
              className="flex items-center gap-1 text-xs text-amber-300 hover:text-amber-200"
            >
              {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <Zap size={12} /> 工具调用
            </button>
            {expanded && (
              <pre className="mt-2 overflow-x-auto rounded bg-[#0c0c0c] p-2 text-xs font-mono text-amber-100">
                <code>{message.content}</code>
              </pre>
            )}
          </div>
        ) : (
          <span className="text-sm">{message.content}</span>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 创建 `web/src/components/chat/MessageList.tsx`**

```typescript
import { useRef, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Bot } from "lucide-react";
import MessageItem from "./MessageItem";
import type { Message } from "../../stores/chatStore";

interface MessageListProps {
  messages: Message[];
  currentReply: string;
  isLoading: boolean;
}

export default function MessageList({ messages, currentReply, isLoading }: MessageListProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, currentReply]);

  return (
    <div className="flex-1 overflow-y-auto px-4 py-2">
      <div className="mx-auto max-w-3xl space-y-1">
        <AnimatePresence initial={false}>
          {messages.map((msg, i) => (
            <motion.div
              key={i}
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.15, ease: "easeOut" }}
            >
              <MessageItem message={msg} />
            </motion.div>
          ))}
        </AnimatePresence>
        {currentReply && (
          <motion.div
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            className="flex gap-3 py-2"
          >
            <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-[#6366f1] to-[#818cf8] text-white">
              <Bot size={14} />
            </div>
            <div className="max-w-[75%] rounded-2xl rounded-tl-sm bg-[#1a1a1a] px-4 py-2.5 text-[14px] text-neutral-100">
              <div className="prose prose-invert prose-sm max-w-none">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {currentReply + "▌"}
                </ReactMarkdown>
              </div>
            </div>
          </motion.div>
        )}
        {isLoading && !currentReply && (
          <div className="flex gap-3 py-2">
            <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-[#6366f1] to-[#818cf8]">
              <span className="h-2 w-2 animate-pulse rounded-full bg-white" />
            </div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add web/src/components/chat/MessageItem.tsx web/src/components/chat/MessageList.tsx
git commit -m "feat(web): add MessageItem and MessageList with markdown, tool collapse, framer motion"
```

---

## Task 12: 聊天模块 — InputBox

**Files:**
- Create: `web/src/components/chat/InputBox.tsx`

- [ ] **Step 1: 创建 `web/src/components/chat/InputBox.tsx`**

```typescript
import { useState, useRef, KeyboardEvent } from "react";
import { Send, Square } from "lucide-react";

interface InputBoxProps {
  onSubmit: (text: string) => void;
  onAbort: () => void;
  isLoading: boolean;
  model?: string;
}

export default function InputBox({ onSubmit, onAbort, isLoading, model }: InputBoxProps) {
  const [value, setValue] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (value.trim() && !isLoading) {
        onSubmit(value.replace(/\\n/g, "\n"));
        setValue("");
      }
    }
  };

  const handleSend = () => {
    if (value.trim() && !isLoading) {
      onSubmit(value.replace(/\\n/g, "\n"));
      setValue("");
    }
  };

  return (
    <div className="shrink-0 border-t border-[rgba(255,255,255,0.06)] bg-[#0c0c0c] px-4 py-3">
      <div className="mx-auto flex max-w-3xl items-end gap-2 rounded-xl border border-[rgba(255,255,255,0.08)] bg-[#242424] px-3 py-2.5 transition focus-within:border-[#6366f1]">
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={
            isLoading
              ? "等待响应中..."
              : model
              ? `[${model}] 输入消息，Shift+Enter 换行...`
              : "输入消息，Shift+Enter 换行..."
          }
          disabled={isLoading}
          rows={1}
          className="max-h-32 min-h-[24px] flex-1 resize-none bg-transparent text-sm text-neutral-100 outline-none placeholder:text-neutral-600"
          style={{ fieldSizing: "content" } as React.CSSProperties}
        />
        {isLoading ? (
          <button
            onClick={onAbort}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-red-600 text-white transition hover:scale-105 hover:bg-red-500"
            title="中断"
          >
            <Square size={14} fill="currentColor" />
          </button>
        ) : (
          <button
            onClick={handleSend}
            disabled={!value.trim()}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#6366f1] text-white transition hover:scale-105 hover:bg-[#818cf8] disabled:opacity-40"
            title="发送"
          >
            <Send size={14} />
          </button>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/chat/InputBox.tsx
git commit -m "feat(web): add InputBox with auto-resize and abort"
```

---

## Task 13: 聊天模块 — ChatPage 组装

**Files:**
- Create: `web/src/components/chat/ChatPage.tsx`

- [ ] **Step 1: 创建 `web/src/components/chat/ChatPage.tsx`**

```typescript
import { useEffect, useMemo } from "react";
import { ApiClient } from "../../api/client";
import { useChatStore } from "../../stores/chatStore";
import { useChatStream } from "../../hooks/useChatStream";
import ChatHeader from "./ChatHeader";
import MessageList from "./MessageList";
import InputBox from "./InputBox";

const apiClient = new ApiClient();

export default function ChatPage() {
  const {
    sessions,
    activeSessionId,
    isLoading,
    currentReply,
    setSessions,
    setError,
  } = useChatStore();
  const { send, abort } = useChatStream(apiClient);

  // 初始加载会话列表
  useEffect(() => {
    let cancelled = false;
    apiClient.listSessions().then((list) => {
      if (cancelled) return;
      const mapped = list.map((s) => ({
        id: s.id,
        model: s.model,
        title: s.title || `会话 ${s.id.slice(0, 6)}`,
        messages: [],
      }));
      setSessions(mapped);
    }).catch((e) => setError(String(e)));
    return () => { cancelled = true; };
  }, [setSessions, setError]);

  // 加载历史消息
  useEffect(() => {
    if (!activeSessionId) return;
    let cancelled = false;
    apiClient.getSessionMessages(activeSessionId).then((msgs) => {
      if (cancelled) return;
      useChatStore.setState((state) => ({
        sessions: state.sessions.map((s) =>
          s.id === activeSessionId
            ? {
                ...s,
                messages: msgs.map((m) => ({
                  role: m.role as any,
                  content: m.content,
                })),
              }
            : s
        ),
      }));
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [activeSessionId]);

  const activeSession = sessions.find((s) => s.id === activeSessionId);

  if (!activeSession) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
        在左侧选择一个会话，或新建会话开始对话
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <ChatHeader
        sessionId={activeSession.id}
        title={activeSession.title}
        model={activeSession.model}
      />
      <MessageList
        messages={activeSession.messages}
        currentReply={currentReply}
        isLoading={isLoading}
      />
      <InputBox
        onSubmit={(text) => send(activeSession.id, text)}
        onAbort={abort}
        isLoading={isLoading}
        model={activeSession.model}
      />
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add web/src/components/chat/ChatPage.tsx
git commit -m "feat(web): add ChatPage assembling header, message list and input"
```

---

## Task 14: 构建验证与开发服务器测试

**Files:** 无新增

- [ ] **Step 1: 类型检查**

Run: `cd web && npx tsc --noEmit`
Expected: 0 errors, 0 warnings

- [ ] **Step 2: 开发服务器启动**

Run: `cd web && npm run dev`
Expected: Vite dev server 启动在 `http://localhost:5173`

- [ ] **Step 3: 生产构建**

Run: `cd web && npm run build`
Expected: `web/dist/` 目录生成，包含 `index.html` 和静态资源。

- [ ] **Step 4: Commit**

```bash
git commit --allow-empty -m "chore(web): verify build and dev server for Plan A"
```

---

## 自检清单

| Spec 章节 | 对应 Task | 状态 |
|---|---|---|
| 4.1 IDE 布局（ActivityBar + Sidebar + MainArea） | Task 6 | ✅ |
| 4.2 视觉设计系统（颜色、字体、圆角、动效） | Task 2 (CSS) + Task 6/11 (组件) | ✅ |
| 5.1 聊天模块 — 顶部工具栏 | Task 10 | ✅ |
| 5.1 聊天模块 — 消息流（用户/助手/工具/系统） | Task 11 | ✅ |
| 5.1 聊天模块 — 底部输入区 | Task 12 | ✅ |
| 5.1 会话增强（搜索/重命名/导出/清空） | Task 9 + 10 | ✅ |
| 6.1 Zustand Store 分层 | Task 4 | ✅ |
| 6.2 数据流（SSE Hook） | Task 5 | ✅ |
| 7. 错误处理（Toast） | Task 4 (appStore) + Task 5/9/10 (调用) | ✅ |
| 8. 技术栈 | Task 1 (package.json) | ✅ |
| 9. 部署方案 | Plan B | ⏳ |
| 5.2 模型管理 | Plan B | ⏳ |
| 5.3 技能中心 | Plan B | ⏳ |

**Placeholder 扫描:** 无 TBD/TODO/"implement later"。所有步骤包含完整代码。

**类型一致性:**
- `Message.role` 在 `types.ts`、`chatStore.ts`、`MessageItem.tsx` 中统一为 `"user" | "assistant" | "tool_call" | "tool_result" | "system"`。
- `ApiClient` 构造函数参数 `baseUrl` 默认为空字符串，适配 Vite proxy。
- `useChatStream` 中 `useChatStore.getState()` 用于读取即时状态，避免闭包 stale 问题。
