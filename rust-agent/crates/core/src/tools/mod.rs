mod bash;
mod file_ops;
mod search;
mod skill_ops;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, bail};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::AgentResult;
use crate::skills::SkillLoader;
use crate::todo::{TodoItemInput, TodoManager};

/// 工具调度的执行结果
#[derive(Clone, Debug)]
pub struct ToolDispatchResult {
    /// 工具执行后的文本输出
    pub output: String,
    /// 本次调用是否是 `todo` 工具（用于 todo 提醒的计数器重置）
    pub used_todo: bool,
}

/// Agent 工具箱：管理并提供所有可用的工具（bash、文件读写、todo、技能加载等）
#[derive(Clone, Debug)]
pub struct AgentToolbox {
    /// 工作区根目录，所有文件操作的基准路径
    pub(crate) workspace_root: PathBuf,
    /// 技能加载器，用 RwLock 包装以支持安装后热更新，与 AgentApp 共享同一个 Arc
    pub(crate) skills: Arc<RwLock<SkillLoader>>,
    /// 技能加载目录列表，用于安装后重新加载
    pub(crate) skill_dirs: Vec<PathBuf>,
    /// 待办事项管理器，用 Mutex 保护以支持异步安全访问
    pub(crate) todo: Arc<Mutex<TodoManager>>,
}

impl AgentToolbox {
    /// 创建新的工具箱实例
    pub fn new(workspace_root: PathBuf, skills: Arc<RwLock<SkillLoader>>, skill_dirs: Vec<PathBuf>) -> Self {
        Self {
            workspace_root,
            skills,
            skill_dirs,
            todo: Arc::new(Mutex::new(TodoManager::default())),
        }
    }

