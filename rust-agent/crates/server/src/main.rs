use axum::Router;
use tower_http::cors::CorsLayer;

mod routes;
mod session;
mod sse;
mod openai_compat;

use session::SessionStore;

/// 启动 HTTP API 服务
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    // 解析命令行参数
    let port: u16 = std::env::args()
        .skip_while(|arg| arg != "--port")
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let store = SessionStore::new();
    let app = Router::new()
        .merge(routes::routes(store))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("rust-agent-server 启动在 http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
