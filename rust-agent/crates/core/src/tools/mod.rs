mod bash;
mod file_ops;
mod schemas;
mod search;
mod skill_ops;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, bail};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::AgentResult;
use crate::skills::SkillLoader;
use crate::infra::todo::{TodoItemInput, TodoManager};

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
        schemas::tool_schemas(allow_task)
    }

    /// 根据工具名称分发并执行对应的工具，返回执行结果
    pub async fn dispatch(&mut self, name: &str, input: &Value) -> AgentResult<ToolDispatchResult> {
        use crate::skills::hub as skillhub;

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
