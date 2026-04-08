/// 程序入口：启动异步运行时并运行 REPL 交互循环
///
/// # 运作原理
/// `#[tokio::main]` 宏将异步函数包装为同步的 `fn main()`，
/// 内部创建 Tokio 异步运行时，然后调用 `rust_agent::run_repl()` 启动 Agent
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rust_agent::run_repl().await
}
