//! 把 `McpTransport` 配置转成 rmcp 的 transport 实例并 serve

use std::collections::HashMap;

use anyhow::Context;
use rmcp::{
    RoleClient, ServiceExt,
    service::RunningService,
    transport::{
        ConfigureCommandExt, StreamableHttpClientTransport,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use tokio::process::Command;

use crate::AgentResult;
use crate::infra::config::McpTransport;

/// 根据传输配置连接 MCP server，返回已初始化的 client
pub(crate) async fn connect(transport: &McpTransport) -> AgentResult<RunningService<RoleClient, ()>> {
    match transport {
        McpTransport::Stdio { command, args, env } => {
            let cmd = Command::new(command).configure(|c| {
                c.args(args);
                for (k, v) in env {
                    c.env(k, v);
                }
            });
            let proc = rmcp::transport::TokioChildProcess::new(cmd)
                .with_context(|| format!("启动 MCP 子进程失败: {command}"))?;
            let client = ()
                .serve(proc)
                .await
                .with_context(|| format!("MCP stdio 握手失败: {command}"))?;
            Ok(client)
        }
        McpTransport::Sse { url, headers } | McpTransport::Http { url, headers } => {
            // rmcp 1.6 起 SSE 与 Streamable HTTP 共用同一 transport（spec 兼容）
            let config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                .custom_headers(parse_headers(headers)?);
            let transport = StreamableHttpClientTransport::from_config(config);
            let client = ()
                .serve(transport)
                .await
                .with_context(|| format!("MCP HTTP 握手失败: {url}"))?;
            Ok(client)
        }
    }
}

fn parse_headers(
    headers: &HashMap<String, String>,
) -> AgentResult<HashMap<http::HeaderName, http::HeaderValue>> {
    let mut out = HashMap::with_capacity(headers.len());
    for (k, v) in headers {
        let name = http::HeaderName::from_bytes(k.as_bytes())
            .with_context(|| format!("非法 header 名称: {k}"))?;
        let value =
            http::HeaderValue::from_str(v).with_context(|| format!("非法 header 值: {v}"))?;
        out.insert(name, value);
    }
    Ok(out)
}
