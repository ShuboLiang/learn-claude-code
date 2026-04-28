//! Bot（子代理）定义与管理模块
//!
//! 从 `~/.rust-agent/bots/` 目录扫描并加载 BOT.md 文件。
//! 每个 Bot 有独立的身份、专属技能和自定义 system prompt，
//! 用户可通过 `/ @botname` 语法委派任务给特定 Bot。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::AgentResult;
use crate::context::ContextService;
use crate::skills::SkillLoader;

/// Bot 定义文件的元数据（从 BOT.md 的 YAML frontmatter 中解析）
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct BotMetadata {
    /// Bot 唯一名称（用于 `/ @botname` 标识）
    pub name: String,
    /// 显示昵称
    #[serde(default)]
    pub nickname: String,
    /// 角色/职位描述
    #[serde(default)]
    pub role: String,
    /// 能力描述：说明该 Bot 擅长什么、何时应该委派给它
    /// 这段描述会显示在 system prompt 中，帮助主 agent 判断路由
    #[serde(default)]
    pub description: String,
    /// 指定模型（可选，不指定则继承主 agent 的 model）
    pub model: Option<String>,
    /// 最大 token 数（可选）
    pub max_tokens: Option<u32>,
    /// 指定 API profile（可选）
    pub profile: Option<String>,
}

/// Bot 活跃会话：缓存对话上下文，支持多轮交互
///
/// 当 Bot 反问用户后，对话上下文（包含简历解析、打分等中间结果）
/// 被保存到此结构。用户回复后，Bot 从断点继续执行。
#[derive(Clone, Debug)]
pub struct BotSession {
    /// Bot 的对话上下文（包含所有中间结果和对话历史）
    pub ctx: ContextService,
    /// 会话创建时间，用于过期清理
    pub created_at: Instant,
}

impl BotSession {
    /// 会话过期时间（30 分钟）
    const TTL: Duration = Duration::from_secs(30 * 60);

    /// 是否已过期
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= Self::TTL
    }
}

/// 解析后的 BOT.md 文件内容
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedBotFile {
    /// Bot 元数据
    pub metadata: BotMetadata,
    /// 正文内容（去掉 frontmatter 后的 Markdown，作为自定义 system prompt）
    pub body: String,
}

/// 完整的 Bot 定义
#[derive(Clone, Debug)]
pub struct BotDefinition {
    /// Bot 元数据
    pub metadata: BotMetadata,
    /// BOT.md 正文作为自定义 system prompt 指令
    pub body: String,
    /// Bot 专属技能加载器
    pub skills: SkillLoader,
    /// BOT.md 在磁盘上的路径
    pub path: PathBuf,
    /// Bot 目录路径（BOT.md 所在的父目录）
    pub dir: PathBuf,
}

/// Bot 摘要信息，用于列表展示
#[derive(Clone, Debug, Serialize)]
pub struct BotSummary {
    /// Bot 名称
    pub name: String,
    /// 昵称
    pub nickname: String,
    /// 角色
    pub role: String,
    /// 能力描述
    pub description: String,
    /// 专属技能数量
    pub skill_count: usize,
    /// 指定模型（可选）
    pub model: Option<String>,
    /// 指定 profile（可选）
    pub profile: Option<String>,
}

/// Bot 注册表：加载并管理所有可用的 Bot 及其活跃会话
#[derive(Clone, Debug, Default)]
pub struct BotRegistry {
    /// Bot 名称 → Bot 定义
    bots: BTreeMap<String, BotDefinition>,
    /// Bot 名称 → 活跃会话（用 RwLock 支持内部可变性）
    sessions: Arc<RwLock<BTreeMap<String, BotSession>>>,
}

