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
    workspace_root: PathBuf,
    /// 技能加载器的共享引用
    skills: Arc<SkillLoader>,
    /// 待办事项管理器，用 Mutex 保护以支持异步安全访问
    todo: Arc<Mutex<TodoManager>>,
}

impl AgentToolbox {
    /// 创建新的工具箱实例
    ///
    /// # 参数
    /// - `workspace_root`: 工作区根目录路径
    /// - `skills`: 已加载的技能加载器
    ///
    /// # 使用场景
    /// 在 `agent.rs` 的 `run_agent_loop` 中，每轮循环开始时创建一个新的工具箱
    pub fn new(workspace_root: PathBuf, skills: Arc<SkillLoader>) -> Self {
        Self {
            workspace_root,
            skills,
            todo: Arc::new(Mutex::new(TodoManager::default())),
        }
    }

    /// 生成所有工具的 JSON Schema 定义列表，用于传给 Claude API
    ///
    /// # 参数
    /// - `allow_task`: 是否包含 `task`（子代理）工具。子代理不允许使用 task
    ///
    /// # 返回值
    /// 工具定义的 JSON 数组，每个元素描述工具名称、功能和输入参数格式
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中每轮调用 API 前生成，作为 `MessagesRequest.tools` 传入，
    /// 告诉 Claude 当前可以使用哪些工具
    ///
    /// # 运作原理
    /// 构建一组固定工具定义（bash、read_file、write_file、edit_file、todo、load_skill），
    /// 如果 `allow_task` 为 true 则额外追加 task 工具定义
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

    /// 根据工具名称分发并执行对应的工具，返回执行结果
    ///
    /// # 参数
    /// - `name`: 工具名称（如 "bash"、"read_file"、"write_file" 等）
    /// - `input`: Claude 传入的工具参数（JSON 对象）
    ///
    /// # 返回值
    /// 包含输出文本和是否使用了 todo 的结果
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中，遍历 Claude 回复的 ToolUse 块时调用，
    /// 根据工具名路由到对应的处理函数
    ///
    /// # 运作原理
    /// 用 match 匹配工具名，从 input JSON 中提取所需参数，调用对应的私有方法执行
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

    /// 执行 shell 命令并返回输出
    ///
    /// # 参数
    /// - `command`: 要执行的 shell 命令字符串
    ///
    /// # 返回值
    /// 命令的标准输出和标准错误的合并文本（截断到 50000 字符）
    ///
    /// # 使用场景
    /// 当 Claude 调用 `bash` 工具时通过 `dispatch` 路由到此方法
    ///
    /// # 运作原理
    /// 1. 检查命令是否包含危险关键词（如 `rm -rf /`、`sudo` 等），有则直接拦截
    /// 2. 根据操作系统选择 shell：Windows 用 PowerShell，其他用 `sh -lc`
    /// 3. 在 Windows 上额外设置 UTF-8 编码环境
    /// 4. 设置工作目录为工作区根目录
    /// 5. 执行命令，超时限制 120 秒
    /// 6. 合并 stdout 和 stderr，尝试 UTF-8 和 GBK 解码，取更合理的那个
    /// 7. 截断到 50000 字符后返回
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

    /// 读取指定文件的内容
    ///
    /// # 参数
    /// - `path`: 文件路径（相对或绝对），会通过 `resolve_workspace_path` 安全校验
    /// - `limit`: 可选的行数限制，超出部分会被截断并显示剩余行数
    ///
    /// # 返回值
    /// 文件文本内容（截断到 50000 字符）
    ///
    /// # 使用场景
    /// 当 Claude 调用 `read_file` 工具时通过 `dispatch` 路由到此方法
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

    /// 将内容写入指定文件
    ///
    /// # 参数
    /// - `path`: 目标文件路径（相对或绝对），会通过 `resolve_workspace_path` 安全校验
    /// - `content`: 要写入的文本内容
    ///
    /// # 返回值
    /// 写入成功的确认信息（包含写入字节数）
    ///
    /// # 使用场景
    /// 当 Claude 调用 `write_file` 工具时通过 `dispatch` 路由到此方法
    ///
    /// # 运作原理
    /// 如果目标文件的父目录不存在会自动创建
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

