//! 命令处理器实现

use super::{CommandResult, UserCommand};
use crate::context::ContextService;

pub(super) async fn handle(
    cmd: UserCommand,
    ctx: &mut ContextService,
    client: Option<&crate::api::LlmProvider>,
    model: &str,
    workspace_root: &std::path::Path,
) -> CommandResult {
    match cmd {
        UserCommand::Clear => handle_clear(ctx),
        UserCommand::Compact => handle_compact(ctx, client, model, workspace_root).await,
        UserCommand::Stats => handle_stats(ctx),
        UserCommand::Help => handle_help(),
        UserCommand::Quit => CommandResult {
            output: String::new(),
            should_quit: true,
        },
    }
}

fn handle_clear(ctx: &mut ContextService) -> CommandResult {
    let stats = ctx.clear();
    let cleared = stats.cleared_count.unwrap_or(0);
    CommandResult {
        output: format!("上下文已清空（清除 {cleared} 条消息）"),
        should_quit: false,
    }
}

async fn handle_compact(
    ctx: &mut ContextService,
    client: Option<&crate::api::LlmProvider>,
    model: &str,
    workspace_root: &std::path::Path,
) -> CommandResult {
    let Some(client) = client else {
        return CommandResult {
            output: "压缩功能不可用（缺少 LLM 客户端）".to_owned(),
            should_quit: false,
        };
    };

    match ctx.auto_compact(client, model, workspace_root).await {
        Ok(new_messages) => {
            let before = ctx.len();
            ctx.replace(new_messages);
            let after = ctx.len();
            CommandResult {
                output: format!("压缩完成（{before} 条 → {after} 条）"),
                should_quit: false,
            }
        }
        Err(e) => CommandResult {
            output: format!("压缩失败: {e}"),
            should_quit: false,
        },
    }
}

fn handle_stats(ctx: &ContextService) -> CommandResult {
    let stats = ctx.stats();
    CommandResult {
        output: format!(
            "消息数: {} | 预估 token: {}",
            stats.message_count, stats.estimated_tokens
        ),
        should_quit: false,
    }
}

fn handle_help() -> CommandResult {
    CommandResult {
        output: concat!(
            "可用命令：\n",
            "  /clear    清空对话历史\n",
            "  /compact  手动压缩上下文\n",
            "  /stats    显示上下文统计\n",
            "  /help     显示此帮助\n",
            "  /quit     退出程序",
        )
        .to_owned(),
        should_quit: false,
    }
}
