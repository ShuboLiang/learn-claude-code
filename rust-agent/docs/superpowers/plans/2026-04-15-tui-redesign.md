# TUI 重构实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 CLI 从 rustyline 行编辑器重构为基于 ratatui 的全功能 TUI 聊天界面

**Architecture:** 双 task + channel 模式。UI task 运行 ratatui 渲染循环并通过 crossterm EventStream 监听键盘事件；Agent task 运行 LLM 逻辑并通过 mpsc channel 发送 AgentEvent。通过 tokio::select! 同时监听两类事件。

**Tech Stack:** ratatui 0.30, crossterm 0.29, futures 0.3, tokio (已有), rust-agent-core (已有)

**Spec:** `docs/superpowers/specs/2026-04-15-tui-redesign-design.md`

---

## 文件结构

```
crates/cli/src/
├── main.rs          # 入口：初始化 terminal、启动事件循环
├── app.rs           # App struct：主状态 + 事件处理逻辑
├── event.rs         # 事件类型：AppEvent 枚举
└── ui/
    ├── mod.rs       # draw() 入口：布局分割 + 调用子组件
    ├── chat.rs      # 聊天记录组件：ChatWidget
    ├── input.rs     # 输入框组件：InputBox
    └── status.rs    # 状态栏组件：StatusBar
```

---

### Task 1: 更新依赖

**Files:**
- Modify: `crates/cli/Cargo.toml`

- [ ] **Step 1: 修改 Cargo.toml**

移除 `rustyline` 和 `termimad`，添加 `ratatui`、`crossterm`、`futures`：

```toml
[dependencies]
rust-agent-core = { path = "../core" }
anyhow = "1.0"
tokio = { version = "1.48", features = ["macros", "rt-multi-thread", "sync"] }
ratatui = "0.30"
crossterm = "0.29"
futures = "0.3"
dotenvy = "0.15"
```

- [ ] **Step 2: 运行 cargo check 确认依赖解析**

Run: `cargo check -p rust-agent-cli 2>&1 | tail -5`
Expected: 编译错误（main.rs 引用了已移除的 rustyline），但依赖本身应能解析成功

- [ ] **Step 3: Commit**

```bash
git add crates/cli/Cargo.toml
git commit -m "chore: 更新 CLI 依赖，移除 rustyline/termimad，添加 ratatui/crossterm"
```

---

### Task 2: 创建事件类型和 InputBox 组件

**Files:**
- Create: `crates/cli/src/event.rs`
- Create: `crates/cli/src/ui/input.rs`
- Create: `crates/cli/src/ui/mod.rs`

- [ ] **Step 1: 创建 `event.rs`**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// UI 事件
#[derive(Debug)]
pub enum AppEvent {
    /// 键盘按键
    Key(KeyEvent),
    /// Agent 事件（文本增量、工具调用等）
    Agent(rust_agent_core::agent::AgentEvent),
    /// Agent 回合完成
    AgentDone(Result<String, anyhow::Error>),
    /// 终端 resize
    Resize(u16, u16),
}

/// 判断按键是否是普通可打印字符输入
fn is_printable(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char(_))
        && key.modifiers.contains(KeyModifiers::NONE)
}
```

- [ ] **Step 2: 创建 `ui/mod.rs`**

```rust
pub mod chat;
pub mod input;
pub mod status;

use ratatui::Frame;

use crate::app::App;

/// 渲染整个 UI
pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1),  // 聊天记录
            ratatui::layout::Constraint::Length(3), // 输入框
            ratatui::layout::Constraint::Length(1), // 状态栏
        ])
        .split(frame.area());

    chat::draw(frame, app, chunks[0]);
    input::draw(frame, app, chunks[1]);
    status::draw(frame, app, chunks[2]);
}
```

- [ ] **Step 3: 创建 `ui/input.rs`**

```rust
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::app::App;

/// 输入框光标位置
#[derive(Debug, Clone)]
pub struct Cursor {
    /// 行索引
    pub row: usize,
    /// 列索引
    pub col: usize,
}

