pub mod agent;
pub mod anthropic;
pub mod skills;
pub mod todo;
pub mod tools;
pub mod workspace;

pub type AgentResult<T> = anyhow::Result<T>;

pub async fn run_repl() -> AgentResult<()> {
    agent::run_repl().await
}
