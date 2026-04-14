//! LLM Provider 抽象层
//!
//! 定义统一的 LLM 接口，支持 Anthropic 和 OpenAI 两种后端。
//! 优先从 `~/.rust-agent/config.json` 的 profile 加载配置，
//! 未配置时回退到环境变量。

pub mod anthropic;
pub mod openai;
pub mod types;

use anyhow::Context;
use crate::AgentResult;
pub use types::{ApiMessage, ProviderRequest, ProviderResponse, ResponseContentBlock};

/// LLM Provider 枚举，封装不同的 LLM 后端
#[derive(Clone, Debug)]
pub enum LlmProvider {
    /// Anthropic Claude API
    Anthropic(anthropic::AnthropicClient),
    /// OpenAI 兼容 API（包括 Ollama、vLLM、DeepSeek 等）
    OpenAI(openai::OpenAIClient),
}

impl LlmProvider {
    /// 发送消息并获取回复（统一入口，自动分发到对应后端，并统计用量）
    pub async fn create_message(
        &self,
        request: &ProviderRequest<'_>,
        quotas: &[crate::infra::usage::QuotaRule],
    ) -> AgentResult<ProviderResponse> {
        let result = match self {
            LlmProvider::Anthropic(client) => client.create_message(request).await,
            LlmProvider::OpenAI(client) => client.create_message(request).await,
        };

        // 无论成功失败都统计调用次数
        if let Ok(mut tracker) = crate::infra::usage::UsageTracker::load(quotas.to_vec()) {
            let _ = tracker.record_call(&self.base_url(), &self.api_key());
        }

        result
    }

    /// 获取当前 provider 的 API 基础 URL
    pub fn base_url(&self) -> &str {
        match self {
            LlmProvider::Anthropic(client) => client.base_url(),
            LlmProvider::OpenAI(client) => client.base_url(),
        }
    }

    /// 获取当前 provider 的 API 密钥
    pub fn api_key(&self) -> &str {
        match self {
            LlmProvider::Anthropic(client) => client.api_key(),
            LlmProvider::OpenAI(client) => client.api_key(),
        }
    }
}

/// 创建 LLM Provider 的结果，包含 provider、模型 ID、配额规则
pub struct ProviderInfo {
    pub provider: LlmProvider,
    pub model: String,
    pub max_tokens: u32,
    /// 当前 profile 的配额规则
    pub quotas: Vec<crate::infra::usage::QuotaRule>,
}

/// 根据配置创建 LLM Provider
///
/// 优先级：
/// 1. `~/.rust-agent/config.json` 中的 profile（通过 `LLM_PROFILE` 环境变量选择）
/// 2. 环境变量（`LLM_PROVIDER` + `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` 等）
pub fn create_provider() -> AgentResult<ProviderInfo> {
    // 优先尝试从配置文件加载
    if let Some(config) = crate::infra::config::AppConfig::load()? {
        let profile = config.current_profile()?;

        // 从 profile 配置中构建配额规则
        let quotas: Vec<crate::infra::usage::QuotaRule> = profile
            .quotas
            .iter()
            .map(|q| crate::infra::usage::QuotaRule::from_config(&q.window, q.max_calls))
            .collect();

        if quotas.is_empty() {
            println!("[配置] 使用 profile: {} ({} / {}) - 无配额限制", profile.name, profile.provider, profile.model);
        } else {
            let quota_descs: Vec<String> = quotas.iter().map(|q| q.description()).collect();
            println!("[配置] 使用 profile: {} ({} / {}) - 配额: {}", profile.name, profile.provider, profile.model, quota_descs.join(", "));
        }

        let provider = match profile.provider.to_lowercase().as_str() {
            "openai" => {
                let client = openai::OpenAIClient::new(
                    &profile.api_key,
                    &profile.base_url,
                )?;
                LlmProvider::OpenAI(client)
            }
            _ => {
                let client = anthropic::AnthropicClient::new(
                    &profile.api_key,
                    &profile.base_url,
                )?;
                LlmProvider::Anthropic(client)
            }
        };

        return Ok(ProviderInfo {
            provider,
            model: profile.model.clone(),
            max_tokens: profile.max_tokens,
            quotas,
        });
    }

    // 回退到环境变量（无配额）
    create_provider_from_env()
}

/// 从环境变量创建 LLM Provider（回退方案）
fn create_provider_from_env() -> AgentResult<ProviderInfo> {
    let provider_type = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "anthropic".to_owned());
    let model = std::env::var("MODEL_ID").context("Missing MODEL_ID in environment or .env")?;
    let max_tokens: u32 = std::env::var("MAX_TOKENS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(16384);

    let provider = match provider_type.to_lowercase().as_str() {
        "openai" => {
            let client = openai::OpenAIClient::from_env()?;
            LlmProvider::OpenAI(client)
        }
        _ => {
            let client = anthropic::AnthropicClient::from_env()?;
            LlmProvider::Anthropic(client)
        }
    };

    Ok(ProviderInfo {
        provider,
        model,
        max_tokens,
        quotas: vec![],
    })
}
