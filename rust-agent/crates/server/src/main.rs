use axum::Router;
use tower_http::cors::CorsLayer;

mod routes;
mod session;
mod sse;

use session::SessionStore;

/// 启动 HTTP API 服务
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let store = SessionStore::new();
    let app = Router::new()
        .merge(routes::routes(store))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("rust-agent-server 启动在 http://localhost:3000");
    axum::serve(listener, app).await?;
    Ok(())
}