    /// 生成所有工具的 JSON Schema 定义列表，用于传给 Claude API
    pub fn tool_schemas(&self, allow_task: bool) -> Vec<Value> {
        let mut tools = vec![
            json!({
                "name": "bash",
                "description": "执行 shell 命令。",
                "input_schema": {
                    "type": "object",
                    "properties": { "command": { "type": "string", "description": "要执行的 shell 命令" } },
                    "required": ["command"]
                }
            }),
            json!({
                "name": "read_file",
                "description": "读取文件内容。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件路径" },
                        "limit": { "type": "integer", "description": "读取行数限制" }
                    },
                    "required": ["path"]
                }
            }),
            json!({
                "name": "write_file",
                "description": "将内容写入文件。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "目标文件路径" },
                        "content": { "type": "string", "description": "要写入的内容" }
                    },
                    "required": ["path", "content"]
                }
            }),
            json!({
                "name": "edit_file",
                "description": "在文件中精确替换一段文本（首次匹配）。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件路径" },
                        "old_text": { "type": "string", "description": "要被替换的原始文本" },
                        "new_text": { "type": "string", "description": "替换后的新文本" }
                    },
                    "required": ["path", "old_text", "new_text"]
                }
            }),
            json!({
                "name": "todo",
                "description": "更新任务列表。用于规划和跟踪多步骤任务的进度。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string", "description": "任务唯一标识" },
                                    "text": { "type": "string", "description": "任务描述" },
                                    "status": {
                                        "type": "string",
                                        "enum": ["pending", "in_progress", "completed"],
                                        "description": "任务状态：待处理、进行中、已完成"
                                    }
                                },
                                "required": ["id", "text", "status"]
                            }
                        }
                    },
                    "required": ["items"]
                }
            }),
            json!({
                "name": "load_skill",
                "description": "按名称加载已安装的技能知识。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "要加载的技能名称" }
                    },
                    "required": ["name"]
                }
            }),
            json!({
                "name": "glob",
                "description": "使用 glob 模式快速搜索匹配的文件路径。支持通配符如 **/*.rs、src/**/*.ts 等。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "glob 模式，如 **/*.rs、src/**/*.ts、*.toml" },
                        "path": { "type": "string", "description": "搜索的基准目录（可选，默认为工作区根目录）" }
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "grep",
                "description": "在文件内容中搜索匹配正则表达式的行。支持多种输出模式、上下文行、大小写忽略等。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "正则表达式搜索模式" },
                        "path": { "type": "string", "description": "搜索的文件或目录路径（可选，默认为工作区根目录）" },
                        "glob": { "type": "string", "description": "用于过滤文件的 glob 模式，如 *.rs（可选）" },
                        "output_mode": {
                            "type": "string",
                            "enum": ["files_with_matches", "content", "count"],
                            "description": "输出模式：files_with_matches 只返回文件路径，content 返回匹配行及行号，count 返回每个文件的匹配数（可选，默认 files_with_matches）"
                        },
                        "-i": { "type": "boolean", "description": "是否忽略大小写（可选，默认 false）" },
                        "-C": { "type": "integer", "description": "显示匹配行前后各多少行上下文（可选）" },
                        "head_limit": { "type": "integer", "description": "限制返回的最大结果数（可选，默认 250）" }
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "search_skillhub",
                "description": "搜索 SkillHub 技能商店中的可用技能。当本地没有安装所需技能时使用。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "queries": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "搜索关键词列表。每个关键词会单独搜索后合并结果。"
                        }
                    },
                    "required": ["queries"]
                }
            }),
            json!({
                "name": "install_skill",
                "description": "从 SkillHub 安装一个技能。每次调用只安装一个技能，不要批量安装。安装后技能即可使用。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "要安装的技能名称" }
                    },
                    "required": ["name"]
                }
            }),
            json!({
                "name": "compact",
                "description": "触发手动对话压缩。当上下文过长时使用，将对话历史压缩为摘要。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "focus": { "type": "string", "description": "摘要中需要重点保留的内容" }
                    }
                }
            }),
        ];

        if allow_task {
            tools.push(json!({
                "name": "task",
                "description": "启动一个拥有独立上下文的子代理来执行子任务。子代理共享文件系统，但不共享对话历史。",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "子代理的任务描述" },
                        "description": { "type": "string", "description": "任务的简要标题" }
                    },
                    "required": ["prompt"]
                }
            }));
        }

        tools
    }

    /// 根据工具名称分发并执行对应的工具，返回执行结果
    pub async fn dispatch(&mut self, name: &str, input: &Value) -> AgentResult<ToolDispatchResult> {
        use crate::skillhub;

        let output = match name {
            "bash" => self.run_bash(required_string(input, "command")?).await?,
            "read_file" => {
                let path = required_string(input, "path")?;
                let limit = optional_u64(input, "limit")?.map(|value| value as usize);
                self.read_file(path, limit)?
            }
            "write_file" => self.write_file(
                required_string(input, "path")?,
                required_string(input, "content")?,
            )?,
            "edit_file" => self.edit_file(
                required_string(input, "path")?,
                required_string(input, "old_text")?,
                required_string(input, "new_text")?,
            )?,
            "glob" => self.glob_search(
                required_string(input, "pattern")?,
                optional_string(input, "path")?,
            )?,
            "grep" => self.grep_search(
                required_string(input, "pattern")?,
                optional_string(input, "path")?,
                optional_string(input, "glob")?,
                optional_string(input, "output_mode")?,
                input.get("-i").and_then(Value::as_bool).unwrap_or(false),
                input.get("-C").and_then(Value::as_u64).map(|v| v as usize),
                input.get("head_limit").and_then(Value::as_u64).map(|v| v as usize),
            )?,
            "todo" => {
                let items = parse_todo_items(input)?;
                let mut manager = self.todo.lock().await;
                manager.update(items)?
            }
            "load_skill" => {
                let skill_name = required_string(input, "name")?;
                let content = self.skills.read().unwrap().load_skill_content(skill_name);
                let tree = self.skills.read().unwrap().get_skill_dir(skill_name)
                    .map(|dir| skill_ops::list_skill_tree(&dir))
                    .unwrap_or_default();
                if tree.is_empty() {
                    content
                } else {
                    format!("{tree}\n\n{content}")
                }
            }
            "search_skillhub" => {
                let queries = input
                    .get("queries")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow!("缺少数组字段 'queries'"))?;
                let mut results = Vec::new();
                for q in queries {
                    if let Some(keyword) = q.as_str() {
                        match skillhub::search(keyword).await {
                            Ok(result) => results.push(format!("[{keyword}]\n{result}")),
                            Err(e) => results.push(format!("[{keyword}] 搜索失败: {e}")),
                        }
                    }
                }
                if results.is_empty() {
                    "（未提供搜索关键词）".to_owned()
                } else {
                    results.join("\n\n")
                }
            }
            "install_skill" => {
                let skill_name = required_string(input, "name")?;
                let result = skillhub::install(skill_name, &self.workspace_root).await?;
                // 安装后立即重新加载技能，无需重启
                let dirs: Vec<&std::path::Path> = self.skill_dirs.iter().map(|p| p.as_path()).collect();
                if let Ok(reloaded) = SkillLoader::reload_from_dirs(&dirs) {
                    *self.skills.write().unwrap() = reloaded;
                }
                // 直接返回技能内容，省去额外一轮 load_skill 调用
                let skill_content = self.skills.read().unwrap().load_skill_content(skill_name);
                format!("{result}\n\n{skill_content}")
            }
            other => bail!("未知工具：{other}"),
        };

        Ok(ToolDispatchResult {
            output,
            used_todo: name == "todo",
        })
    }
}