impl BotRegistry {
    /// 获取 Bot 存储的根目录
    pub fn bots_dir() -> AgentResult<PathBuf> {
        let dir = dirs::home_dir()
            .context("无法获取用户主目录")?
            .join(".rust-agent")
            .join("bots");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 从 `~/.rust-agent/bots/` 加载所有 Bot
    pub fn load() -> AgentResult<Self> {
        let bots_dir = Self::bots_dir()?;
        let mut bots = BTreeMap::new();

        if !bots_dir.exists() {
            return Ok(Self {
                bots,
                sessions: Arc::new(RwLock::new(BTreeMap::new())),
            });
        }

        // 扫描 bots/ 下所有 BOT.md 文件
        for entry in WalkDir::new(&bots_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file() && entry.file_name() == "BOT.md")
        {
            let path = entry.path().to_path_buf();
            let dir = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| bots_dir.clone());

            let raw = fs::read_to_string(&path)
                .with_context(|| format!("读取 Bot 文件失败: {}", path.display()))?;
            let parsed = parse_bot_file(&raw)?;

            // Bot 名称 fallback：frontmatter > 目录名
            let name = if !parsed.metadata.name.is_empty() {
                parsed.metadata.name.clone()
            } else {
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_owned()
            };

            // 加载 Bot 专属技能（从 bots/<name>/skills/ 目录）
            let bot_skills_dir = dir.join("skills");
            let bot_skills = SkillLoader::load_from_dir(&bot_skills_dir)?;

            bots.insert(
                name,
                BotDefinition {
                    metadata: parsed.metadata,
                    body: parsed.body,
                    skills: bot_skills,
                    path,
                    dir,
                },
            );
        }

        Ok(Self {
            bots,
            sessions: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    /// 按名称查找 Bot
    pub fn find(&self, name: &str) -> Option<&BotDefinition> {
        self.bots.get(name)
    }

    /// 列出所有已加载的 Bot 摘要
    pub fn list(&self) -> Vec<BotSummary> {
        self.bots
            .iter()
            .map(|(name, bot)| {
                BotSummary {
                    name: name.clone(),
                    nickname: bot.metadata.nickname.clone(),
                    role: bot.metadata.role.clone(),
                    description: bot.metadata.description.clone(),
                    skill_count: bot.skills.list_skills().len(),
                    model: bot.metadata.model.clone(),
                    profile: bot.metadata.profile.clone(),
                }
            })
            .collect()
    }

    /// Bot 数量
    pub fn len(&self) -> usize {
        self.bots.len()
    }

    /// 是否有 Bot
    pub fn is_empty(&self) -> bool {
        self.bots.is_empty()
    }

    /// 生成用于 system prompt 的 Bot 列表字符串
    /// 包含名称、角色、能力描述、昵称，便于 LLM 进行智能路由和编排
    pub fn descriptions_for_system_prompt(&self) -> String {
        if self.bots.is_empty() {
            return String::new();
        }
        self.bots
            .iter()
            .map(|(name, bot)| {
                let mut parts = vec![format!("**{name}**")];
                if !bot.metadata.nickname.is_empty() {
                    parts.push(format!("（{}）", bot.metadata.nickname));
                }
                if !bot.metadata.role.is_empty() {
                    parts.push(format!("— {}", bot.metadata.role));
                }
                let label = parts.join("");

                // description 优先，fallback 到 body 摘要
                let desc = if !bot.metadata.description.is_empty() {
                    bot.metadata.description.clone()
                } else {
                    bot.body
                        .chars()
                        .take(120)
                        .collect::<String>()
                        .replace('\n', " ")
                        .trim()
                        .to_owned()
                };
                format!("- {label}\n  能力：{desc}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ── 会话管理（支持 Bot 多轮交互） ──

    /// 获取 Bot 的活跃会话克隆。过期会话自动过滤。
    pub fn get_session(&self, bot_name: &str) -> Option<BotSession> {
        let sessions = self.sessions.read().unwrap();
        sessions.get(bot_name).filter(|s| !s.is_expired()).cloned()
    }

    /// 保存（或覆盖）Bot 会话
    pub fn save_session(&self, bot_name: String, ctx: ContextService) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.insert(
            bot_name,
            BotSession {
                ctx,
                created_at: Instant::now(),
            },
        );
    }

    /// 移除并销毁 Bot 会话（任务完成或出错时调用）
    pub fn clear_session(&self, bot_name: &str) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.remove(bot_name);
    }

    /// 清理所有过期会话
    pub fn cleanup_expired_sessions(&self) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.retain(|_, s| !s.is_expired());
    }
}

/// 解析 BOT.md 文件的原始内容，分离 YAML frontmatter 和正文。
///
/// BOT.md 使用与 SKILL.md 相同的 frontmatter 格式。
/// frontmatter 中的字段映射到 `BotMetadata`，正文作为 Bot 的自定义 system prompt。
pub fn parse_bot_file(raw: &str) -> AgentResult<ParsedBotFile> {
    if !raw.starts_with("---\n") {
        // 没有 frontmatter，全部内容作为正文，使用空元数据
        return Ok(ParsedBotFile {
            metadata: BotMetadata::default(),
            body: raw.trim().to_owned(),
        });
    }

    let rest = &raw[4..];
    if let Some(index) = rest.find("\n---\n") {
        let frontmatter = &rest[..index];
        let body = &rest[index + 5..];
        let metadata: BotMetadata = serde_yaml::from_str(frontmatter).unwrap_or_default();
        return Ok(ParsedBotFile {
            metadata,
            body: body.trim().to_owned(),
        });
    }

    // 有起始 `---` 但没有结束标记，整个内容作为正文
    Ok(ParsedBotFile {
        metadata: BotMetadata::default(),
        body: raw.trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bot_with_frontmatter() {
        let raw = r#"---
name: code-reviewer
nickname: 代码审查员
role: 代码质量审查
model: gpt-4o
profile: my-profile
---
# 你是代码审查专家

审查代码时请关注安全性和性能。
"#;
        let parsed = parse_bot_file(raw).unwrap();
        assert_eq!(parsed.metadata.name, "code-reviewer");
        assert_eq!(parsed.metadata.nickname, "代码审查员");
        assert_eq!(parsed.metadata.role, "代码质量审查");
        assert_eq!(parsed.metadata.model.as_deref(), Some("gpt-4o"));
        assert_eq!(parsed.metadata.profile.as_deref(), Some("my-profile"));
        assert!(parsed.body.contains("你是代码审查专家"));
    }

    #[test]
    fn test_parse_bot_without_frontmatter() {
        let raw = "# 前端架构师\n专注于 React 组件设计。";
        let parsed = parse_bot_file(raw).unwrap();
        assert_eq!(parsed.metadata.name, "");
        assert!(parsed.body.contains("前端架构师"));
    }

    #[test]
    fn test_parse_bot_minimal_frontmatter() {
        let raw = r#"---
name: minimal
---
Just a body.
"#;
        let parsed = parse_bot_file(raw).unwrap();
        assert_eq!(parsed.metadata.name, "minimal");
        assert_eq!(parsed.metadata.nickname, "");
        assert_eq!(parsed.body, "Just a body.");
    }

    #[test]
    fn test_parse_bot_unclosed_frontmatter() {
        let raw = r#"---
name: broken
Some body without closing.
"#;
        let parsed = parse_bot_file(raw).unwrap();
        // 无结束标记时，整个内容作为正文
        assert_eq!(parsed.metadata.name, "");
        assert!(parsed.body.contains("name: broken"));
    }
}
