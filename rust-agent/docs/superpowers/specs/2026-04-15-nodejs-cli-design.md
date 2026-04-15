# Node.js CLI 设计文档

**日期**：2026-04-15
**状态**：已批准

## 背景

Rust agent CLI 的终端 UI 方案（rustyline/ratatui）因多行编辑和滚动等问题无法满足需求。迁移到 Node.js CLI + Rust server 架构。

## 架构

```
用户运行 node cli/src/index.ts
  ↓
1. spawn rust-agent-server（子进程，端口自动分配）
2. 等待 server 就绪（GET /sessions 健康检查）
3. POST /sessions 创建会话
4. 启动 Ink React 渲染循环
5. 用户退出时 kill 子进程
```

## 技术选型

| 组件 | 方案 |
|------|------|
| 终端 UI | Ink (React for CLI) |
| 语言 | TypeScript (tsx 运行) |
| HTTP | fetch (Node.js 原生) |
| 流式输出 | EventSource 消费 SSE |

## 文件结构

```
cli/
├── package.json
├── tsconfig.json
└── src/
    ├── index.tsx       # 入口：spawn server + 启动 Ink
    ├── app.tsx         # 主应用组件（状态管理 + 事件循环）
    ├── chat.tsx        # 聊天记录（可滚动）
    ├── input.tsx       # 多行输入框（Ink Textarea）
    └── api.ts         # 调用 Rust server HTTP API
```

## 与现有 server 交互

```
POST /sessions                      → 创建会话
POST /sessions/{id}/messages        → 发消息，SSE 流式响应：
  data: {"type":"text_delta","content":"..."}
  data: {"type":"tool_call","name":"bash",...}
  data: {"type":"tool_result","output":"..."}
  data: [DONE]
```

## 快捷键

- Enter：提交输入
- Shift+Enter：换行（Ink Textarea 原生）
- Ctrl+C：退出
- PgUp/PgDn：滚动聊天记录

## 依赖

- ink（React for CLI）
- react（Ink 底层）
- @types/react
- typescript
- tsx（零配置 TS 执行）
