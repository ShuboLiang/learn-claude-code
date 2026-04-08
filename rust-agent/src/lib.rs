pub mod agent;
pub mod anthropic;
pub mod skills;
pub mod todo;
pub mod tools;
pub mod workspace;

/// 统一的 Agent 结果类型别名，简化错误处理
pub type AgentResult<T> = anyhow::Result<T>;

/// 库的公共入口函数：启动 Agent 的 REPL 交互循环
///
/// # 使用场景
/// 被 `main.rs` 调用，是整个程序的真正入口
///
/// # 运作原理
/// 直接委托给 `agent::run_repl()`
pub async fn run_repl() -> AgentResult<()> {
    agent::run_repl().await
}