/// 多行输入框
#[derive(Debug, Clone)]
pub struct InputBox {
    /// 每行文本
    pub lines: Vec<String>,
    /// 光标位置
    pub cursor: Cursor,
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: Cursor { row: 0, col: 0 },
        }
    }

    /// 获取完整文本（拼接所有行，去除行尾 \）
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// 获取带 \ 的原始文本（用于显示）
    pub fn display_text(&self) -> String {
        self.lines.join("\n")
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_empty())
    }

    /// 清空
    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor = Cursor { row: 0, col: 0 };
    }

    /// 在光标位置插入字符
    pub fn insert_char(&mut self, c: char) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        if row < self.lines.len() {
            self.lines[row].insert(col, c);
            self.cursor.col += 1;
        }
    }

    /// 处理退格
    pub fn backspace(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;

        if col > 0 {
            // 当前行内删除
            self.lines[row].remove(col - 1);
            self.cursor.col -= 1;
        } else if row > 0 {
            // 合并到上一行
            let prev_len = self.lines[row - 1].len();
            let current = self.lines.remove(row);
            self.lines[row - 1].push_str(&current);
            self.cursor.row -= 1;
            self.cursor.col = prev_len;
        }
    }

    /// 删除光标后的字符
    pub fn delete(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;

        if col < self.lines[row].len() {
            self.lines[row].remove(col);
        } else if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
        }
    }

    /// 换行（在光标位置插入新行）
    pub fn newline(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let current = &mut self.lines[row];
        let rest: String = current.drain(col..).collect();
        self.lines.insert(row + 1, rest);
        self.cursor.row += 1;
        self.cursor.col = 0;
    }

    /// 光标左移
    pub fn move_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.col = self.lines[self.cursor.row].len();
        }
    }

    /// 光标右移
    pub fn move_right(&mut self) {
        let max_col = self.lines[self.cursor.row].len();
        if self.cursor.col < max_col {
            self.cursor.col += 1;
        } else if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            self.cursor.col = 0;
        }
    }

    /// 光标上移
    pub fn move_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            let max_col = self.lines[self.cursor.row].len();
            if self.cursor.col > max_col {
                self.cursor.col = max_col;
            }
        }
    }

    /// 光标下移
    pub fn move_down(&mut self) {
        if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            let max_col = self.lines[self.cursor.row].len();
            if self.cursor.col > max_col {
                self.cursor.col = max_col;
            }
        }
    }

    /// 移动到行首
    pub fn move_home(&mut self) {
        self.cursor.col = 0;
    }

    /// 移动到行尾
    pub fn move_end(&mut self) {
        self.cursor.col = self.lines[self.cursor.row].len();
    }

    /// 清空当前行
    pub fn clear_line(&mut self) {
        let row = self.cursor.row;
        self.lines[row].clear();
        self.cursor.col = 0;
    }
}

/// 渲染输入框
pub fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input = &app.input;
    let text = input.display_text();

    let prompt = if app.agent_running { " (等待响应) " } else { " agent >> " };

    let paragraph = Paragraph::new(text.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(prompt),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);

    // 设置光标位置（需考虑 block 边框偏移）
    let x = area.x + 1 + input.cursor.col as u16;
    let y = area.y + 1 + input.cursor.row as u16;
    frame.set_cursor_position((x, y));
}
```

- [ ] **Step 4: 运行 cargo check**

Run: `cargo check -p rust-agent-cli 2>&1 | tail -5`
Expected: 编译错误（mod 声明了但 main.rs 还没引用），属于正常

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/event.rs crates/cli/src/ui/
git commit -m "feat(tui): 添加事件类型和 InputBox 多行输入组件"
```

---

### Task 3: 创建聊天记录和状态栏组件

**Files:**
- Create: `crates/cli/src/ui/chat.rs`
- Create: `crates/cli/src/ui/status.rs`

- [ ] **Step 1: 创建 `ui/chat.rs`**

```rust
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::app::{App, ChatMessage};

/// 渲染聊天记录区域
pub fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let messages: Vec<Line> = app.messages.iter().flat_map(|msg| {
        match msg {
            ChatMessage::User(text) => {
                vec![
                    Line::from(Span::styled(
                        "你:",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(text.as_str()),
                    Line::from(""),
                ]
            }
            ChatMessage::Assistant(text) => {
                let mut lines: Vec<Line> = text.lines()
                    .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::White))))
                    .collect();
                if lines.is_empty() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(""));
                lines
            }
            ChatMessage::ToolCall { name, detail, parallel_tag } => {
                let tag = if parallel_tag.is_empty() { String::new() } else { format!("{} ", parallel_tag) };
                vec![
                    Line::from(Span::styled(
                        format!("┌─ {tag}{name}: `{detail}`"),
                        Style::default().fg(Color::Yellow),
                    )),
                ]
            }
            ChatMessage::ToolResult { output, parallel_tag } => {
                let tag = if parallel_tag.is_empty() { String::new() } else { format!("{} ", parallel_tag) };
                let mut lines: Vec<Line> = output.lines()
                    .map(|l| Line::from(Span::styled(
                        format!("│  {tag}{l}"),
                        Style::default().fg(Color::DarkGray),
                    )))
                    .collect();
                lines.push(Line::from(Span::styled(
                    "└─",
                    Style::default().fg(Color::DarkGray),
                )));
                lines
            }
            ChatMessage::Error(text) => {
                vec![
                    Line::from(Span::styled(
                        format!("Error: {text}"),
                        Style::default().fg(Color::Red),
                    )),
                    Line::from(""),
                ]
            }
        }
    }).collect();

    // 计算滚动偏移，确保最新内容可见
    let content_height = messages.len() as u16;
    let visible_height = area.height.saturating_sub(2) as u16; // 减去边框
    let scroll_offset = if content_height > visible_height {
        content_height - visible_height
    } else {
        0
    };
    let effective_scroll = app.chat_scroll.max(scroll_offset);

    let paragraph = Paragraph::new(messages)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" 聊天记录 "),
        )
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll, 0));

    frame.render_widget(paragraph, area);
}
```

