use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    let port = std::env::var("A2A_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3001u16);

    let base_url =
        std::env::var("A2A_BASE_URL").unwrap_or_else(|_| format!("http://localhost:{}", port));

    let app = rust_agent_a2a::app(&base_url)
        .await
        .expect("Failed to build app");

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("rust-agent-a2a listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
