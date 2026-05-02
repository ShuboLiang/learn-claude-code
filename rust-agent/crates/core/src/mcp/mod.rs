//! MCP (Model Context Protocol) 客户端集成
//!
//! 启动时连接外部 MCP server，把 server 暴露的工具自动注册到 LLM function calling 列表。
//! 工具名以 `mcp__{server_name}__{tool_name}` 形式暴露给 LLM，避免与内置工具冲突。

mod extension;
mod transport;

use std::sync::Arc;

use rmcp::RoleClient;
use rmcp::model::{CallToolRequestParams, JsonObject, Tool};
use rmcp::service::RunningService;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::AgentResult;
pub use crate::infra::config::{McpServerConfig, McpTransport};
pub use extension::McpExtension;

/// 已连接的单个 MCP server
pub struct McpServer {
    pub name: String,
    pub client: RunningService<RoleClient, ()>,
    pub tools: Vec<Tool>,
}

/// MCP 连接管理器：负责连接所有配置的 server，并按前缀路由工具调用
pub struct McpManager {
    servers: Vec<McpServer>,
}

impl McpManager {
    /// 并行连接所有启用的 server。失败仅 warn，不阻塞 agent 启动。
    pub async fn connect_all(configs: &[McpServerConfig]) -> Self {
        let enabled: Vec<&McpServerConfig> = configs.iter().filter(|c| c.enabled).collect();
        if enabled.is_empty() {
            return Self { servers: Vec::new() };
        }

        let futures = enabled.iter().map(|cfg| async move {
            match Self::connect_one(cfg).await {
                Ok(server) => Some(server),
                Err(e) => {
                    warn!("MCP server '{}' 连接失败: {e:#}", cfg.name);
                    None
                }
            }
        });

        let results = futures::future::join_all(futures).await;
        let servers: Vec<McpServer> = results.into_iter().flatten().collect();

        info!(
            "MCP: 已连接 {}/{} 个 server，共 {} 个工具",
            servers.len(),
            enabled.len(),
            servers.iter().map(|s| s.tools.len()).sum::<usize>()
        );

        Self { servers }
    }

    async fn connect_one(cfg: &McpServerConfig) -> AgentResult<McpServer> {
        let client = transport::connect(&cfg.transport).await?;
        let tools = client.list_all_tools().await?;
        Ok(McpServer {
            name: cfg.name.clone(),
            client,
            tools,
        })
    }

    /// 把所有 server 的工具拍平为 Anthropic 风格 schema（name 加前缀）
    pub fn schemas(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for server in &self.servers {
            for tool in &server.tools {
                out.push(tool_to_schema(&server.name, tool));
            }
        }
        out
    }

    /// 工具名是否属于本 manager（mcp__ 前缀）
    pub fn handles(name: &str) -> bool {
        name.starts_with("mcp__")
    }

    /// 拆解 mcp__server__tool → (server_name, tool_name)
    pub(crate) fn split_prefixed_name(name: &str) -> Option<(&str, &str)> {
        let rest = name.strip_prefix("mcp__")?;
        let sep = rest.find("__")?;
        Some((&rest[..sep], &rest[sep + 2..]))
    }

    /// 路由调用：解析前缀，找到 server，调用 call_tool，扁平化结果为 String
    pub async fn dispatch(&self, prefixed_name: &str, input: &Value) -> AgentResult<String> {
        let (server_name, tool_name) = Self::split_prefixed_name(prefixed_name)
            .ok_or_else(|| anyhow::anyhow!("不是 MCP 工具名: {prefixed_name}"))?;

        let server = self
            .servers
            .iter()
            .find(|s| s.name == server_name)
            .ok_or_else(|| anyhow::anyhow!("未找到 MCP server: {server_name}"))?;

        let arguments: Option<JsonObject> = match input {
            Value::Object(map) => Some(map.clone()),
            Value::Null => None,
            _ => return Err(anyhow::anyhow!("MCP 工具参数必须是对象，得到: {input}")),
        };

        let mut params = CallToolRequestParams::new(tool_name.to_owned());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }

        let result = server.client.call_tool(params).await?;
        let mut buf = flatten_content(&result.content);