- [ ] **Step 2: 创建 `ui/status.rs`**

```rust
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use crate::app::App;

/// 渲染状态栏
pub fn draw(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mode = if app.agent_running { "响应中" } else { "输入" };
    let mode_style = if app.agent_running {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };

    let line = Line::from(vec![
        Span::styled(format!(" {} ", mode), mode_style),
        Span::raw(" │ "),
        Span::styled(
            format!("模型: {}", app.model),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" │ "),
        Span::styled(
            "Enter=提交 \\+Enter=换行 Ctrl+C=退出 PgUp/PgDn=滚动",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(paragraph, area);
}
```

- [ ] **Step 3: Commit**

```bash
git add crates/cli/src/ui/chat.rs crates/cli/src/ui/status.rs
git commit -m "feat(tui): 添加聊天记录和状态栏组件"
```

---

### Task 4: 创建 App struct 和事件处理逻辑

**Files:**
- Create: `crates/cli/src/app.rs`

- [ ] **Step 1: 创建 `app.rs`**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::command::CommandDispatcher;
use rust_agent_core::context::ContextService;
use tokio::sync::{mpsc, oneshot};

use crate::event::AppEvent;
use crate::ui::input::{Cursor, InputBox};

/// 聊天消息
#[derive(Debug, Clone)]
pub enum ChatMessage {
    /// 用户输入
    User(String),
    /// Agent 回复（累积文本）
    Assistant(String),
    /// 工具调用
    ToolCall {
        name: String,
        detail: String,
        parallel_tag: String,
    },
    /// 工具结果
    ToolResult {
        output: String,
        parallel_tag: String,
    },
    /// 错误信息
    Error(String),
}

/// 应用模式
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// 等待用户输入
    Input,
    /// Agent 正在处理
    Running,
}

/// 应用主状态
pub struct App {
    /// Agent 应用（用于调用 LLM）
    pub agent_app: AgentApp,
    /// 对话上下文
    pub ctx: ContextService,
    /// 聊天记录
    pub messages: Vec<ChatMessage>,
    /// 输入框
    pub input: InputBox,
    /// 聊天记录滚动偏移
    pub chat_scroll: u16,
    /// 当前模式
    pub mode: AppMode,
    /// Agent 是否正在运行
    pub agent_running: bool,
    /// 模型名称
    pub model: String,
    /// 是否应该退出
    pub should_quit: bool,
    /// Agent 事件接收端
    pub agent_rx: Option<mpsc::Receiver<AgentEvent>>,
    /// Agent 结果接收端
    pub result_rx: Option<oneshot::Receiver<(anyhow::Result<String>, ContextService)>>,
    /// 当前正在累积的 Agent 回复文本
    pub current_reply: String,
}

impl App {
    pub fn new(agent_app: AgentApp) -> Self {
        let model = agent_app.model().to_owned();
        Self {
            agent_app,
            ctx: ContextService::new(),
            messages: Vec::new(),
            input: InputBox::new(),
            chat_scroll: 0,
            mode: AppMode::Input,
            agent_running: false,
            model,
            should_quit: false,
            agent_rx: None,
            result_rx: None,
            current_reply: String::new(),
        }
    }

