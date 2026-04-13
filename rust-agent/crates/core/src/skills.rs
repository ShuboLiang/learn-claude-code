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
    ///
    /// # 参数
    /// - `skills_dir`: 技能文件的根目录路径（通常是工作区下的 `skills/` 目录）
    ///
    /// # 返回值
    /// 加载完成的 `SkillLoader` 实例
    ///
    /// # 使用场景
    /// 在 `AgentApp::from_env()` 初始化时调用，从项目的 `skills/` 目录加载所有技能
    ///
    /// # 运作原理
    /// 1. 如果目录不存在，直接返回空的加载器
    /// 2. 递归遍历目录，找出所有名为 `SKILL.md` 的文件
    /// 3. 读取并解析每个文件（提取 YAML frontmatter 和正文）
    /// 4. 用文件夹名作为 fallback 名称（如果 frontmatter 没指定 name）
    /// 5. 存入 BTreeMap（按技能名排序）
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
    ///
    /// # 返回值
    /// 每行一个技能，格式为 `  - 技能名: 描述 [标签]`；如果没有技能则返回提示文本
    ///
    /// # 使用场景
    /// 在 `AgentApp::system_prompt()` 中调用，将可用技能列表展示给 Claude
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
    ///
    /// 同名技能会被另一个加载器中的技能覆盖（即后加载的优先）
    ///
    /// # 使用场景
    /// 当需要从多个目录（用户目录 + 项目目录）加载技能时，分别加载后合并
    pub fn merge(&mut self, other: SkillLoader) {
        self.skills.extend(other.skills);
    }

    /// 从多个目录依次加载技能并合并，同名技能后被加载的覆盖先加载的
    pub fn load_from_dirs(dirs: &[&Path]) -> AgentResult<Self> {
        let mut loader = Self::default();
        for dir in dirs {
            let other = Self::load_from_dir(dir)?;
            loader.merge(other);
        }
        Ok(loader)
    }

    /// 重新从原始目录加载所有技能文件
    ///
    /// # 参数
    /// - `dirs`: 要扫描的目录列表
    ///
    /// # 返回值
    /// 新加载的 `SkillLoader` 实例
    ///
    /// # 使用场景
    /// 在 `install_skill` 安装完成后调用，生成包含新技能的加载器
    pub fn reload_from_dirs(dirs: &[&Path]) -> AgentResult<Self> {
        Self::load_from_dirs(dirs)
    }

    /// 按名称加载指定技能的完整内容，用 XML 标签包裹后返回
    ///
    /// # 参数
    /// - `name`: 技能名称
    ///
    /// # 返回值
    /// 用 `<skill name="...">` 标签包裹的技能正文；如果找不到则返回错误提示
    ///
    /// # 使用场景
    /// 在 `dispatch` 处理 `load_skill` 工具时调用，将技能知识注入到对话中
    pub fn load_skill_content(&self, name: &str) -> String {
        match self.skills.get(name) {
            Some(skill) => format!("<skill name=\"{name}\">\n{}\n</skill>", skill.body),
            None => format!(
                "错误：未知技能 '{name}'。可用技能：{}",
                self.skills.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        }
    }

    /// 按名称获取技能的目录路径（即 SKILL.md 所在的父目录）
    pub fn get_skill_dir(&self, name: &str) -> Option<PathBuf> {
        self.skills.get(name).and_then(|s| s.path.parent().map(|p| p.to_path_buf()))
    }
}

/// 解析 SKILL.md 文件的原始内容，分离 YAML frontmatter 和正文
///
/// # 参数
/// - `raw`: 文件的原始文本内容
///
/// # 返回值
/// 解析后的 `ParsedSkillFile`（元数据 + 正文）
///
/// # 使用场景
/// 在 `SkillLoader::load_from_dir` 中读取每个 SKILL.md 文件后调用
///
/// # 运作原理
/// 支持三种格式：
/// 1. 以 `---\n` 开头且包含 `\n---\n` 分隔符：标准 frontmatter 格式，解析 YAML 获取元数据
/// 2. 以 `---\n` 开头但没有闭合分隔符：视为无 frontmatter，全部作为正文
/// 3. 不以 `---\n` 开头：全部作为正文，元数据为空
pub fn parse_skill_file(raw: &str) -> AgentResult<ParsedSkillFile> {
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
