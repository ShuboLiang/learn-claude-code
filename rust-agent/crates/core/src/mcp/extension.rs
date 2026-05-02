//! 把 `McpManager` 包装成 `ToolExtension`，可链式回退到已有 extension（如 a2a 的 WeatherToolExtension）

use std::sync::Arc;

use serde_json::Value;

use crate::AgentResult;
use crate::mcp::McpManager;
use crate::tools::extension::ToolExtension;

/// MCP 工具扩展：把 `mcp__*` 前缀的工具调用路由到对应 server，
/// 其它工具回退到 `inner` extension（如果有的话）
pub struct McpExtension {
    manager: Arc<McpManager>,
    inner: Option<Arc<dyn ToolExtension>>,
}

impl McpExtension {
    pub fn new(manager: Arc<McpManager>, inner: Option<Arc<dyn ToolExtension>>) -> Self {
        Self { manager, inner }
    }
}

#[async_trait::async_trait]
impl ToolExtension for McpExtension {
    fn schemas(&self) -> Vec<Value> {
        let mut v = self.manager.schemas();
        if let Some(inner) = &self.inner {
            v.extend(inner.schemas());
        }
        v
    }

    fn can_handle(&self, name: &str) -> bool {
        McpManager::handles(name)
            || self.inner.as_ref().is_some_and(|e| e.can_handle(name))
    }

    async fn dispatch(&self, name: &str, input: &Value) -> AgentResult<String> {
        if McpManager::handles(name) {
            self.manager.dispatch(name, input).await
        } else if let Some(inner) = &self.inner {
            inner.dispatch(name, input).await
        } else {
            anyhow::bail!("McpExtension 无法处理工具: {name}")
        }
    }
}
