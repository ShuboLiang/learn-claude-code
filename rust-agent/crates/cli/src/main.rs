use rustyline::error::ReadlineError;
use rustyline::{Cmd, Config, DefaultEditor, Event, KeyCode, KeyEvent, Modifiers};

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

    // 启动时显示用量状态
    rust_agent_core::infra::usage::UsageTracker::display_with_quotas(app.quotas());

    let mut history = Vec::new();
    let config = Config::builder()
        .bracketed_paste(true)
        .build();
    let mut rl = DefaultEditor::with_config(config)?;

    // Enter = 提交，Ctrl+Enter = 插入换行
    rl.bind_sequence(
        Event::from(KeyEvent(KeyCode::Enter, Modifiers::NONE)),
        Cmd::AcceptLine,
    );
    rl.bind_sequence(
        Event::from(KeyEvent(KeyCode::Enter, Modifiers::CTRL)),
        Cmd::Newline,
    );

    loop {
        let line = match rl.readline("agent >> ") {
            Ok(line) => line,
            Err(ReadlineError::Eof | ReadlineError::Interrupted) => break,
            Err(e) => return Err(e.into()),
        };

        let query = line.trim();
        if query.is_empty() || matches!(query, "q" | "quit" | "exit") {
            break;
        }

        // 拦截 /skills 命令，直接在 CLI 层展示，不走 LLM
        if query == "/skills" {
            rl.add_history_entry(query)?;
            let skills = app.list_skills();
            if skills.is_empty() {
                println!("（没有已安装的技能）");
            } else {
                println!("已安装的技能（{} 个）：", skills.len());
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
                    println!("  - {}{desc}{tags}", s.name);
                }
            }
            println!();
            continue;
        }

        rl.add_history_entry(query)?;

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
                AgentEvent::ToolCall { name, input, parallel_index } => {
                    // 提取关键参数显示
                    let detail = match name.as_str() {
                        "bash" => input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "read_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "write_file" => format!("{} ({} 字节)", input.get("path").and_then(|v| v.as_str()).unwrap_or(""), input.get("content").map(|v| v.as_str().map(|s| s.len()).unwrap_or(0)).unwrap_or(0)),
                        "edit_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "glob" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "grep" => {
                            let mut parts = vec![input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned()];
                            if let Some(p) = input.get("path").and_then(|v| v.as_str()) { parts.push(p.to_owned()); }
                            parts.join(" in ")
                        }
                        "todo" => {
                            let items = input.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                            format!("{items} 项")
                        }
                        "task" => input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        _ => input.to_string(),
                    };
                    let tag = match parallel_index {
                        Some((idx, total)) => format!("[并行 {idx}/{total}] "),
                        None => String::new(),
                    };
                    println!("┌─ {tag}{name}: `{detail}`");
                }
                AgentEvent::ToolResult { name: _, output, parallel_index } => {
                    let tag = match parallel_index {
                        Some((idx, total)) => format!("[并行 {idx}/{total}] "),
                        None => String::new(),
                    };
                    for line in output.lines() {
                        println!("│  {tag}{line}");
                    }
                    println!("└─");
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

        // 每轮对话结束后刷新用量显示
        rust_agent_core::infra::usage::UsageTracker::display_with_quotas(app.quotas());
    }

    Ok(())
}
