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
    let visible_height = area.height.saturating_sub(2) as u16;
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
