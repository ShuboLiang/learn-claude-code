//! 外部工具扩展接口
//!
//! core 只定义通用 trait，具体业务工具由外部 crate 实现并注入。

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
