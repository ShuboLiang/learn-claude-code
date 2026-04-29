//! 技能加载与管理模块
//!
//! 从磁盘目录扫描并加载 SKILL.md 技能文件，支持多目录合并和热更新。

pub mod hub;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::AgentResult;

/// 技能文件的元数据（从 SKILL.md 文件的 YAML frontmatter 中解析）
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkillMetadata {
    /// 技能名称（如果 frontmatter 中未指定，则使用文件夹名作为 fallback）
    pub name: Option<String>,
    /// 技能的简要描述
    pub description: Option<String>,
    /// 技能标签（用于分类）
    pub tags: Option<String>,
}

/// 解析后的技能文件内容，分离了元数据和正文
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedSkillFile {
    /// 技能的元数据（名称、描述、标签）
    pub metadata: SkillMetadata,
    /// 技能的正文内容（去掉 frontmatter 后的 Markdown）
    pub body: String,
}

/// 技能摘要信息，用于列表展示
#[derive(Clone, Debug)]
pub struct SkillSummary {
    /// 技能名称
    pub name: String,
    /// 技能描述
    pub description: String,
    /// 技能标签
    pub tags: String,
    /// 技能文件路径
    pub path: PathBuf,
}

/// 完整的技能定义，包含元数据、正文和文件路径
#[derive(Clone, Debug)]
pub struct SkillDefinition {
    /// 技能的元数据
    pub metadata: SkillMetadata,
    /// 技能的正文内容
    pub body: String,
    /// 技能文件在磁盘上的路径
    pub path: PathBuf,
}

/// 技能加载器：从磁盘目录扫描并加载所有 SKILL.md 技能文件
#[derive(Clone, Debug, Default)]
pub struct SkillLoader {
    /// 技能名称 → 技能定义的映射表（按名称排序）
    skills: BTreeMap<String, SkillDefinition>,
}

impl SkillLoader {
    /// 从指定目录递归扫描并加载所有 SKILL.md 技能文件
    pub fn load_from_dir(skills_dir: &Path) -> AgentResult<Self> {
        let mut skills = BTreeMap::new();
        if !skills_dir.exists() {
            return Ok(Self { skills });
        }

        for entry in WalkDir::new(skills_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file() && entry.file_name() == "SKILL.md")
        {
            let path = entry.path().to_path_buf();
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read skill file: {}", path.display()))?;
            let parsed = parse_skill_file(&raw)?;
            let fallback_name = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_owned();
            let name = parsed.metadata.name.clone().unwrap_or(fallback_name);
            skills.insert(
                name,
                SkillDefinition {
                    metadata: parsed.metadata,
                    body: parsed.body,
                    path,
                },
            );
        }

        Ok(Self { skills })
    }

    /// 生成所有技能的描述文本，用于注入到系统提示词中
    pub fn descriptions_for_system_prompt(&self) -> String {
        if self.skills.is_empty() {
            return "（没有可用的技能）".to_owned();
        }

        self.skills
            .iter()
            .map(|(name, skill)| {
                let description = skill
                    .metadata
                    .description
                    .clone()
                    .unwrap_or_else(|| "No description".to_owned());
                match skill.metadata.tags.clone() {
                    Some(tags) if !tags.trim().is_empty() => {
                        format!("  - {name}: {description} [{tags}]")
                    }
                    _ => format!("  - {name}: {description}"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 合并另一个 SkillLoader 的技能到当前实例中
    pub fn merge(&mut self, other: SkillLoader) {
        self.skills.extend(other.skills);
    }

    /// 从多个目录依次加载技能并合并
    pub fn load_from_dirs(dirs: &[&Path]) -> AgentResult<Self> {
        let mut loader = Self::default();
        for dir in dirs {
            let other = Self::load_from_dir(dir)?;
            loader.merge(other);
        }
        Ok(loader)
    }

    /// 重新从原始目录加载所有技能文件
    pub fn reload_from_dirs(dirs: &[&Path]) -> AgentResult<Self> {
        Self::load_from_dirs(dirs)
    }

    /// 按名称加载指定技能的完整内容，用 XML 标签包裹后返回
    pub fn load_skill_content(&self, name: &str) -> String {
        match self.skills.get(name) {
            Some(skill) => format!("<skill name=\"{name}\">\n{}\n</skill>", skill.body),
            None => format!(
                "错误：未知技能 '{name}'。可用技能：{}",
                self.skills.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        }
    }

    /// 列出所有已安装技能的摘要信息
    pub fn list_skills(&self) -> Vec<SkillSummary> {
        self.skills
            .iter()
            .map(|(name, skill)| SkillSummary {
                name: name.clone(),
                description: skill.metadata.description.clone().unwrap_or_default(),
                tags: skill.metadata.tags.clone().unwrap_or_default(),
                path: skill.path.clone(),
            })
            .collect()
    }

    /// 按名称获取技能的目录路径（即 SKILL.md 所在的父目录）
    pub fn get_skill_dir(&self, name: &str) -> Option<PathBuf> {
        self.skills
            .get(name)
            .and_then(|s| s.path.parent().map(|p| p.to_path_buf()))
    }
}

/// 解析 SKILL.md 文件的原始内容，分离 YAML frontmatter 和正文
pub fn parse_skill_file(raw: &str) -> AgentResult<ParsedSkillFile> {
    // 统一换行符，兼容 Windows CRLF
    let normalized = raw.replace("\r\n", "\n");
    let raw = normalized.as_str();

    if !raw.starts_with("---\n") {
        return Ok(ParsedSkillFile {
            metadata: SkillMetadata::default(),
            body: raw.trim().to_owned(),
        });
    }

    let rest = &raw[4..];
    if let Some(index) = rest.find("\n---\n") {
        let frontmatter = &rest[..index];
        let body = &rest[index + 5..];
        let metadata: SkillMetadata = serde_yaml::from_str(frontmatter).unwrap_or_default();
        return Ok(ParsedSkillFile {
            metadata,
            body: body.trim().to_owned(),
        });
    }

    Ok(ParsedSkillFile {
        metadata: SkillMetadata::default(),
        body: raw.trim().to_owned(),
    })
}
