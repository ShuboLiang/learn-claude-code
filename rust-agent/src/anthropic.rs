use anyhow::{Context, anyhow};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::AgentResult;

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Clone, Debug)]
pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: Value,
}

impl ApiMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Value::String(text.into()),
        }
    }

    pub fn user_blocks(blocks: Vec<Value>) -> Self {
        Self {
            role: "user".to_owned(),
            content: Value::Array(blocks),
        }
    }

    pub fn assistant_blocks(blocks: &[ResponseContentBlock]) -> AgentResult<Self> {
        Ok(Self {
            role: "assistant".to_owned(),
            content: serde_json::to_value(blocks)?,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MessagesRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub messages: &'a [ApiMessage],
    pub tools: &'a [Value],
    pub max_tokens: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MessagesResponse {
    pub content: Vec<ResponseContentBlock>,
    pub stop_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

impl AnthropicClient {
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
    pub fn stop_reason(&self) -> &str {
        self.stop_reason.as_deref().unwrap_or("")
    }

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
