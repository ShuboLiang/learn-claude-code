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
use rust_agent_core::agent::{AgentApp, AgentEvent};

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
        // 先收集所有待处理事件，避免借用冲突
        let agent_events: Vec<AgentEvent> = if let Some(rx) = &mut app.agent_rx {
            let mut evts = Vec::new();
            while let Ok(event) = rx.try_recv() {
                evts.push(event);
            }
            evts
        } else {
            Vec::new()
        };
        for event in agent_events {
            app.handle_agent_event(event);
        }

        // 检查 agent 是否完成
        let agent_result = if let Some(rx) = &mut app.result_rx {
            rx.try_recv().ok()
        } else {
            None
        };
        if let Some((res, ctx)) = agent_result {
            app.handle_agent_done(res, ctx);
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
            _ => {}
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
