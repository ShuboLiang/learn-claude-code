pub mod agent;
pub mod api;
pub mod command;
pub mod context;
pub mod infra;
pub mod skills;
pub mod tools;

/// 统一的 Agent 结果类型别名，简化错误处理
pub type AgentResult<T> = anyhow::Result<T>;

/// 重新导出 tokio mpsc channel，供 CLI 和 server 使用
pub use tokio::sync::mpsc;

// ── 公共 API 统一导出 ──
pub use agent::{AgentApp, AgentEvent};
pub use api::types::{ApiMessage, ProviderRequest, ProviderResponse, ResponseContentBlock, TokenUsage};
pub use infra::token_tracker::{TokenTracker, TokenSnapshot};
pub use api::{LlmProvider, ProviderInfo};
pub use command::{CommandDispatcher, CommandResult, UserCommand};
pub use context::ContextService;
pub use infra::todo::TodoManager;
pub use skills::SkillLoader;
pub use tools::extension::ToolExtension;
