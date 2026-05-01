//! LLM API 数据类型定义
//!
//! 定义与 LLM Provider 交互所需的统一请求/响应类型，以及内部消息表示。

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

/// 发送给 Claude Messages API 的请求体（仅 Anthropic provider 内部使用）
#[derive(Clone, Debug, Serialize)]
pub(crate) struct MessagesRequest<'a> {
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

/// Claude Messages API 的响应体（仅 Anthropic provider 内部使用）
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct MessagesResponse {
    /// Claude 回复的内容块列表（包含文本和/或工具调用）
    pub content: Vec<ResponseContentBlock>,
    /// 停止原因："tool_use"（需要调用工具）、"end_turn"（正常结束）等
    pub stop_reason: Option<String>,
    /// 本次 API 调用的 token 用量
    pub usage: AnthropicUsage,
}

/// Anthropic API 返回的 usage 字段
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnthropicUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

/// Claude API 返回的单个内容块，可以是文本、思考内容或工具调用请求
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContentBlock {
    /// 普通文本内容
    Text {
        /// 文本内容
        text: String,
    },
    /// 思考内容块，仅用于协议兼容，不直接展示给用户
    Thinking {
        /// 模型返回的思考内容
        thinking: String,
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

/// 发送给 LLM Provider 的统一请求
#[derive(Clone, Debug)]
pub struct ProviderRequest<'a> {
    /// 模型 ID（如 "claude-sonnet-4-20250514" 或 "gpt-4o"）
    pub model: &'a str,
    /// 系统提示词
    pub system: &'a str,
    /// 对话历史消息（内部统一格式）
    pub messages: &'a [ApiMessage],
    /// 可用工具定义列表（JSON）
    pub tools: &'a [Value],
    /// 最大生成 token 数
    pub max_tokens: u32,
}

/// 单次 LLM API 调用的 token 用量
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// 输入 token 数（含缓存命中部分）
    pub input_tokens: u64,
    /// 输出 token 数
    pub output_tokens: u64,
    /// 缓存读取的 token 数（Anthropic 的 `cache_read_input_tokens`，
    /// OpenAI Chat Completions 的 `prompt_tokens_details.cached_tokens`）
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// 缓存写入的 token 数（Anthropic 的 `cache_creation_input_tokens`，
    /// OpenAI 当前无对应字段）
    #[serde(default)]
    pub cache_creation_tokens: u64,
}

impl std::ops::AddAssign for TokenUsage {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
    }
}

impl TokenUsage {
    /// 总 token 数（输入 + 输出）
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// LLM Provider 返回的统一响应
#[derive(Clone, Debug)]
pub struct ProviderResponse {
    /// 回复的内容块列表（文本和/或工具调用）
    pub content: Vec<ResponseContentBlock>,
    /// 停止原因："end_turn"（正常结束）或 "tool_calls"（需要调用工具）
    pub stop_reason: String,
    /// 本次 API 调用的 token 用量（部分 provider 可能不返回）
    pub usage: TokenUsage,
}

impl ProviderResponse {
    /// 提取回复中的所有文本内容，忽略思考内容和工具调用块
    pub fn final_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ResponseContentBlock::Text { text } => Some(text.as_str()),
                ResponseContentBlock::Thinking { .. } => None,
                ResponseContentBlock::ToolUse { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// LLM 流式响应中的单个数据块
///
/// Provider 层把上游 SSE 解析为统一的本地方块，Agent 层据此转发事件。
#[derive(Clone, Debug)]
pub enum LlmStreamChunk {
    /// 文本增量（真正的 token delta，不是完整文本）
    TextDelta(String),

    /// 思考内容增量（Kimi 等兼容层返回的 reasoning_content）
    ThinkingDelta(String),

    /// 工具调用开始（收到 tool name 和 id）
    ToolUseStart { id: String, name: String },

    /// 工具调用参数增量（JSON 片段）
    ToolUseDelta {
        id: String,
        input_json_delta: String,
    },

    /// 工具调用结束（参数已完整）
    ToolUseEnd { id: String },

    /// 停止原因（用于 Agent 层判断是否需要调用工具）
    StopReason(String),

    /// Token 用量（通常在流末尾出现）
    Usage(TokenUsage),

    /// 流正常结束
    Done,
}