    /// 在文件中精确替换一段文本（首次出现的位置）
    ///
    /// # 参数
    /// - `path`: 文件路径（相对或绝对），会通过 `resolve_workspace_path` 安全校验
    /// - `old_text`: 要被替换的原始文本（必须在文件中精确匹配）
    /// - `new_text`: 替换后的新文本
    ///
    /// # 返回值
    /// 编辑成功的确认信息，或找不到原始文本时的错误信息
    ///
    /// # 使用场景
    /// 当 Claude 调用 `edit_file` 工具时通过 `dispatch` 路由到此方法
    ///
    /// # 运作原理
    /// 读取文件内容 → 检查 `old_text` 是否存在 → 用 `replacen` 替换第一次出现 → 写回文件
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

/// 从 JSON 对象中提取必需的字符串字段值
///
/// # 参数
/// - `input`: JSON 对象
/// - `key`: 要提取的字段名
///
/// # 返回值
/// 字段对应的字符串值引用，字段不存在或不是字符串则返回错误
///
/// # 使用场景
/// 在 `dispatch` 方法中从 Claude 传入的工具参数 JSON 中提取必需的参数值
fn required_string<'a>(input: &'a Value, key: &str) -> AgentResult<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Missing string field '{key}'"))
}

/// 从 JSON 对象中提取可选的 u64 字段值
///
/// # 参数
/// - `input`: JSON 对象
/// - `key`: 要提取的字段名
///
/// # 返回值
/// `Some(值)` 如果字段存在且为整数，`None` 如果字段不存在，类型不匹配则返回错误
///
/// # 使用场景
/// 在 `dispatch` 方法中提取可选参数（如 `read_file` 的 `limit`）
fn optional_u64(input: &Value, key: &str) -> AgentResult<Option<u64>> {
    match input.get(key) {
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| anyhow!("Field '{key}' must be an integer")),
        None => Ok(None),
    }
}

/// 从 JSON 对象中解析待办事项列表
///
/// # 参数
/// - `input`: 包含 `items` 数组的 JSON 对象
///
/// # 返回值
/// 解析后的 `TodoItemInput` 向量
///
/// # 使用场景
/// 在 `dispatch` 处理 `todo` 工具时调用，从 Claude 传入的参数中提取待办列表
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

/// 将文本截断到 50000 字符
///
/// # 参数
/// - `text`: 待截断的文本
///
/// # 返回值
/// 截断后的文本（不超过 50000 字符）
///
/// # 使用场景
/// 在 `read_file` 中使用，防止过大的文件内容撑爆 API 的上下文窗口
fn truncate(text: &str) -> String {
    text.chars().take(50_000).collect()
}

/// 为 PowerShell 命令包装 UTF-8 编码环境设置
///
/// # 参数
/// - `command`: 原始要执行的命令
///
/// # 返回值
/// 前置了编码设置语句的完整 PowerShell 命令
///
/// # 使用场景
/// 在 `run_bash` 中，Windows 环境下调用 PowerShell 前使用，
/// 确保命令输出能正确以 UTF-8 编码被 Rust 程序读取
///
/// # 运作原理
/// 在用户命令前注入四行 PowerShell 语句：设置输入/输出/全局编码为 UTF-8，
/// 并执行 `chcp 65001` 将控制台代码页切换到 UTF-8
fn wrap_powershell_command(command: &str) -> String {
    format!(
        "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
$OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
chcp 65001 > $null; \
{command}"
    )
}

/// 智能解码命令输出的字节数据，自动在 UTF-8 和 GBK 之间选择最佳解码结果
///
/// # 参数
/// - `bytes`: 命令输出的原始字节数据
///
/// # 返回值
/// 解码后的字符串
///
/// # 使用场景
/// 在 `run_bash` 中解码 stdout 和 stderr 的原始字节输出，
/// 主要解决 Windows 中文环境下部分命令输出 GBK 编码的问题
///
/// # 运作原理
/// 1. 先尝试 UTF-8 解码
/// 2. 再尝试 GBK 解码
/// 3. 用 `decoding_score` 给两种结果打分（中文字符+3，常见 ASCII+1，乱码字符扣分）
/// 4. 返回得分更高的那个结果
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

/// 给解码后的文本打分，用于判断哪种编码的解码结果更合理
///
/// # 参数
/// - `text`: 待评分的文本
///
/// # 返回值
/// 整数分数，越高表示解码越可能正确
///
/// # 使用场景
/// 被 `decode_command_output` 调用，比较 UTF-8 和 GBK 解码结果的质量
///
/// # 运作原理
/// 逐字符评分：
/// - 中文字符（U+4E00 ~ U+9FFF）：+3（说明解码出了正确的中文）
/// - 换行/制表符：+1
/// - 常见 ASCII 可见字符：+1
/// - 拉丁扩展字符（U+0100 ~ U+024F）：-2（可能是误解码）
/// - 替换字符 U+FFFD：-5（乱码标志）
/// - 其他控制字符：-3
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
