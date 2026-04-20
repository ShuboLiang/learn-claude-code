use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::errors::{A2AError, A2AErrorResponse};
use crate::handlers;
use crate::state::AppState;

async fn a2a_version_check(req: Request, next: Next) -> Result<Response, A2AErrorResponse> {
    let version = req
        .headers()
        .get("A2A-Version")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            req.uri()
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find_map(|pair| {
                            let mut parts = pair.splitn(2, '=');
                            let key = parts.next()?;
                            let value = parts.next()?;
                            if key.eq_ignore_ascii_case("A2A-Version") {
                                Some(value.to_string())
                            } else {
                                None
                            }
                        })
                })
        });

    if let Some(ref v) = version {
        if v.is_empty() {
            // Empty version is treated as 0.3 — skip validation.
            return Ok(next.run(req).await);
        }
        if v != "1.0" {
            return Err(A2AErrorResponse {
                error: A2AError::version_not_supported(v),
            });
        }
    }
    Ok(next.run(req).await)
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/.well-known/agent.json", get(handlers::get_agent_card))
        .route("/message:send", post(handlers::send_message))
        .route("/message:stream", post(handlers::send_message_stream))
        .route("/tasks", get(handlers::list_tasks))
        .route("/tasks/{taskId}/subscribe", post(handlers::subscribe_task))
        .route(
            "/tasks/{taskId}/pushNotificationConfigs",
            post(handlers::create_push_config).get(handlers::list_push_configs),
        )
        .route(
            "/tasks/{taskId}/pushNotificationConfigs/{configId}",
            get(handlers::get_push_config).delete(handlers::delete_push_config),
        )
        .route("/tasks/{taskId}/cancel", post(handlers::cancel_task))
        .route("/tasks/{taskId}", get(handlers::get_task))
        .route("/extendedAgentCard", get(handlers::get_extended_agent_card))
        .layer(middleware::from_fn(a2a_version_check))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
