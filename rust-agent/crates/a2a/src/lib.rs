pub mod executor;

use std::sync::Arc;

use a2a::*;
use a2a_server::{
    DefaultRequestHandler, InMemoryTaskStore, StaticAgentCard,
};
use axum::{extract::Request, middleware::Next, response::Response};
use rust_agent_core::agent::AgentApp;

use crate::executor::RustAgentExecutor;

/// 日志中间件：打印每个请求的方法、路径和响应状态码
async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let response = next.run(req).await;
    let status = response.status();
    println!("[A2A] {} {} -> {}", method, uri, status.as_u16());
    response
}

/// Build the axum app using the a2a-rs SDK.
pub async fn app(base_url: &str) -> anyhow::Result<axum::Router> {
    let agent = AgentApp::from_env().await?;
    let skill_summaries = agent.list_skills();

    let executor = RustAgentExecutor::new(agent);
    let task_store = InMemoryTaskStore::new();
    let handler = Arc::new(
        DefaultRequestHandler::new(executor, task_store).with_capabilities(AgentCapabilities {
            streaming: Some(true),
            push_notifications: Some(false),
            extensions: None,
            extended_agent_card: None,
        }),
    );

    let agent_card = build_agent_card(base_url, &skill_summaries);
    let card_producer = Arc::new(StaticAgentCard::new(agent_card));

    let app = axum::Router::new()
        .nest("/jsonrpc", a2a_server::jsonrpc::jsonrpc_router(handler.clone()))
        .merge(a2a_server::rest::rest_router(handler))
        .merge(a2a_server::agent_card::agent_card_router(card_producer))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(axum::middleware::from_fn(log_request));

    Ok(app)
}

fn build_agent_card(base_url: &str, skill_summaries: &[rust_agent_core::skills::SkillSummary]) -> AgentCard {
    let skills: Vec<AgentSkill> = skill_summaries
        .iter()
        .map(|s| {
            let tags = if s.tags.is_empty() {
                Vec::new()
            } else {
                s.tags
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            };
            AgentSkill {
                id: s.name.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                tags,
                examples: None,
                input_modes: None,
                output_modes: None,
                security_requirements: None,
            }
        })
        .collect();

    AgentCard {
        name: "rust-agent".to_string(),
        description: "A Rust-based programming assistant with tool execution capabilities."
            .to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        supported_interfaces: vec![
            AgentInterface::new(format!("{}/jsonrpc", base_url), TRANSPORT_PROTOCOL_JSONRPC),
            AgentInterface::new(base_url.to_string(), TRANSPORT_PROTOCOL_HTTP_JSON),
        ],
        capabilities: AgentCapabilities {
            streaming: Some(true),
            push_notifications: Some(false),
            extensions: None,
            extended_agent_card: None,
        },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills,
        provider: None,
        documentation_url: None,
        icon_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}