    /// 处理键盘事件
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return;
        }

        // Ctrl+C: 退出
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Agent 运行中时，只接受 Ctrl+C 和 PgUp/PgDn
        if self.agent_running {
            match key.code {
                KeyCode::PageUp => {
                    self.chat_scroll = self.chat_scroll.saturating_sub(5);
                }
                KeyCode::PageDown => {
                    self.chat_scroll = self.chat_scroll.saturating_add(5);
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Enter => {
                // 检查当前行是否以 \ 结尾 → 换行
                let current_line = &self.input.lines[self.input.cursor.row];
                if current_line.ends_with('\\') {
                    // 去掉末尾 \，插入换行
                    self.input.lines[self.input.cursor.row].pop();
                    self.input.newline();
                } else {
                    // 提交
                    self.submit_input();
                }
            }
            KeyCode::Backspace => {
                self.input.backspace();
            }
            KeyCode::Delete => {
                self.input.delete();
            }
            KeyCode::Left => self.input.move_left(),
            KeyCode::Right => self.input.move_right(),
            KeyCode::Up => self.input.move_up(),
            KeyCode::Down => self.input.move_down(),
            KeyCode::Home => self.input.move_home(),
            KeyCode::End => self.input.move_end(),
            KeyCode::PageUp => {
                self.chat_scroll = self.chat_scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.chat_scroll = self.chat_scroll.saturating_add(5);
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+U: 清空当前行
                    if c == 'u' {
                        self.input.clear_line();
                    }
                } else {
                    self.input.insert_char(c);
                }
            }
            _ => {}
        }
    }

    /// 提交用户输入
    fn submit_input(&mut self) {
        let text = self.input.text().trim().to_owned();
        if text.is_empty() {
            return;
        }

        // 退出命令
        if matches!(text.as_str(), "q" | "quit" | "exit") {
            self.should_quit = true;
            self.input.clear();
            return;
        }

        // /skills 命令
        if text == "/skills" {
            self.messages.push(ChatMessage::User(text.clone()));
            let skills = self.agent_app.list_skills();
            if skills.is_empty() {
                self.messages.push(ChatMessage::Assistant("（没有已安装的技能）".to_owned()));
            } else {
                let mut output = format!("已安装的技能（{} 个）：\n", skills.len());
                for s in &skills {
                    let desc = if s.description.is_empty() { String::new() } else { format!(": {}", s.description) };
                    let tags = if s.tags.is_empty() { String::new() } else { format!(" [{}]", s.tags) };
                    output.push_str(&format!("  - {}{desc}{tags}\n", s.name));
                }
                self.messages.push(ChatMessage::Assistant(output));
            }
            self.input.clear();
            return;
        }

        // 命令分发
        if let Some(cmd) = CommandDispatcher::parse(&text) {
            self.messages.push(ChatMessage::User(text.clone()));
            // 命令需要异步执行，这里用标记让主循环处理
            // 暂时先用同步方式处理（后续可优化）
            self.input.clear();
            return;
        }

        // 普通对话：启动 agent task
        self.messages.push(ChatMessage::User(text.clone()));
        self.input.clear();
        self.agent_running = true;
        self.mode = AppMode::Running;
        self.current_reply = String::new();

        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(64);
        let (result_tx, result_rx) = oneshot::channel();

        self.agent_rx = Some(event_rx);
        self.result_rx = Some(result_rx);

        let agent_app = self.agent_app.clone();
        let mut ctx = self.ctx.clone();
        let input = text;

        tokio::spawn(async move {
            let result = agent_app.handle_user_turn(&mut ctx, &input, event_tx).await;
            let _ = result_tx.send((result, ctx));
        });
    }

    /// 处理 Agent 事件
    pub fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(delta) => {
                self.current_reply.push_str(&delta);
            }
            AgentEvent::ToolCall { name, input, parallel_index } => {
                // 先保存当前累积的回复
                if !self.current_reply.is_empty() {
                    self.messages.push(ChatMessage::Assistant(std::mem::take(&mut self.current_reply)));
                }

                let detail = match name.as_str() {
                    "bash" => input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                    "read_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                    "write_file" => format!("{} ({} 字节)",
                        input.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                        input.get("content").map(|v| v.as_str().map(|s| s.len()).unwrap_or(0)).unwrap_or(0)),
                    "edit_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                    "glob" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                    "grep" => {
                        let mut parts = vec![input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned()];
                        if let Some(p) = input.get("path").and_then(|v| v.as_str()) {
                            parts.push(p.to_owned());
                        }
                        parts.join(" in ")
                    }
                    "todo" => {
                        let items = input.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                        format!("{items} 项")
                    }
                    "task" => input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                    _ => input.to_string(),
                };

                let parallel_tag = match parallel_index {
                    Some((idx, total)) => format!("[并行 {idx}/{total}]"),
                    None => String::new(),
                };

                self.messages.push(ChatMessage::ToolCall {
                    name,
                    detail,
                    parallel_tag,
                });
            }
            AgentEvent::ToolResult { output, parallel_index } => {
                let parallel_tag = match parallel_index {
                    Some((idx, total)) => format!("[并行 {idx}/{total}]"),
                    None => String::new(),
                };

                self.messages.push(ChatMessage::ToolResult {
                    output,
                    parallel_tag,
                });
            }
            AgentEvent::TurnEnd => {}
            AgentEvent::Done => {}
        }
    }

    /// 处理 Agent 完成
    pub fn handle_agent_done(&mut self, result: Result<String, anyhow::Error>, updated_ctx: ContextService) {
        // 保存最后累积的回复
        if !self.current_reply.is_empty() {
            self.messages.push(ChatMessage::Assistant(std::mem::take(&mut self.current_reply)));
        }

        match result {
            Ok(text) => {
                if !text.trim().is_empty() && self.current_reply.is_empty() {
                    // 回复文本在 TextDelta 中已经累积，这里不需要再添加
                }
            }
            Err(error) => {
                self.messages.push(ChatMessage::Error(error.to_string()));
            }
        }

        self.ctx = updated_ctx;
        self.agent_running = false;
        self.mode = AppMode::Input;
        self.agent_rx = None;
        self.result_rx = None;
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/cli/src/app.rs
git commit -m "feat(tui): 添加 App struct 和事件处理逻辑"
```

---

### Task 5: 重写 main.rs，整合所有组件

**Files:**
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: 重写 main.rs**

```rust
mod app;
mod event;
mod ui;

use std::io;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, EventStream};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use rust_agent_core::agent::AgentApp;

