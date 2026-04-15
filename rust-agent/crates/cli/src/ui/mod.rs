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
            ratatui::layout::Constraint::Min(1),    // 聊天记录
            ratatui::layout::Constraint::Length(3), // 输入框
            ratatui::layout::Constraint::Length(1), // 状态栏
        ])
        .split(frame.area());

    chat::draw(frame, app, chunks[0]);
    input::draw(frame, app, chunks[1]);
    status::draw(frame, app, chunks[2]);
}
