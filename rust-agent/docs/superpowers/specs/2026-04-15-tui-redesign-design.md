# TUI 重构设计文档

**日期**：2026-04-15
**状态**：已批准

## 背景

当前 CLI 使用 `rustyline` 作为输入库，多行输入体验不佳（无法回退编辑前一行）。需要重构为基于 `ratatui` 的全功能 TUI 界面。

## 目标

构建类 ChatGPT/Claude Code 的终端聊天界面，支持多行可编辑输入、流式输出、工具调用展示等。

## 布局设计

```
┌─────────────────────────────────────┐
│ 聊天记录区域（可滚动）               │
│                                     │
│  ┌─ bash: `cargo check`             │
│  │  Finished dev profile             │
│  └─                                 │
│                                     │
│  Agent: 编译通过，没有错误。         │
│                                     │
├─────────────────────────────────────┤
│ 多行输入框（可编辑、可回退修改）       │
│ Enter 提交, \+Enter 换行            │
├─────────────────────────────────────┤
│ 状态栏：模型 | token 用量 | 快捷键   │
└─────────────────────────────────────┘
```

## 核心组件

### 1. App struct — 主状态

- `messages: Vec<ChatMessage>` — 聊天记录
- `input: InputBox` — 输入框状态
- `scroll_offset: u16` — 聊天记录滚动偏移
- `mode: AppMode` — 当前模式（输入/等待响应/查看历史）

### 2. UI 渲染循环（tokio task）

- 使用 `crossterm` 监听终端事件（键盘、resize）
- 使用 `ratatui` 渲染界面
- 收到 agent 事件时更新状态并重绘

### 3. Agent task（tokio task）

- 运行 agent 逻辑（调用 `rust-agent-core`）
- 通过 `mpsc::channel` 发送 `AgentEvent` 到 UI task

### 4. InputBox 组件

- 多行文本编辑：光标上下左右移动、行首行尾跳转
- `\` + Enter 换行
- Enter 提交
- 支持 Backspace 删除、Ctrl+U 清空当前行

### 5. 聊天记录组件

- 区分消息类型：用户输入、Agent 回复、工具调用、工具结果、错误
- 自动滚动到最新消息
- 支持手动向上滚动查看历史

### 6. 状态栏

- 显示：模型名称、token 用量/配额、当前模式、快捷键提示

## 事件流

```
用户输入 ──→ App.handle_key() ──→ 提交时发送到 agent channel
                                         │
                                         ▼
Agent task ←── 提交查询 ──→ 调用 LLM API
    │
    ▼
AgentEvent (TextDelta/ToolCall/...) ──→ App.update() ──→ UI 重绘
```

## 异步架构

采用双 task + channel 模式：

- **UI task**：主循环，`tokio::select!` 同时监听终端事件和 agent 事件
- **Agent task**：每次用户提交查询时 spawn，完成后通过 channel 通知

```rust
// 伪代码
loop {
    tokio::select! {
        event = crossterm_events.next() => { app.handle_key(event); }
        agent_event = agent_rx.recv()    => { app.handle_agent_event(agent_event); }
    }
    terminal.draw(|f| ui::draw(f, &app))?;
}
```

## 依赖变更

| 操作 | crate | 说明 |
|------|-------|------|
| 移除 | `rustyline` | 不再需要 |
| 移除 | `termimad` | 用 ratatui 替代 |
| 新增 | `ratatui` | TUI 框架 |
| 新增 | `crossterm` | 终端后端（ratatui 默认后端） |
| 保留 | `tokio` | 异步运行时 |
| 保留 | `anyhow` | 错误处理 |
| 保留 | `rust-agent-core` | 核心 agent 逻辑 |

## 文件结构

```
crates/cli/src/
├── main.rs          # 入口，初始化 terminal 和 app
├── app.rs           # App struct，主状态和事件处理
├── ui/
│   ├── mod.rs       # UI 模块入口
│   ├── chat.rs      # 聊天记录组件
│   ├── input.rs     # 输入框组件
│   └── status.rs    # 状态栏组件
└── event.rs         # 事件类型定义
```

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| Enter | 提交输入 |
| `\` + Enter | 换行 |
| Ctrl+C | 取消当前输入 / 退出 |
| Ctrl+U | 清空当前行 |
| Page Up/Down | 滚动聊天记录 |