use crate::app::App;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化 agent 应用
    let agent_app = AgentApp::from_env().await?;

    rust_agent_core::infra::usage::UsageTracker::display_with_quotas(agent_app.quotas());

    // 初始化终端
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(agent_app);
    let mut events = EventStream::new();

    // 主事件循环
    loop {
        // 处理 agent 事件（非阻塞）
        if let Some(rx) = &mut app.agent_rx {
            while let Ok(event) = rx.try_recv() {
                app.handle_agent_event(event);
            }
        }

        // 检查 agent 是否完成
        if let Some(rx) = &mut app.result_rx {
            if let Ok(result) = rx.try_recv() {
                let (res, ctx) = result;
                app.handle_agent_done(res, ctx);
            }
        }

        // 渲染
        terminal.draw(|frame| ui::draw(frame, &app))?;

        // 异步等待下一个事件
        let event = events.next().await;
        match event {
            Some(Ok(crossterm::event::Event::Key(key))) => {
                app.handle_key(key);
            }
            Some(Ok(crossterm::event::Event::Resize(_, _))) => {
                // ratatui 自动处理 resize
            }
            Some(Err(_)) => {}
            None => {}
        }

        if app.should_quit {
            break;
        }
    }

    // 恢复终端
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
```

- [ ] **Step 2: 运行 cargo check**

Run: `cargo check -p rust-agent-cli 2>&1 | tail -20`
Expected: 可能有小错误需要修复（import 路径、trait 约束等）

- [ ] **Step 3: 修复编译错误，直到 cargo check 通过**

根据具体错误信息修复。

- [ ] **Step 4: 运行 cargo build**

Run: `cargo build -p rust-agent-cli 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "feat(tui): 重写 main.rs，整合 ratatui 事件循环"
```

---

### Task 6: 集成测试和修复

**Files:**
- Modify: `crates/cli/src/main.rs`（可能的小修复）
- Modify: `crates/cli/src/app.rs`（可能的小修复）
- Modify: `crates/cli/src/ui/*.rs`（可能的小修复）

- [ ] **Step 1: 手动测试基本功能**

Run: `cargo run -p rust-agent-cli`

测试清单：
- [ ] 启动后显示三栏布局（聊天记录 + 输入框 + 状态栏）
- [ ] 在输入框中输入文字，光标可移动
- [ ] `\` + Enter 换行
- [ ] Enter 提交，Agent 响应流式显示
- [ ] 工具调用显示正确
- [ ] Ctrl+C 退出
- [ ] 终端恢复正常状态（无乱码）

- [ ] **Step 2: 修复发现的问题**

根据手动测试结果修复。

- [ ] **Step 3: Commit 修复**

```bash
git add -A crates/cli/
git commit -m "fix(tui): 修复集成测试发现的问题"
```

---

### Task 7: 清理和最终提交

- [ ] **Step 1: 移除 Cargo.lock 中的旧依赖**

Run: `cargo build -p rust-agent-cli`
Expected: 编译成功，Cargo.lock 自动更新

- [ ] **Step 2: 运行全量编译检查**

Run: `cargo check 2>&1 | tail -5`
Expected: 全部通过

- [ ] **Step 3: 最终 Commit**

```bash
git add -A
git commit -m "chore: 清理 TUI 重构后的残余"
```
