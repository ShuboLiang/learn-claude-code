use ratatui::{
    style::{Color, Style},
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

    /// 获取完整文本（拼接所有行）
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// 获取用于显示的文本
    pub fn display_text(&self) -> String {
        self.lines.join("\n")
    }

    /// 是否为空
    #[allow(dead_code)]
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
            self.lines[row].remove(col - 1);
            self.cursor.col -= 1;
        } else if row > 0 {
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