        if result.is_error.unwrap_or(false) {
            anyhow::bail!("MCP 工具 {prefixed_name} 报错: {}", buf);
        }

        // 如果有 structured_content，附加显示
        if let Some(structured) = &result.structured_content {
            if !buf.is_empty() {
                buf.push_str("\n\n");
            }
            buf.push_str("structured: ");
            buf.push_str(&structured.to_string());
        }

        Ok(buf)
    }

    /// 用 Arc 包装为可注入 toolbox 的 ToolExtension
    pub fn into_extension(self, inner: Option<Arc<dyn crate::tools::extension::ToolExtension>>) -> Arc<McpExtension> {
        Arc::new(McpExtension::new(Arc::new(self), inner))
    }
}

/// 把单个 MCP Tool 转换为 Anthropic 风格 schema（name 加 server 前缀）
fn tool_to_schema(server_name: &str, tool: &Tool) -> Value {
    let desc_prefix = tool.description.as_deref().unwrap_or("");
    let suffix = format!(" (via MCP server: {server_name})");
    json!({
        "name": format!("mcp__{server_name}__{}", tool.name),
        "description": format!("{desc_prefix}{suffix}"),
        "input_schema": Value::Object((*tool.input_schema).clone()),
    })
}

/// 把 Vec<Content> 扁平化为单个 String，供 ToolDispatchResult.output 使用
fn flatten_content(content: &[rmcp::model::Content]) -> String {
    use rmcp::model::RawContent;
    let mut buf = String::new();
    for item in content {
        match &item.raw {
            RawContent::Text(text) => buf.push_str(&text.text),
            RawContent::Image(img) => {
                buf.push_str(&format!("[image: {}]", img.mime_type));
            }
            RawContent::Audio(audio) => {
                buf.push_str(&format!("[audio: {}]", audio.mime_type));
            }
            RawContent::Resource(resource) => {
                use rmcp::model::ResourceContents;
                let uri = match &resource.resource {
                    ResourceContents::TextResourceContents { uri, .. } => uri.as_str(),
                    ResourceContents::BlobResourceContents { uri, .. } => uri.as_str(),
                };
                buf.push_str(&format!("[resource: {uri}]"));
            }
            _ => {
                buf.push_str("[unknown content]");
            }
        }
        buf.push('\n');
    }
    // 去除末尾多余换行
    while buf.ends_with('\n') {
        buf.pop();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_prefixed_name_works() {
        assert_eq!(
            McpManager::split_prefixed_name("mcp__fs__read_file"),
            Some(("fs", "read_file"))
        );
        assert_eq!(
            McpManager::split_prefixed_name("mcp__server__tool__with__underscores"),
            Some(("server", "tool__with__underscores"))
        );
        assert_eq!(McpManager::split_prefixed_name("read_file"), None);
        assert_eq!(McpManager::split_prefixed_name("mcp__only"), None);
    }

    #[test]
    fn handles_recognizes_prefix() {
        assert!(McpManager::handles("mcp__fs__read"));
        assert!(!McpManager::handles("bash"));
        assert!(!McpManager::handles("mcp_fs_read")); // 单下划线，不算
    }

    #[test]
    fn schema_format_matches_anthropic() {
        use serde_json::Map;
        use std::sync::Arc;

        let mut input_schema = Map::new();
        input_schema.insert("type".into(), json!("object"));
        let tool = Tool::new("echo", "echo the input", Arc::new(input_schema));

        let schema = tool_to_schema("test", &tool);
        assert_eq!(schema["name"], "mcp__test__echo");
        assert_eq!(
            schema["description"],
            "echo the input (via MCP server: test)"
        );
        assert_eq!(schema["input_schema"]["type"], "object");
    }

    #[test]
    fn flatten_content_handles_text() {
        use rmcp::model::{Annotated, RawContent, RawTextContent};
        let content = vec![
            Annotated {
                raw: RawContent::Text(RawTextContent {
                    text: "hello".to_owned(),
                    meta: None,
                }),
                annotations: None,
            },
            Annotated {
                raw: RawContent::Text(RawTextContent {
                    text: "world".to_owned(),
                    meta: None,
                }),
                annotations: None,
            },
        ];
        assert_eq!(flatten_content(&content), "hello\nworld");
    }
}
