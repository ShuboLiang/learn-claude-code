//! 全局配置管理
//!
//! 从 `~/.rust-agent/config.json` 加载配置，支持多组 API 配置（profiles）。
//! 通过环境变量 `LLM_PROFILE` 选择要使用的 profile。

use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::AgentResult;

/// 配置文件名
const CONFIG_FILE_NAME: &str = "config.json";

/// 单个 API 配置（profile）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiProfile {
    /// profile 名称
    pub name: String,
    /// provider 类型："anthropic" 或 "openai"
    #[serde(default = "default_provider")]
    pub provider: String,
    /// API 密钥
    pub api_key: String,
    /// API 基础 URL
    pub base_url: String,
    /// 模型 ID
    pub model: String,
    /// 最大 token 数
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// 该 profile 的用量配额规则（不配置则不限制）
    #[serde(default)]
    pub quotas: Vec<QuotaConfig>,
}

fn default_provider() -> String {
    "openai".to_owned()
}

fn default_max_tokens() -> u32 {
    16384
}

/// 配额规则
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuotaConfig {
    /// 时间窗口描述（如 "5h"、"7d"、"30d"）
    pub window: String,
    /// 该窗口内的最大调用次数
    pub max_calls: u64,
}

/// 全局配置
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    /// 默认使用的 profile 名称
    #[serde(default)]
    pub default_profile: String,
    /// API 配置列表
    pub profiles: Vec<ApiProfile>,
}

impl AppConfig {
    /// 获取配置文件路径
    pub fn file_path() -> AgentResult<PathBuf> {
        let dir = dirs::home_dir()
            .context("无法获取用户主目录")?
            .join(".rust-agent");
        fs::create_dir_all(&dir)?;
        Ok(dir.join(CONFIG_FILE_NAME))
    }

    /// 加载配置，如果文件不存在则返回 None
    pub fn load() -> AgentResult<Option<Self>> {
        let path = Self::file_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
        let config: AppConfig = serde_json::from_str(&content)
            .with_context(|| format!("解析配置文件失败: {}", path.display()))?;
        Ok(Some(config))
    }

    /// 保存配置到磁盘
    pub fn save(&self) -> AgentResult<()> {
        let path = Self::file_path()?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// 根据名称查找 profile
    pub fn find_profile(&self, name: &str) -> Option<&ApiProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// 获取当前应使用的 profile
    ///
    /// 优先级：环境变量 LLM_PROFILE > 配置文件 default_profile > 第一个 profile
    pub fn current_profile(&self) -> AgentResult<&ApiProfile> {
        // 1. 环境变量 LLM_PROFILE
        if let Ok(name) = std::env::var("LLM_PROFILE") {
            if let Some(profile) = self.find_profile(&name) {
                return Ok(profile);
            }
            anyhow::bail!("配置文件中未找到名为 '{}' 的 profile", name);
        }

        // 2. 配置文件 default_profile
        if !self.default_profile.is_empty() {
            if let Some(profile) = self.find_profile(&self.default_profile) {
                return Ok(profile);
            }
        }

        // 3. 第一个 profile
        self.profiles
            .first()
            .context("配置文件中没有任何 API profile")
    }

    /// 列出所有 profile 名称
    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).collect()
    }
}
