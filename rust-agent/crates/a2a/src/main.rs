use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// 初始化日志：文件层 + stderr 层
fn init_logging() {
    let log_dir = dirs::home_dir()
        .map(|p| p.join(".rust-agent").join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("./logs"));

    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "a2a.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    Box::leak(Box::new(guard));

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stderr_layer)
        .init();
}

#[tokio::main]
async fn main() {
    init_logging();
    let _ = dotenvy::dotenv();

    let port = std::env::var("A2A_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3001u16);

    let base_url =
        std::env::var("A2A_BASE_URL").unwrap_or_else(|_| format!("http://0.0.0.0:{}", port));

    let app = rust_agent_a2a::app(&base_url)
        .await
        .expect("Failed to build app");

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    info!("rust-agent-a2a 启动在 http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
