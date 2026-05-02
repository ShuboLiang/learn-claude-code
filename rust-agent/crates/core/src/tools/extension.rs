//! 外部工具扩展接口
//!
//! core 只定义通用 trait，具体业务工具由外部 crate 实现并注入。

use std::sync::Arc;

use serde_json::Value;

use crate::AgentResult;

/// 外部工具扩展接口。
///
/// 实现此 trait 即可在不修改 core 代码的前提下，为 Agent 添加新的可执行工具。
/// 工具 schema 会自动合并到 LLM 的 function calling 列表中，
/// dispatch 会优先路由到扩展，再 fallback 到内置工具。
#[async_trait::async_trait]
pub trait ToolExtension: Send + Sync {
    /// 返回额外工具的 JSON Schema 定义列表（Anthropic function calling 格式）
    fn schemas(&self) -> Vec<Value>;

    /// 判断本扩展是否能处理指定工具名
    fn can_handle(&self, name: &str) -> bool;

    /// 执行工具，返回文本输出
    async fn dispatch(&self, name: &str, input: &Value) -> AgentResult<String>;
}

/// 把两个 ToolExtension 组合：先看 outer，未命中再 fallback 到 inner。
/// schemas() 会合并两边的工具列表（outer 在前）。
pub struct ChainedExtension {
    outer: Arc<dyn ToolExtension>,
    inner: Arc<dyn ToolExtension>,
}

impl ChainedExtension {
    pub fn new(outer: Arc<dyn ToolExtension>, inner: Arc<dyn ToolExtension>) -> Self {
        Self { outer, inner }
    }
}

#[async_trait::async_trait]
impl ToolExtension for ChainedExtension {
    fn schemas(&self) -> Vec<Value> {
        let mut v = self.outer.schemas();
        v.extend(self.inner.schemas());
        v
    }

    fn can_handle(&self, name: &str) -> bool {
        self.outer.can_handle(name) || self.inner.can_handle(name)
    }

    async fn dispatch(&self, name: &str, input: &Value) -> AgentResult<String> {
        if self.outer.can_handle(name) {
            self.outer.dispatch(name, input).await
        } else {
            self.inner.dispatch(name, input).await
        }
    }
}
