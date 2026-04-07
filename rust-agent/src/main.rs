#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rust_agent::run_repl().await
}
