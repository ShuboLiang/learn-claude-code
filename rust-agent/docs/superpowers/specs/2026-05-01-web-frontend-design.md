# Web 前端设计方案

> 日期：2026-05-01  
> 状态：已确认，待实现

---

## 1. 概述

为 Rust Agent 项目构建一个纯 Web 前端，作为现有 Desktop (Tauri) 应用的 Web 替代品，同时扩展多模型管理、会话增强、技能中心三大功能模块。采用 IDE 风格的工作台布局，全新视觉设计，与 Axum 后端同构部署。

---

## 2. 背景与现状

当前项目已有两类前端：
- **Desktop** (`desktop/`): Tauri + React 19 + Tailwind CSS v4 + Vite，功能完整的聊天客户端
- **CLI** (`cli/`): Node.js 命令行界面

后端 (`crates/server/`) 为 Axum HTTP 服务器，提供：
- 会话管理（创建、删除、列表）
- SSE 消息流（text_delta, tool_call, tool_result, turn_end, error, done）
- 工具调用与执行
- OpenAI 兼容 API

Web 前端将直接复用后端 HTTP API，无需新增后端接口。

---

## 3. 设计目标

| 目标 | 说明 |
|---|---|
| **Web 替代** | 核心聊天体验与 Desktop 一致，通过浏览器访问 |
| **功能扩展** | 新增多模型管理、会话增强（搜索/重命名/导出）、技能中心 |
| **全新视觉** | 抛弃 Desktop 的 neutral-900 纯黑风格，采用现代深灰蓝层次设计 |
| **同构部署** | 由 Axum 直接托管静态文件，开箱即用 |
| **单用户** | 暂不做多用户认证和权限管理 |

---

## 4. 架构设计

### 4.1 整体布局（IDE-Style Workbench）

全屏三栏固定布局，无外部滚动条：

```
┌──────────────────────────────────────────────────────────────┐
│  ┌──┐ ┌─────────────────┐ ┌──────────────────────────────┐ │
│  │🗨️│ │ 上下文侧边栏      │ │                              │ │
│  │⚙️│ │ • 会话/模型/技能  │ │    主内容区域                 │ │
│  │🔧│ │                  │ │                              │ │
│  │◉ │ │                  │ │    (聊天 / 配置 / 详情)       │ │
│  └──┘ │                  │ │                              │ │
│  48px │    260px         │ │         flex-1               │ │
│       │   (可拖拽调整)    │ │                              │ │
│       └─────────────────┘ └──────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
     ↑ Activity Bar      ↑ Sidebar            ↑ Main Area
```

- **Activity Bar**（最左，48px 宽）：垂直图标栏，4 个视图（聊天、模型、技能、设置）。当前视图高亮（indigo 竖线 + 背景变化），hover 显示 tooltip。
- **上下文侧边栏**（左二，默认 260px，可拖拽调整）：根据 Activity Bar 选中项动态切换内容。
- **主区域**（剩余空间）：详情展示。

### 4.2 视觉设计系统

- **主题**：深色为主，深灰蓝层次
  - 最底层背景：`#0C0C0C`
  - 面板背景：`#141414`（Activity Bar、侧边栏）
  - 浮层/卡片：`#1A1A1A` → `#242424`（悬停、选中、输入框）
  - 分割线：`rgba(255,255,255,0.06)`
- **主色调**：Indigo/Violet 系（`#6366f1` → `#818cf8`）
- **字体**：`Inter`（界面）+ `JetBrains Mono`（代码/工具调用）
- **字号层级**：12px（标签）、13px（列表）、14px（正文）、16px（标题）
- **圆角**：小元素 6px，卡片 10px，大面板 12px
- **动效**（Framer Motion）：
  - 视图切换：淡入 + 轻微左滑（150ms `easeOut`）
  - 列表项：staggered 进入，每项延迟 20ms
  - 按钮/卡片：hover `scale: 1.02` + 背景过渡，200ms
  - 消息气泡：新消息从底部滑入
  - **尊重 `prefers-reduced-motion`**：所有动效可降级为瞬间切换

---

## 5. 功能模块设计

### 5.1 聊天模块

**顶部工具栏**（48px 高）：
- 左侧：会话标题（可 inline edit 重命名，回车确认）
- 中间：模型选择下拉框（展示当前模型，带提供商标签和在线状态点）
- 右侧：操作按钮组（清空会话、导出 Markdown、删除会话）

