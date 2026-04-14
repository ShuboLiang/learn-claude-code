//! Claude API 数据类型定义
//!
//! 定义与 Claude Messages API 交互所需的所有请求/响应类型。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentResult;

/// 对话消息，对应 Claude API 中的 message 格式
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiMessage {
    /// 消息角色："user"（用户）或 "assistant"（助手）
    pub role: String,
    /// 消息内容：可以是纯文本字符串，也可以是内容块数组（如工具结果、混合内容）
    pub content: Value,
}

impl ApiMessage {
    /// 创建一条纯文本的用户消息
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Value::String(text.into()),
        }
    }

    /// 创建一条包含多个内容块的用户消息
    pub fn user_blocks(blocks: Vec<Value>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Value::Array(blocks),
        }
    }

    /// 创建一条纯文本的助手消息
    pub fn assistant_text(text: &str) -> Self {
        Self {
            role: "assistant".to_owned(),
            content: Value::String(text.to_owned()),
        }
    }

    /// 从 Claude API 返回的内容块列表创建一条助手消息
    pub fn assistant_blocks(blocks: &[ResponseContentBlock]) -> AgentResult<Self> {
        Ok(Self {
            role: "assistant".to_owned(),
            content: serde_json::to_value(blocks)?,
        })
    }
}

/// 发送给 Claude Messages API 的请求体
#[derive(Clone, Debug, Serialize)]
pub struct MessagesRequest<'a> {
    /// 模型 ID（如 "claude-sonnet-4-20250514"）
    pub model: &'a str,
    /// 系统提示词
    pub system: &'a str,
    /// 对话历史消息
    pub messages: &'a [ApiMessage],
    /// 可用工具定义列表
    pub tools: &'a [Value],
    /// 最大生成 token 数
    pub max_tokens: u32,
}

/// Claude Messages API 的响应体
#[derive(Clone, Debug, Deserialize)]
pub struct MessagesResponse {
    /// Claude 回复的内容块列表（包含文本和/或工具调用）
    pub content: Vec<ResponseContentBlock>,
    /// 停止原因："tool_use"（需要调用工具）、"end_turn"（正常结束）等
    pub stop_reason: Option<String>,
}

/// Claude API 返回的单个内容块，可以是文本或工具调用请求
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContentBlock {
    /// 普通文本内容
    Text {
        /// 文本内容
        text: String,
    },
    /// 工具调用请求：Claude 想要调用某个工具
    ToolUse {
        /// 本次工具调用的唯一标识（用于将结果回传给正确的调用）
        id: String,
        /// 要调用的工具名称
        name: String,
        /// 传给工具的参数（JSON 对象）
        input: Value,
    },
}

impl MessagesResponse {
    /// 获取停止原因，如果没有则返回空字符串
    pub fn stop_reason(&self) -> &str {
        self.stop_reason.as_deref().unwrap_or("")
    }

    /// 提取回复中的所有文本内容，忽略工具调用块
    pub fn final_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ResponseContentBlock::Text { text } => Some(text.as_str()),
                ResponseContentBlock::ToolUse { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
