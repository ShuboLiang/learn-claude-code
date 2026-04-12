pub mod agent;
pub mod anthropic;
pub mod skillhub;
pub mod skills;
pub mod todo;
pub mod tools;
pub mod workspace;

/// 统一的 Agent 结果类型别名，简化错误处理
pub type AgentResult<T> = anyhow::Result<T>;

/// 重新导出 tokio mpsc channel，供 CLI 和 server 使用
pub use tokio::sync::mpsc;
