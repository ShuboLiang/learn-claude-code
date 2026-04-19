//! 命令域：解析和执行用户指令（/clear、/compact、/stats、/help、/quit）

mod handlers;

use crate::context::ContextService;

/// 用户指令枚举
#[derive(Clone, Debug)]
pub enum UserCommand {
    /// 清空对话历史
    Clear,
    /// 手动触发压缩
    Compact,
    /// 显示上下文统计
    Stats,
    /// 显示帮助信息
    Help,
    /// 退出程序
    Quit,
}

/// 命令执行结果
#[derive(Clone, Debug)]
pub struct CommandResult {
    /// 给用户的反馈文本
    pub output: String,
    /// 是否退出程序
    pub should_quit: bool,
}

/// 命令分发器（无状态，通过方法参数传入依赖）
pub struct CommandDispatcher;

impl CommandDispatcher {
    /// 解析用户输入，匹配 /command 格式
    pub fn parse(input: &str) -> Option<UserCommand> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        match trimmed.to_lowercase().as_str() {
            "/clear" => Some(UserCommand::Clear),
            "/compact" => Some(UserCommand::Compact),
            "/stats" => Some(UserCommand::Stats),
            "/help" | "/?" | "/h" => Some(UserCommand::Help),
            "/quit" | "/exit" | "/q" => Some(UserCommand::Quit),
            _ => None,
        }
    }

    /// 执行命令，返回结果
    pub async fn execute(
        cmd: UserCommand,
        ctx: &mut ContextService,
        client: Option<&crate::api::LlmProvider>,
        model: &str,
        workspace_root: &std::path::Path,
    ) -> CommandResult {
        handlers::handle(cmd, ctx, client, model, workspace_root).await
    }
}