**消息流**：
- **用户消息**：右对齐，`#312e81` 背景，白色文字，圆角 `16px 16px 4px 16px`，最大宽度 75%，Markdown 渲染。
- **助手消息**：左对齐，indigo 渐变圆形机器人头像 + `#1A1A1A` 背景，圆角 `16px 16px 16px 4px`，Markdown 全功能渲染。
- **工具调用**：可折叠卡片，琥珀色 `⚡` 图标，背景 `rgba(245,158,11,0.08)`，默认折叠显示工具名，点击展开 JSON 参数（JetBrains Mono、语法高亮）。
- **工具结果**：翠绿 `✓` 标识，背景 `rgba(16,185,129,0.08)`，简洁文本。
- **系统消息**：居中，12px，`rgba(255,255,255,0.35)`，斜体，无气泡。
- **打字中**：助手头像轻微脉动，消息末尾闪烁光标 `▌`。

**底部输入区**：
- 多行文本框，auto-resize（1~6 行），`#242424` 背景，聚焦边框 indigo。
- 占位符：`[模型名] 输入消息，Shift+Enter 换行...`
- 发送按钮：indigo 圆形，hover 放大；加载时变红色方形中断按钮。

### 5.2 模型管理模块

**侧边栏（模型列表）**：
- 顶部搜索框 + "添加模型"按钮（indigo 主按钮）。
- 模型卡片：名称（14px 粗体）、提供商标签（小药丸 badge）、状态指示灯（🟢在线 / 🔴离线 / ⚪未测试）、默认标记角标。
- 卡片 hover：`#1A1A1A` → `#242424`，轻微左移指示。

**主区域（模型详情配置）**：
- 头部：模型名称 + 删除按钮 + "设为默认"按钮。
- 配置表单（Shadcn/ui）：
  - 基础信息：显示名称、提供商（下拉）、模型 ID
  - 连接配置：API Base URL、API Key（密码输入，带显示/隐藏切换）
  - 生成参数：Temperature（0-2 滑块）、Top P 滑块、Max Tokens 数字输入
- **测试连接**：调用后端接口验证，返回 success/error toast。
- **自动保存**：所有字段 debounce 500ms，无需手动保存。

### 5.3 技能中心模块

**侧边栏（技能目录）**：
- 树形结构：分类节点可折叠（系统内置 📦 / 用户自定义 🛠️ / 第三方 🔌）。
- 技能项：图标、名称、版本号（灰色小字）、启用 Switch（即时切换）。
- 支持按名称/描述实时搜索过滤。

**主区域（技能详情）**：
- 头部：技能名称 + 版本标签 + 作者 + 全局启用开关。
- 标签页：**概览** / **参数配置** / **源码预览** / **测试**
  - 概览：描述、用途、输入输出示例
  - 参数配置：根据 JSON Schema 动态渲染表单（字符串、数字滑块、布尔、枚举）
  - 源码预览：只读代码编辑器风格（深色语法高亮）
  - 测试：手动输入参数 JSON → 运行 → 展示结果（成功绿色 / 错误红色）

---

## 6. 数据流与状态管理

### 6.1 Store 分层（Zustand）

| Store | 职责 |
|---|---|
| `useAppStore` | 全局 UI（当前视图、侧边栏宽度、主题、全局 Toast） |
| `useChatStore` | 聊天核心（sessions、activeSessionId、messages、isLoading、currentReply、abortController） |
| `useModelStore` | 模型配置（models、selectedModelId、draftForm） |
| `useSkillStore` | 技能管理（skills、categories、selectedSkillId） |

### 6.2 数据流策略

- **服务器状态**（模型列表、技能列表）：使用 **TanStack Query**（`useQuery` / `useMutation`），自动缓存、去重、后台刷新、错误重试。
- **聊天会话/消息**：Zustand 管理。SSE 流通过自定义 Hook `useChatStream` 接收事件 → 直接更新 `chatStore` → UI 渲染。
- **模型表单草稿**：Zustand 存储，debounce 500ms 后自动调用 `updateModel` mutation。
- **侧边栏宽度、当前视图**：纯 Zustand，持久化到 `localStorage`。

---

## 7. 错误处理策略

| 场景 | 处理方式 |
|---|---|
| API 请求失败 | 全局 Toast（Shadcn Sonner，右下角红色滑入，显示消息和重试按钮） |
| SSE 连接断开 | 顶部 Banner（红色条 + "重连"按钮，自动重试 3 次后手动重连） |
| 表单验证失败 | 字段级即时反馈（Shadcn Form + Zod，红边框 + 错误文案） |
| 模型测试连接失败 | Inline Alert（黄色警告框，展示后端具体错误） |

