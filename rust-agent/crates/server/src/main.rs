use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use rust_agent_core::agent::AgentApp;
use rust_agent_core::bots::BotRegistry;

mod openai_compat;
mod routes;
mod session;
mod sse;

use session::SessionStore;

/// 初始化日志：文件层（始终写入）+ 控制台层（AGENT_LOG 环境变量控制级别）
struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = chrono::Local::now();
        write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S"))
    }
}

fn init_logging() {
    let log_dir = dirs::home_dir()
        .map(|p| p.join(".rust-agent").join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("./logs"));

    let _ = std::fs::create_dir_all(&log_dir);

    // 文件层：始终写入 server-YYYY-MM-DD.log
    let now = chrono::Local::now();
    let log_filename = format!("server-{}.log", now.format("%Y-%m-%d"));
    let file_appender = tracing_appender::rolling::never(&log_dir, log_filename);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // guard 必须存到 static 保持 alive，否则 writer 会关闭
    Box::leak(Box::new(guard));

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // 组合：文件 + stderr
    let file_layer = tracing_subscriber::fmt::layer()
        .with_timer(LocalTimer)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(false);

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_timer(LocalTimer)
        .with_writer(std::io::stderr)
        .with_target(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stderr_layer)
        .init();
}

/// 启动 HTTP API 服务
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();
    let _ = dotenvy::dotenv();

    // 解析命令行参数
    let port: u16 = std::env::args()
        .skip_while(|arg| arg != "--port")
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let agent = Arc::new(AgentApp::from_env().await?);

    let data_dir = dirs::home_dir()
        .map(|p| p.join(".rust-agent").join("sessions"))
        .unwrap_or_else(|| std::path::PathBuf::from("./sessions"));
    let store = SessionStore::new(data_dir).await;

    let bot_registry = Arc::new(BotRegistry::load().unwrap_or_default());

    let app_state = routes::AppState {
        store,
        agent,
        bot_registry,
    };

    let app = Router::new()
        .merge(routes::routes(app_state))
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("rust-agent-server 启动在 http://localhost:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}
