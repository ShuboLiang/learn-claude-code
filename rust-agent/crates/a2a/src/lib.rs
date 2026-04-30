pub mod executor;
pub mod extension;

use std::sync::Arc;

use a2a::*;
use a2a_server::{DefaultRequestHandler, InMemoryTaskStore};
use axum::body::{Body, to_bytes};
use axum::{Extension, Json, extract::Request, middleware::Next, response::Response};
use rust_agent_core::agent::AgentApp;
use serde_json::json;
use tracing::info;

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

    info!("[A2A] {} {} -> {}", method, uri, status.as_u16());
    if !req_body_str.is_empty() {
        info!("[A2A] Request Body:\n{}", req_body_str);
    }
    if !resp_body_str.is_empty() {
        info!("[A2A] Response Body:\n{}", resp_body_str);
    }

    // 重建响应
    Response::from_parts(parts, Body::from(resp_bytes))
}

/// Build the axum app using the a2a-rs SDK.
pub async fn app(base_url: &str) -> anyhow::Result<axum::Router> {
    let agent = AgentApp::from_env()
        .await?
        .with_extension(Arc::new(extension::WeatherToolExtension));
    let identity = agent.identity().clone();
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

    let agent_card = Arc::new(build_agent_card(base_url, &skill_summaries, &identity));

    let app = axum::Router::new()
        .merge(a2a_server::jsonrpc::jsonrpc_router(handler.clone()))
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

async fn agent_card_handler(Extension(card): Extension<Arc<AgentCard>>) -> Json<serde_json::Value> {
    // 先序列化成标准 A2A 的 JSON（含 supportedInterfaces 等）
    let mut value = serde_json::to_value(&*card).unwrap();

    // 再注入模板要求的额外顶层字段
    if let Some(obj) = value.as_object_mut() {
        // 移除标准 A2A 的 supportedInterfaces（模板里没有这个字段）
        obj.remove("supportedInterfaces");

        obj.insert("preferredTransport".to_string(), json!("JSONRPC"));
        obj.insert("protocolVersion".to_string(), json!("0.3.0"));
        // 顶层 url 直接用 base_url（不带 /jsonrpc 后缀）
        if let Some(http_json) = card
            .supported_interfaces
            .iter()
            .find(|i| i.protocol_binding == TRANSPORT_PROTOCOL_HTTP_JSON)
        {
            obj.insert("url".to_string(), json!(http_json.url.clone()));
        }
    }

    Json(value)
}

fn build_agent_card(
    base_url: &str,
    skill_summaries: &[rust_agent_core::skills::SkillSummary],
    identity: &rust_agent_core::agent::AgentIdentity,
) -> AgentCard {
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

    let (name, description) = if identity.nickname.is_empty() && identity.role.is_empty() {
        (
            "rust-agent".to_string(),
            "A Rust-based programming assistant with tool execution capabilities.".to_string(),
        )
    } else if identity.role.is_empty() {
        (
            identity.nickname.clone(),
            format!("Agent '{}' powered by rust-agent.", identity.nickname),
        )
    } else if identity.nickname.is_empty() {
        (
            identity.role.clone(),
            format!("A {} assistant powered by rust-agent.", identity.role),
        )
    } else {
        (
            identity.display_name(),
            format!(
                "{}，一个由 rust-agent 驱动的{}助手。",
                identity.nickname, identity.role
            ),
        )
    };

    AgentCard {
        name,
        description,
        version: env!("CARGO_PKG_VERSION").to_string(),
        supported_interfaces: vec![
            AgentInterface::new(base_url.to_string(), TRANSPORT_PROTOCOL_JSONRPC),
            AgentInterface::new(base_url.to_string(), TRANSPORT_PROTOCOL_HTTP_JSON),
        ],
        capabilities: AgentCapabilities {
            streaming: Some(true),
            push_notifications: Some(false),
            extensions: None,
            extended_agent_card: None,
        },
        default_input_modes: vec!["text".to_string(), "text/plain".to_string()],
        default_output_modes: vec!["text".to_string(), "text/plain".to_string()],
        skills,
        provider: None,
        documentation_url: None,
        icon_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}
