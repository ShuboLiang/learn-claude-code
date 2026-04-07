use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::AgentResult;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedSkillFile {
    pub metadata: SkillMetadata,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct SkillDefinition {
    pub metadata: SkillMetadata,
    pub body: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct SkillLoader {
    skills: BTreeMap<String, SkillDefinition>,
}

impl SkillLoader {
    pub fn load_from_dir(skills_dir: &Path) -> AgentResult<Self> {
        let mut skills = BTreeMap::new();
        if !skills_dir.exists() {
            return Ok(Self { skills });
        }

        for entry in WalkDir::new(skills_dir)
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

    pub fn descriptions_for_system_prompt(&self) -> String {
        if self.skills.is_empty() {
            return "(no skills available)".to_owned();
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

    pub fn load_skill_content(&self, name: &str) -> String {
        match self.skills.get(name) {
            Some(skill) => format!("<skill name=\"{name}\">\n{}\n</skill>", skill.body),
            None => format!(
                "Error: Unknown skill '{name}'. Available: {}",
                self.skills.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

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