---

## 8. 技术栈

| 层级 | 选型 |
|---|---|
| 框架 | React 19 |
| 构建工具 | Vite 6 |
| 样式 | Tailwind CSS v4 |
| 组件库 | Shadcn/ui（基于 Radix UI） |
| 动效 | Framer Motion |
| 状态管理 | Zustand 5 |
| 服务器状态 | TanStack Query v5 |
| 路由 | React Router v7 |
| 表单+校验 | React Hook Form + Zod |
| 图标 | Lucide React |
| Markdown | React Markdown + remark-gfm |
| 代码高亮 | PrismJS / highlight.js（工具调用 JSON 展示） |
| Toast | Sonner |

---

## 9. 部署方案

**Axum 同构部署**：
- `crates/server/src/main.rs` 增加 `tower_http::services::ServeDir`，托管 `web/dist` 静态文件。
- API 路由保持现有路径，前端静态文件从 `/` 提供。
- 所有前端路由（`/chat`, `/models` 等）fallback 到 `index.html`。
- 开发模式：`npm run dev`（Vite dev server，代理 API 到 `localhost:3000`）。
- 生产模式：`cargo run --bin rust-agent-server`（Axum 同时提供 API + 静态文件）。

---

## 10. 项目结构

新增 `web/` 目录，与 `desktop/` 同级：

```
web/
├── package.json
├── vite.config.ts              # build.outDir = 'dist'
├── tsconfig.json
├── index.html
├── components.json             # shadcn/ui 配置
├── src/
│   ├── main.tsx                # React 挂载 + QueryClientProvider
│   ├── App.tsx                 # 根布局
│   ├── router.tsx              # 路由定义
│   ├── index.css               # Tailwind + 自定义主题变量
│   ├── api/
│   │   ├── client.ts           # API 封装（复用 desktop 逻辑）
│   │   └── types.ts            # DTO 类型
│   ├── stores/
│   │   ├── appStore.ts
│   │   ├── chatStore.ts
│   │   ├── modelStore.ts
│   │   └── skillStore.ts
│   ├── components/
│   │   ├── layout/
│   │   │   ├── ActivityBar.tsx
│   │   │   ├── Sidebar.tsx
│   │   │   ├── ResizablePane.tsx
│   │   │   └── MainArea.tsx
│   │   ├── chat/
│   │   │   ├── ChatHeader.tsx
│   │   │   ├── MessageList.tsx
│   │   │   ├── MessageItem.tsx
│   │   │   ├── InputBox.tsx
│   │   │   └── ModelSelector.tsx
│   │   ├── model/
│   │   │   ├── ModelList.tsx
│   │   │   ├── ModelCard.tsx
│   │   │   └── ModelForm.tsx
│   │   ├── skill/
│   │   │   ├── SkillTree.tsx
│   │   │   ├── SkillDetail.tsx
│   │   │   └── SkillTester.tsx
│   │   └── ui/               # shadcn/ui 组件（由 CLI 生成）
│   └── hooks/
│       ├── useChatStream.ts
│       └── useSidebarResize.ts
└── dist/                       # Vite 构建输出（Axum 托管）
```

---

## 11. 构建流程

1. `cd web && npm install && npm run build` → 输出 `web/dist`
2. `cargo run --bin rust-agent-server` → Axum 加载 `web/dist` 并绑定端口
3. 访问 `http://localhost:3000` 即可使用

---

## 12. 附录：API 复用说明

Web 前端直接复用 Desktop 的 API 客户端逻辑（`desktop/src/api/client.ts`），无需新增后端接口。后端已具备：
- `POST /sessions` — 创建会话
- `GET /sessions/:id` — 获取会话
- `DELETE /sessions/:id` — 删除会话
- `POST /sessions/:id/messages` — SSE 发送消息
- 模型、技能相关接口视后端现有能力对接（若后端暂无独立模型/技能管理接口，可在 Web 实现阶段先以本地状态模拟，或优先实现后端接口）。

---

## 13. 非目标（明确不做）

- 多用户系统、登录认证、权限控制
- 移动端优先适配（先做桌面端，响应式仅保证基本可用）
- 国际化（仅中文界面）
- PWA / 离线能力
- 语音输入 / 语音播报
