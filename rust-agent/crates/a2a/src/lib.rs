use std::sync::Arc;

pub mod agent_card;
pub mod handlers;
pub mod routes;
pub mod state;
pub mod streaming;
pub mod task_runner;
pub mod types;

use rust_agent_core::agent::AgentApp;

/// Build the axum app for testing or custom serving.
pub async fn app(base_url: &str) -> anyhow::Result<axum::Router> {
    let agent = AgentApp::from_env().await?;
    let tool_schemas = agent.tool_schemas();
    let agent_card = agent_card::build_agent_card(base_url, &tool_schemas);

    let state = Arc::new(state::AppState {
        tasks: Arc::new(dashmap::DashMap::new()),
        contexts: Arc::new(dashmap::DashMap::new()),
        agent: agent.clone(),
        agent_card,
    });

    Ok(routes::routes(state))
}
