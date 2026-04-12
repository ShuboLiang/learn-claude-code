use std::io::{self, BufRead, Write};

use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::mpsc;

/// 启动交互式 REPL 循环
///
/// 每轮循环：
/// 1. 读取用户输入
/// 2. 创建 event channel，在后台 tokio 任务中运行 agent
/// 3. 前台渲染收到的事件（工具调用、工具结果）
/// 4. 通过 oneshot channel 取回最终结果和更新后的 history
/// 5. 用 termimad 渲染最终回复文本
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = AgentApp::from_env().await?;
    let mut history = Vec::new();
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();

    loop {
        print!("agent >> ");
        io::stdout().flush().ok();

        let mut buf = Vec::new();
        let read = stdin_lock.read_until(b'\n', &mut buf)?;
        let line = String::from_utf8_lossy(&buf).into_owned();
        if read == 0 {
            break;
        }

        let query = line.trim();
        if query.is_empty() || matches!(query, "q" | "quit" | "exit") {
            break;
        }

        let (event_tx, mut event_rx) = mpsc::channel(64);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let app_clone = app.clone();
        let mut history_clone = history.clone();
        let input = query.to_owned();

        // Agent 在后台任务中运行，通过 event_tx 发送事件，通过 result_tx 返回结果
        tokio::spawn(async move {
            let result = app_clone
                .handle_user_turn(&mut history_clone, &input, event_tx)
                .await;
            let _ = result_tx.send((result, history_clone));
        });

        // 前台渲染事件
        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::TextDelta(_) => {
                    // 流式文本暂不处理，最终结果用 termimad 渲染
                }
                AgentEvent::ToolCall { name, .. } => println!("> {}", name),
                AgentEvent::ToolResult { name, output } => {
                    println!("> {}:", name);
                    println!("{}", output);
                }
                AgentEvent::TurnEnd => {}
                AgentEvent::Done => {}
            }
        }

        // 获取最终结果和更新后的 history
        match result_rx.await {
            Ok((Ok(text), updated_history)) => {
                if !text.trim().is_empty() {
                    termimad::print_text(&text);
                }
                history = updated_history;
                println!();
            }
            Ok((Err(error), _)) => {
                eprintln!("Error: {error}");
                println!();
            }
            Err(_) => {
                eprintln!("Error: agent 任务异常终止");
                println!();
            }
        }
    }

    Ok(())
}
