//! 全局配置管理
//!
//! 从 `~/.rust-agent/config.json` 加载配置，支持多组 API 配置（profiles）。
//! 通过环境变量 `LLM_PROFILE` 选择要使用的 profile。
//! 所有配置集中在 config.json 中管理，不再依赖 .env 环境变量。

use std::collections::HashMap;
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
    /// 最大 token 数（不指定则使用全局默认值）
    pub max_tokens: Option<u32>,
    /// 该 profile 的用量配额规则（不配置则不限制）
    #[serde(default)]
    pub quotas: Vec<QuotaConfig>,
}

fn default_provider() -> String {
    "openai".to_owned()
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
    /// 全局默认最大 token 数（profile 未指定时继承）
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: u32,
    /// 额外环境变量，加载配置后注入到进程环境中
    #[serde(default)]
    pub extra_env: HashMap<String, String>,
}

fn default_max_tokens() -> u32 {
    16384
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

    /// 加载配置，如果文件不存在则报错提示用户创建
    pub fn load() -> AgentResult<Self> {
        let path = Self::file_path()?;
        if !path.exists() {
            anyhow::bail!(
                "配置文件不存在: {}\n\
                 请创建 ~/.rust-agent/config.json，参考以下模板：\n\n\
                 {{\n  \
                   \"default_profile\": \"my-api\",\n  \
                   \"default_max_tokens\": 16384,\n  \
                   \"profiles\": [\n    {{\n      \"name\": \"my-api\",\n      \
                   \"provider\": \"openai\",\n      \
                   \"api_key\": \"sk-...\",\n      \
                   \"base_url\": \"https://api.openai.com\",\n      \
                   \"model\": \"gpt-4o\",\n      \
                   \"max_tokens\": 65536,\n      \
                   \"quotas\": [\n        {{ \"window\": \"5h\", \"max_calls\": 1200 }}\n      ]\n    }}\n  \
                   ]\n\
                 }}\n",
                path.display()
            );
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
        let config: AppConfig = serde_json::from_str(&content)
            .with_context(|| format!("解析配置文件失败: {}", path.display()))?;
        config.inject_extra_env();
        Ok(config)
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
            anyhow::bail!(
                "配置文件中未找到名为 '{}' 的 profile\n可用 profile: {}",
                name,
                self.profile_names().join(", "),
            );
        }

        // 2. 配置文件 default_profile
        if !self.default_profile.is_empty()
            && let Some(profile) = self.find_profile(&self.default_profile)
        {
            return Ok(profile);
        }

        // 3. 第一个 profile
        self.profiles
            .first()
            .context("配置文件中没有任何 API profile，请检查 config.json")
    }

    /// 列出所有 profile 名称
    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).collect()
    }

    /// 获取 profile 的 max_tokens，未指定则使用全局默认值
    pub fn effective_max_tokens(&self, profile: &ApiProfile) -> u32 {
        profile.max_tokens.unwrap_or(self.default_max_tokens)
    }

    /// 将 extra_env 中的键值对注入到进程环境中
    ///
    /// # Safety
    /// `std::env::set_var` 在 Rust 1.94+ 中标记为 unsafe，
    /// 因为在多线程环境中修改环境变量可能导致未定义行为。
    /// 此方法仅在 Agent 初始化阶段（单线程上下文）调用，后续所有
    /// 子进程通过 fork 继承环境变量，不会再修改。
    fn inject_extra_env(&self) {
        for (key, value) in &self.extra_env {
            unsafe { std::env::set_var(key, value) };
        }
        if !self.extra_env.is_empty() {
            println!("[配置] 已注入 {} 个额外环境变量", self.extra_env.len());
        }
    }
}
