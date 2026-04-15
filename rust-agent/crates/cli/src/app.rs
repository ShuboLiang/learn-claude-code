use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::command::CommandDispatcher;
use rust_agent_core::context::ContextService;
use tokio::sync::{mpsc, oneshot};

use crate::ui::input::InputBox;

/// 聊天消息
#[derive(Debug, Clone)]
pub enum ChatMessage {
    /// 用户输入
    User(String),
    /// Agent 回复
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

/// 应用主状态
pub struct App {
    /// Agent 应用
    pub agent_app: AgentApp,
    /// 对话上下文
    pub ctx: ContextService,
    /// 聊天记录
    pub messages: Vec<ChatMessage>,
    /// 输入框
    pub input: InputBox,
    /// 聊天记录滚动偏移
    pub chat_scroll: u16,
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
                self.messages
                    .push(ChatMessage::Assistant("（没有已安装的技能）".to_owned()));
            } else {
                let mut output = format!("已安装的技能（{} 个）：\n", skills.len());
                for s in &skills {
                    let desc = if s.description.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", s.description)
                    };
                    let tags = if s.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", s.tags)
                    };
                    output.push_str(&format!("  - {}{desc}{tags}\n", s.name));
                }
                self.messages.push(ChatMessage::Assistant(output));
            }
            self.input.clear();
            return;
        }

        // 命令分发
        if let Some(_cmd) = CommandDispatcher::parse(&text) {
            self.messages.push(ChatMessage::User(text.clone()));
            // TODO: 异步执行命令（暂时标记为需要处理）
            self.input.clear();
            return;
        }

        // 普通对话：启动 agent task
        self.messages.push(ChatMessage::User(text.clone()));
        self.input.clear();
        self.agent_running = true;
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
            AgentEvent::ToolCall {
                name,
                input,
                parallel_index,
            } => {
                // 先保存当前累积的回复
                if !self.current_reply.is_empty() {
                    self.messages
                        .push(ChatMessage::Assistant(std::mem::take(&mut self.current_reply)));
                }

                let detail = match name.as_str() {
                    "bash" => input
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    "read_file" => input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    "write_file" => format!(
                        "{} ({} 字节)",
                        input.get("path").and_then(|v| v.as_str()).unwrap_or(""),
                        input
                            .get("content")
                            .map(|v| v.as_str().map(|s| s.len()).unwrap_or(0))
                            .unwrap_or(0)
                    ),
                    "edit_file" => input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    "glob" => input
                        .get("pattern")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    "grep" => {
                        let mut parts = vec![input
                            .get("pattern")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_owned()];
                        if let Some(p) = input.get("path").and_then(|v| v.as_str()) {
                            parts.push(p.to_owned());
                        }
                        parts.join(" in ")
                    }
                    "todo" => {
                        let items = input
                            .get("items")
                            .and_then(|v| v.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0);
                        format!("{items} 项")
                    }
                    "task" => input
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
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
            AgentEvent::ToolResult {
                output,
                parallel_index,
                ..
            } => {
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
    pub fn handle_agent_done(
        &mut self,
        result: Result<String, anyhow::Error>,
        updated_ctx: ContextService,
    ) {
        // 保存最后累积的回复
        if !self.current_reply.is_empty() {
            self.messages
                .push(ChatMessage::Assistant(std::mem::take(&mut self.current_reply)));
        }

        match result {
            Ok(_) => {}
            Err(error) => {
                self.messages.push(ChatMessage::Error(error.to_string()));
            }
        }

        self.ctx = updated_ctx;
        self.agent_running = false;
        self.agent_rx = None;
        self.result_rx = None;
    }
}
