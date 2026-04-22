pub mod executor;

use std::sync::Arc;

use a2a::*;
use a2a_server::{
    DefaultRequestHandler, InMemoryTaskStore,
};
use axum::body::{to_bytes, Body};
use axum::{extract::Request, middleware::Next, response::Response, Extension, Json};
use rust_agent_core::agent::AgentApp;
use serde::Serialize;

use crate::executor::RustAgentExecutor;

const MAX_BODY_SIZE: usize = 10 * 1024 * 1024; // 10MB

/// 日志中间件：打印每个请求的方法、路径、响应状态码、入参和出参
async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();

    // 读取请求体
    let (parts, body) = req.into_parts();
    let req_bytes = to_bytes(body, MAX_BODY_SIZE).await.unwrap_or_default();
    let req_body_str = String::from_utf8_lossy(&req_bytes).to_string();

    // 重建请求
    let req = Request::from_parts(parts, Body::from(req_bytes));

    let response = next.run(req).await;
    let status = response.status();

    // 读取响应体
    let (parts, body) = response.into_parts();
    let resp_bytes = to_bytes(body, MAX_BODY_SIZE).await.unwrap_or_default();
    let resp_body_str = String::from_utf8_lossy(&resp_bytes).to_string();

    println!("[A2A] {} {} -> {}", method, uri, status.as_u16());
    if !req_body_str.is_empty() {
        println!("[A2A] Request Body:\n{}", req_body_str);
    }
    if !resp_body_str.is_empty() {
        println!("[A2A] Response Body:\n{}", resp_body_str);
    }

    // 重建响应
    Response::from_parts(parts, Body::from(resp_bytes))
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

    let agent_card = Arc::new(build_agent_card(base_url, &skill_summaries));

    let app = axum::Router::new()
        .nest("/jsonrpc", a2a_server::jsonrpc::jsonrpc_router(handler.clone()))
        .merge(a2a_server::rest::rest_router(handler))
        .route(
            "/.well-known/agent-card.json",
            axum::routing::get(agent_card_handler),
        )
        .layer(axum::Extension(agent_card))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(axum::middleware::from_fn(log_request));

    Ok(app)
}

async fn agent_card_handler(
    Extension(card): Extension<Arc<AgentCardTemplate>>,
) -> Json<AgentCardTemplate> {
    Json((*card).clone())
}

/// 匹配目标模板格式的 AgentCard
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AgentCardTemplate {
    pub capabilities: Capabilities,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
    pub description: String,
    pub name: String,
    pub preferred_transport: String,
    pub protocol_version: String,
    pub skills: Vec<SkillTemplate>,
    pub url: String,
    pub version: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    pub push_notifications: bool,
    pub streaming: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SkillTemplate {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<String>>,
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
}

fn build_agent_card(
    base_url: &str,
    skill_summaries: &[rust_agent_core::skills::SkillSummary],
) -> AgentCardTemplate {
    let skills: Vec<SkillTemplate> = skill_summaries
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
            SkillTemplate {
                id: s.name.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                tags,
                examples: None,
            }
        })
        .collect();

    AgentCardTemplate {
        name: "rust-agent".to_string(),
        description: "A Rust-based programming assistant with tool execution capabilities."
            .to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        url: base_url.to_string(),
        preferred_transport: "JSONRPC".to_string(),
        protocol_version: "0.3.0".to_string(),
        capabilities: Capabilities {
            streaming: true,
            push_notifications: false,
        },
        default_input_modes: vec!["text".to_string(), "text/plain".to_string()],
        default_output_modes: vec!["text".to_string(), "text/plain".to_string()],
        skills,
    }
}
