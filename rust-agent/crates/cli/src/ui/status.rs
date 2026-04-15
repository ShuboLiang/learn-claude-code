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
