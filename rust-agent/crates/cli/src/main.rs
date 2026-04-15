use rustyline::error::ReadlineError;
use rustyline::{Cmd, Config, DefaultEditor, Event, KeyCode, KeyEvent, Modifiers};

use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::command::CommandDispatcher;
use rust_agent_core::context::ContextService;
use rust_agent_core::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = AgentApp::from_env().await?;

    rust_agent_core::infra::usage::UsageTracker::display_with_quotas(app.quotas());

    let mut ctx = ContextService::new();
    let config = Config::builder()
        .bracketed_paste(true)
        .build();
    let mut rl = DefaultEditor::with_config(config)?;

    rl.bind_sequence(
        Event::from(KeyEvent(KeyCode::Enter, Modifiers::NONE)),
        Cmd::AcceptLine,
    );

    loop {
        // 读取输入（支持 \ 换行：输入以 \ 结尾时继续读取下一行）
        let mut lines: Vec<String> = Vec::new();
        let mut eof = false;

        loop {
            let prompt = if lines.is_empty() { "agent >> " } else { "      .. " };
            match rl.readline(prompt) {
                Ok(line) => {
                    if line.ends_with('\\') {
                        lines.push(line[..line.len() - 1].to_string());
                    } else {
                        lines.push(line);
                        break;
                    }
                }
                Err(ReadlineError::Eof | ReadlineError::Interrupted) => {
                    eof = true;
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }

        if eof {
            break;
        }

        let query = lines.join("\n").trim().to_owned();
        if query.is_empty() {
            continue;
        }
        if matches!(query.as_str(), "q" | "quit" | "exit") {
            break;
        }

        // /skills 命令由 CLI 专有处理
        if query == "/skills" {
            rl.add_history_entry(query)?;
            let skills = app.list_skills();
            if skills.is_empty() {
                println!("（没有已安装的技能）");
            } else {
                println!("已安装的技能（{} 个）：", skills.len());
                for s in &skills {
                    let desc = if s.description.is_empty() { String::new() } else { format!(": {}", s.description) };
                    let tags = if s.tags.is_empty() { String::new() } else { format!(" [{}]", s.tags) };
                    println!("  - {}{desc}{tags}", s.name);
                }
            }
            println!();
            continue;
        }

        // 通用命令分发（通过 CommandDispatcher）
        if let Some(cmd) = CommandDispatcher::parse(&query) {
            rl.add_history_entry(query)?;
            let result = CommandDispatcher::execute(
                cmd,
                &mut ctx,
                Some(app.client()),
                app.model(),
                app.quotas(),
                app.workspace_root(),
            ).await;
            if result.should_quit {
                break;
            }
            println!("{}\n", result.output);
            continue;
        }

        // 普通对话
        rl.add_history_entry(&query)?;

        let (event_tx, mut event_rx) = mpsc::channel(64);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let app_clone = app.clone();
        let input = query;
        let ctx_for_spawn = ctx.clone();

        tokio::spawn(async move {
            let mut ctx_clone = ctx_for_spawn;
            let result = app_clone.handle_user_turn(&mut ctx_clone, &input, event_tx).await;
            let _ = result_tx.send((result, ctx_clone));
        });

        // 前台渲染事件
        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::TextDelta(_) => {}
                AgentEvent::ToolCall { name, input, parallel_index } => {
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

        match result_rx.await {
            Ok((Ok(text), updated_ctx)) => {
                if !text.trim().is_empty() {
                    termimad::print_text(&text);
                }
                ctx = updated_ctx;
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

        rust_agent_core::infra::usage::UsageTracker::display_with_quotas(app.quotas());
    }

    Ok(())
}
