use anyhow::{Context, anyhow};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentResult;

/// Anthropic API 的协议版本号
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic API 的 HTTP 客户端，封装了认证和请求发送逻辑
#[derive(Clone, Debug)]
pub struct AnthropicClient {
    /// reqwest HTTP 客户端（已配置默认请求头）
    http: reqwest::Client,
    /// Anthropic API 密钥
    api_key: String,
    /// API 基础 URL（默认为 https://api.anthropic.com，可自定义用于代理）
    base_url: String,
}

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
    ///
    /// # 参数
    /// - `text`: 用户输入的文本内容
    ///
    /// # 使用场景
    /// 在 `agent.rs` 中添加用户输入和子代理任务 prompt 时使用
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Value::String(text.into()),
        }
    }

    /// 创建一条包含多个内容块的用户消息
    ///
    /// # 参数
    /// - `blocks`: 内容块数组（通常是工具执行结果的 JSON 列表）
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中，将所有工具执行结果包装为一条用户消息回传给 Claude
    pub fn user_blocks(blocks: Vec<Value>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Value::Array(blocks),
        }
    }

    /// 从 Claude API 返回的内容块列表创建一条助手消息
    ///
    /// # 参数
    /// - `blocks`: API 返回的 `ResponseContentBlock` 列表（文本块或工具调用块）
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中，将 Claude 的回复转化为消息历史中的一条记录
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

impl AnthropicClient {
    /// 从环境变量创建 Anthropic API 客户端
    ///
    /// # 读取的环境变量
    /// - `ANTHROPIC_API_KEY`: API 密钥（必需）
    /// - `ANTHROPIC_BASE_URL`: 自定义 API 地址（可选，默认 `https://api.anthropic.com`）
    ///
    /// # 使用场景
    /// 在 `AgentApp::from_env()` 初始化时调用
    ///
    /// # 运作原理
    /// 读取环境变量 → 构建带有 JSON Content-Type 和 anthropic-version 请求头的
    /// reqwest 客户端 → 返回客户端实例
    pub fn from_env() -> AgentResult<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("Missing ANTHROPIC_API_KEY in environment or .env")?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_owned());

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            http,
            api_key,
            base_url,
        })
    }

    /// 调用 Claude Messages API，发送请求并获取回复
    ///
    /// # 参数
    /// - `request`: 完整的 API 请求体（包含模型、消息、工具定义等）
    ///
    /// # 返回值
    /// API 返回的响应（内容块列表和停止原因）
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中每轮循环调用一次，是 Agent 与 Claude 通信的核心方法
    ///
    /// # 运作原理
    /// 1. 拼接 API URL（base_url + /v1/messages）
    /// 2. 发送 POST 请求，附带 x-api-key 认证头
    /// 3. 检查 HTTP 状态码，非成功则返回错误
    /// 4. 将响应体 JSON 反序列化为 `MessagesResponse`
    pub async fn create_message(
        &self,
        request: &MessagesRequest<'_>,
    ) -> AgentResult<MessagesResponse> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let response = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .json(request)
            .send()
            .await
            .context("Failed to call Anthropic Messages API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read Anthropic response body")?;

        if !status.is_success() {
            return Err(anyhow!("Anthropic API error {status}: {body}"));
        }

        serde_json::from_str(&body).context("Failed to parse Anthropic response JSON")
    }
}

impl MessagesResponse {
    /// 获取停止原因，如果没有则返回空字符串
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中判断 Claude 是要继续调用工具还是给出最终回复
    pub fn stop_reason(&self) -> &str {
        self.stop_reason.as_deref().unwrap_or("")
    }

    /// 提取回复中的所有文本内容，忽略工具调用块
    ///
    /// # 返回值
    /// 所有文本块拼接在一起的字符串
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中，当 Claude 不再调用工具时，提取最终文本回复返回给用户
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