// ── JSON 参数提取辅助函数 ──

/// 从 JSON 对象中提取必需的字符串字段值
pub(crate) fn required_string<'a>(input: &'a Value, key: &str) -> AgentResult<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少字符串字段 '{key}'"))
}

/// 从 JSON 对象中提取可选的 u64 字段值
pub(crate) fn optional_u64(input: &Value, key: &str) -> AgentResult<Option<u64>> {
    match input.get(key) {
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| anyhow!("字段 '{key}' 必须是整数")),
        None => Ok(None),
    }
}

/// 从 JSON 对象中提取可选的字符串字段值
pub(crate) fn optional_string<'a>(input: &'a Value, key: &str) -> AgentResult<Option<&'a str>> {
    match input.get(key) {
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| anyhow!("字段 '{key}' 必须是字符串")),
        None => Ok(None),
    }
}

/// 从 JSON 对象中解析待办事项列表
pub(crate) fn parse_todo_items(input: &Value) -> AgentResult<Vec<TodoItemInput>> {
    let items = input
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("缺少数组字段 'items'"))?;

    items
        .iter()
        .map(|item| {
            Ok(TodoItemInput {
                id: required_string(item, "id")?.to_owned(),
                text: required_string(item, "text")?.to_owned(),
                status: required_string(item, "status")?.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, RwLock};

    use encoding_rs::GBK;
    use serde_json::json;

    use super::{AgentToolbox, bash::decode_command_output};
    use crate::skills::SkillLoader;

    /// 需要外部技能目录（../skills），在 CI 或无 fixtures 环境下跳过
    #[tokio::test]
    #[ignore]
    async fn load_skill_wraps_body() {
        let skills = SkillLoader::load_from_dir(Path::new("../skills")).unwrap();
        let mut toolbox = AgentToolbox::new(std::env::current_dir().unwrap(), Arc::new(RwLock::new(skills)), vec![]);
        let result = toolbox
            .dispatch("load_skill", &json!({ "name": "pdf" }))
            .await
            .unwrap();

        assert!(result.output.contains("<skill name=\"pdf\">"));
    }

    #[test]
    fn decode_command_output_handles_gbk_bytes() {
        let (encoded, _, _) = GBK.encode("目录");
        let decoded = decode_command_output(encoded.as_ref());

        assert_eq!(decoded, "目录");
    }

    #[test]
    fn decode_command_output_keeps_utf8_text() {
        let decoded = decode_command_output("目录".as_bytes());

        assert_eq!(decoded, "目录");
    }
}
