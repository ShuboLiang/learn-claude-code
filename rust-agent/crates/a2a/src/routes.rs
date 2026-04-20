use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::handlers;
use crate::state::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/.well-known/agent.json", get(handlers::get_agent_card))
        .route("/tasks/send", post(handlers::send_task))
        .route("/tasks/sendSubscribe", post(handlers::send_task_subscribe))
        .route("/tasks/{taskId}", get(handlers::get_task))
        .route("/tasks/{taskId}/send", post(handlers::send_task_followup))
        .route("/tasks/{taskId}/cancel", post(handlers::cancel_task))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
