use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

use crate::AgentResult;
use crate::skills::SkillLoader;
use crate::todo::{TodoItemInput, TodoManager};
use crate::workspace::resolve_workspace_path;

#[derive(Clone, Debug)]
pub struct ToolDispatchResult {
    pub output: String,
    pub used_todo: bool,
}

#[derive(Clone, Debug)]
pub struct AgentToolbox {
    workspace_root: PathBuf,
    skills: Arc<SkillLoader>,
    todo: Arc<Mutex<TodoManager>>,
}

impl AgentToolbox {
    pub fn new(workspace_root: PathBuf, skills: Arc<SkillLoader>) -> Self {
        Self {
            workspace_root,
            skills,
            todo: Arc::new(Mutex::new(TodoManager::default())),
        }
    }

    pub fn tool_schemas(&self, allow_task: bool) -> Vec<Value> {
        let mut tools = vec![
            json!({
                "name": "bash",
                "description": "Run a shell command.",
                "input_schema": {
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
                    "required": ["command"]
                }
            }),
            json!({
                "name": "read_file",
                "description": "Read file contents.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "limit": { "type": "integer" }
                    },
                    "required": ["path"]
                }
            }),
            json!({
                "name": "write_file",
                "description": "Write content to file.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }
            }),
            json!({
                "name": "edit_file",
                "description": "Replace exact text in file.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" }
                    },
                    "required": ["path", "old_text", "new_text"]
                }
            }),
            json!({
                "name": "todo",
                "description": "Update task list. Track progress on multi-step tasks.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "text": { "type": "string" },
                                    "status": {
                                        "type": "string",
                                        "enum": ["pending", "in_progress", "completed"]
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
                "description": "Load specialized knowledge by name.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Skill name to load" }
                    },
                    "required": ["name"]
                }
            }),
        ];

        if allow_task {
            tools.push(json!({
                "name": "task",
                "description": "Spawn a subagent with fresh context. It shares the filesystem but not conversation history.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string" },
                        "description": { "type": "string" }
                    },
                    "required": ["prompt"]
                }
            }));
        }

        tools
    }

    pub async fn dispatch(&self, name: &str, input: &Value) -> AgentResult<ToolDispatchResult> {
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
            "todo" => {
                let items = parse_todo_items(input)?;
                let mut manager = self.todo.lock().await;
                manager.update(items)?
            }
            "load_skill" => self
                .skills
                .load_skill_content(required_string(input, "name")?),
            other => bail!("Unknown tool: {other}"),
        };

        Ok(ToolDispatchResult {
            output,
            used_todo: name == "todo",
        })
    }

    async fn run_bash(&self, command: &str) -> AgentResult<String> {
        let dangerous = ["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];
        if dangerous.iter().any(|blocked| command.contains(blocked)) {
            return Ok("Error: Dangerous command blocked".to_owned());
        }

        let mut process = if cfg!(windows) {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoLogo")
                .arg("-NonInteractive")
                .arg("-Command")
                .arg(wrap_powershell_command(command));
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-lc").arg(command);
            cmd
        };

        process.current_dir(&self.workspace_root);
        let output = timeout(Duration::from_secs(120), process.output()).await;
        let output = match output {
            Ok(result) => result.context("Failed to execute shell command")?,
            Err(_) => return Ok("Error: Timeout (120s)".to_owned()),
        };

        let mut combined = String::new();
        combined.push_str(&decode_command_output(&output.stdout));
        combined.push_str(&decode_command_output(&output.stderr));
        let trimmed = combined.trim();
        if trimmed.is_empty() {
            Ok("(no output)".to_owned())
        } else {
            Ok(trimmed.chars().take(50_000).collect())
        }
    }

    fn read_file(&self, path: &str, limit: Option<usize>) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read {}", resolved.display()))?;
        let mut lines = content.lines().map(str::to_owned).collect::<Vec<_>>();
        if let Some(limit) = limit {
            if limit < lines.len() {
                let remaining = lines.len() - limit;
                lines.truncate(limit);
                lines.push(format!("... ({remaining} more lines)"));
            }
        }
        Ok(truncate(&lines.join("\n")))
    }

    fn write_file(&self, path: &str, content: &str) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(&resolved, content)
            .with_context(|| format!("Failed to write {}", resolved.display()))?;
        Ok(format!("Wrote {} bytes", content.len()))
    }

    fn edit_file(&self, path: &str, old_text: &str, new_text: &str) -> AgentResult<String> {
        let resolved = resolve_workspace_path(&self.workspace_root, path)?;
        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("Failed to read {}", resolved.display()))?;
        if !content.contains(old_text) {
            return Ok(format!("Error: Text not found in {path}"));
        }
        let updated = content.replacen(old_text, new_text, 1);
        std::fs::write(&resolved, updated)
            .with_context(|| format!("Failed to write {}", resolved.display()))?;
        Ok(format!("Edited {path}"))
    }
}

fn required_string<'a>(input: &'a Value, key: &str) -> AgentResult<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Missing string field '{key}'"))
}

fn optional_u64(input: &Value, key: &str) -> AgentResult<Option<u64>> {
    match input.get(key) {
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| anyhow!("Field '{key}' must be an integer")),
        None => Ok(None),
    }
}

fn parse_todo_items(input: &Value) -> AgentResult<Vec<TodoItemInput>> {
    let items = input
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Missing array field 'items'"))?;

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

fn truncate(text: &str) -> String {
    text.chars().take(50_000).collect()
}

fn wrap_powershell_command(command: &str) -> String {
    format!(
        "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
$OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
chcp 65001 > $null; \
{command}"
    )
}

fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    let utf8 = String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned());
    let (gbk, _, gbk_had_errors) = encoding_rs::GBK.decode(bytes);
    let gbk = gbk.into_owned();

    if gbk_had_errors {
        return utf8;
    }

    if decoding_score(&gbk) > decoding_score(&utf8) {
        gbk
    } else {
        utf8
    }
}

fn decoding_score(text: &str) -> i32 {
    text.chars().fold(0, |score, ch| {
        score
            + match ch {
                '\u{4E00}'..='\u{9FFF}' => 3,
                '\n' | '\r' | '\t' => 1,
                ' '..='~' => 1,
                '\u{0100}'..='\u{024F}' => -2,
                '\u{FFFD}' => -5,
                _ if ch.is_control() => -3,
                _ => 0,
            }
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use encoding_rs::GBK;
    use serde_json::json;

    use super::{AgentToolbox, decode_command_output};
    use crate::skills::SkillLoader;

    #[tokio::test]
    async fn load_skill_wraps_body() {
        let skills = SkillLoader::load_from_dir(Path::new("../skills")).unwrap();
        let toolbox = AgentToolbox::new(std::env::current_dir().unwrap(), Arc::new(skills));
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
