use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, anyhow, bail};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

use crate::AgentResult;
use crate::skillhub;
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
    /// 技能加载器，用 RwLock 包装以支持安装后热更新，与 AgentApp 共享同一个 Arc
    skills: Arc<RwLock<SkillLoader>>,
    /// 技能加载目录列表，用于安装后重新加载
    skill_dirs: Vec<PathBuf>,
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
    pub fn new(workspace_root: PathBuf, skills: Arc<RwLock<SkillLoader>>, skill_dirs: Vec<PathBuf>) -> Self {
        Self {
            workspace_root,
            skills,
            skill_dirs,
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
    pub async fn dispatch(&mut self, name: &str, input: &Value) -> AgentResult<ToolDispatchResult> {
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
                let tree = list_skill_tree(skill_name, &self.skill_dirs);
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
                let dirs: Vec<&Path> = self.skill_dirs.iter().map(|p| p.as_path()).collect();
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
            return Ok("错误：危险命令已被拦截".to_owned());
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
            Err(_) => return Ok("错误：命令执行超时（120秒）".to_owned()),
        };

        let mut combined = String::new();
        combined.push_str(&decode_command_output(&output.stdout));
        combined.push_str(&decode_command_output(&output.stderr));
        let trimmed = combined.trim();
        if trimmed.is_empty() {
            Ok("(无输出)".to_owned())
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
        Ok(format!("已写入 {} 字节", content.len()))
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
            return Ok(format!("错误：在 {path} 中未找到指定文本"));
        }
        let updated = content.replacen(old_text, new_text, 1);
        std::fs::write(&resolved, updated)
            .with_context(|| format!("Failed to write {}", resolved.display()))?;
        Ok(format!("已编辑 {path}"))
    }

    /// 使用 glob 模式搜索匹配的文件路径
    ///
    /// # 参数
    /// - `pattern`: glob 模式，如 `**/*.rs`、`src/**/*.ts`
    /// - `path`: 可选的搜索基准目录，默认为工作区根目录
    ///
    /// # 返回值
    /// 匹配的文件路径列表（相对于工作区根目录），最多 250 条
    fn glob_search(&self, pattern: &str, path: Option<&str>) -> AgentResult<String> {
        let base = match path {
            Some(p) => resolve_workspace_path(&self.workspace_root, p)?,
            None => self.workspace_root.clone(),
        };

        if !base.is_dir() {
            return Ok(format!("错误：{} 不是目录", base.display()));
        }

        let full_pattern = base.join(pattern);
        let full_pattern_str = full_pattern.to_string_lossy();

        let mut results = Vec::new();
        for entry in glob::glob(&full_pattern_str)
            .with_context(|| format!("无效的 glob 模式: {pattern}"))?
            .filter_map(Result::ok)
        {
            // 跳过不应该搜索的目录中的文件
            if should_skip_path(&entry, Self::SKIP_DIRS) {
                continue;
            }
            if !entry.is_file() {
                continue;
            }

            if let Ok(rel) = entry.strip_prefix(&self.workspace_root) {
                results.push(rel.to_string_lossy().into_owned());
            }

            // 收集够就停止，避免遍历整个目录树
            if results.len() >= 250 {
                break;
            }
        }

        if results.is_empty() {
            return Ok("（无匹配文件）".to_owned());
        }

        Ok(results.join("\n"))
    }

    /// 在文件内容中搜索匹配正则表达式的行
    ///
    /// # 参数
    /// - `pattern`: 正则表达式搜索模式
    /// - `path`: 可选的搜索路径（文件或目录），默认为工作区根目录
    /// - `glob_filter`: 可选的 glob 模式用于过滤搜索的文件
    /// - `output_mode`: 输出模式，"files_with_matches" | "content" | "count"
    /// - `case_insensitive`: 是否忽略大小写
    /// - `context_lines`: 匹配行前后显示的上下文行数
    /// - `head_limit`: 限制返回的最大结果数
    fn grep_search(
        &self,
        pattern: &str,
        path: Option<&str>,
        glob_filter: Option<&str>,
        output_mode: Option<&str>,
        case_insensitive: bool,
        context_lines: Option<usize>,
        head_limit: Option<usize>,
    ) -> AgentResult<String> {
        let mode = output_mode.unwrap_or("files_with_matches");
        let limit = head_limit.unwrap_or(250);

        let base = match path {
            Some(p) => resolve_workspace_path(&self.workspace_root, p)?,
            None => self.workspace_root.clone(),
        };

        // 构建正则表达式
        let mut regex_builder = regex::RegexBuilder::new(pattern);
        regex_builder.case_insensitive(case_insensitive);
        let re = regex_builder
            .build()
            .with_context(|| format!("无效的正则表达式: {pattern}"))?;

        // 收集要搜索的文件列表
        let files = self.collect_search_files(&base, glob_filter)?;
        if files.is_empty() {
            return Ok("（无文件可搜索）".to_owned());
        }

        // 三种模式各自就地构建输出，避免二次遍历
        let mut output_lines: Vec<String> = Vec::new();
        let mut file_count = 0;

        for file_path in &files {
            if file_count >= limit {
                break;
            }

            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lines: Vec<&str> = content.lines().collect();

            // 找到所有匹配行的索引（0-based）
            let matched_indices: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, line)| re.is_match(line))
                .map(|(i, _)| i)
                .collect();

            if matched_indices.is_empty() {
                continue;
            }

            let rel_path = file_path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(file_path)
                .to_string_lossy();

            match mode {
                "files_with_matches" => {
                    output_lines.push(rel_path.into_owned());
                    file_count += 1;
                }
                "count" => {
                    output_lines.push(format!("{}: {}", rel_path, matched_indices.len()));
                    file_count += 1;
                }
                "content" => {
                    let ctx = context_lines.unwrap_or(0);

                    if ctx > 0 {
                        // 带上下文行：收集需要显示的行号，合并相邻区间
                        let mut show_ranges: Vec<(usize, usize)> = Vec::new();
                        for &idx in &matched_indices {
                            let start = idx.saturating_sub(ctx);
                            let end = (idx + ctx + 1).min(lines.len());
                            show_ranges.push((start, end));
                        }
                        // 合并重叠区间
                        show_ranges.sort();
                        let mut merged: Vec<(usize, usize)> = Vec::new();
                        for range in show_ranges {
                            if let Some(last) = merged.last_mut() {
                                if range.0 <= last.1 {
                                    last.1 = last.1.max(range.1);
                                    continue;
                                }
                            }
                            merged.push(range);
                        }

                        output_lines.push(format!("{}:", rel_path));
                        for (start, end) in merged {
                            for i in start..end {
                                let line_num = i + 1;
                                let marker =
                                    if matched_indices.contains(&i) { ">" } else { " " };
                                output_lines.push(format!(
                                    "{marker} {line_num:4} | {}",
                                    lines[i]
                                ));
                                file_count += 1;
                                if file_count >= limit {
                                    break;
                                }
                            }
                        }
                    } else {
                        // 无上下文，只显示匹配行
                        for &idx in &matched_indices {
                            output_lines.push(format!(
                                "  {}:{}: {}",
                                rel_path,
                                idx + 1,
                                lines[idx]
                            ));
                            file_count += 1;
                            if file_count >= limit {
                                break;
                            }
                        }
                    }
                }
                _ => return Ok(format!("未知输出模式: {mode}")),
            }
        }

        if output_lines.is_empty() {
            return Ok("（无匹配结果）".to_owned());
        }

        let total = output_lines.len();
        let output = output_lines.into_iter().take(limit).collect::<Vec<_>>().join("\n");
        let suffix = if total > limit {
            format!("\n... （共 {total} 条结果，仅显示前 {limit} 条）")
        } else {
            String::new()
        };

        Ok(truncate(&format!("{output}{suffix}")))
    }

    /// 收集指定路径下需要搜索的文件列表
    ///
    /// # 参数
    /// - `base`: 搜索的基准路径（文件或目录）
    /// - `glob_filter`: 可选的 glob 过滤模式
    ///
    /// # 返回值
    /// 需要搜索的文件绝对路径列表
    /// 应该跳过的大目录列表
    const SKIP_DIRS: &[&str] = &[
        "target", "node_modules", ".git", ".svn", ".hg",
        "__pycache__", ".next", ".nuxt", "dist", "build",
        ".cache", ".venv", "venv",
    ];

    fn collect_search_files(
        &self,
        base: &Path,
        glob_filter: Option<&str>,
    ) -> AgentResult<Vec<PathBuf>> {
        if base.is_file() {
            return Ok(vec![base.to_path_buf()]);
        }

        let pattern = glob_filter.unwrap_or("**/*");
        let full_pattern = base.join(pattern);
        let full_pattern_str = full_pattern.to_string_lossy();

        let files: Vec<PathBuf> = glob::glob(&full_pattern_str)
            .with_context(|| format!("无效的 glob 模式: {pattern}"))?
            .filter_map(Result::ok)
            .filter(|p| !should_skip_path(p, Self::SKIP_DIRS))
            .filter(|p| p.is_file())
            .collect();

        Ok(files)
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
        .ok_or_else(|| anyhow!("缺少字符串字段 '{key}'"))
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
            .ok_or_else(|| anyhow!("字段 '{key}' 必须是整数")),
        None => Ok(None),
    }
}

/// 从 JSON 对象中提取可选的字符串字段值
///
/// # 参数
/// - `input`: JSON 对象
/// - `key`: 要提取的字段名
///
/// # 返回值
/// `Some(值)` 如果字段存在且为字符串，`None` 如果字段不存在
fn optional_string<'a>(input: &'a Value, key: &str) -> AgentResult<Option<&'a str>> {
    match input.get(key) {
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| anyhow!("字段 '{key}' 必须是字符串")),
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

/// 判断路径是否应该被跳过（属于无关的大目录）
///
/// # 参数
/// - `path`: 待检查的文件路径
/// - `skip_dir_names`: 需要跳过的目录名列表
///
/// # 返回值
/// 如果路径中包含任何应跳过的目录名则返回 true
fn should_skip_path(path: &Path, skip_dir_names: &[&str]) -> bool {
    path.components().any(|comp| {
        comp.as_os_str()
            .to_str()
            .is_some_and(|name| skip_dir_names.contains(&name))
    })
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
/// 1. 先尝试 UTF-8 严格解码
/// 2. 如果 UTF-8 完全成功，直接返回（不再尝试 GBK）
/// 3. 如果 UTF-8 失败（含无效字节），再尝试 GBK 解码
/// 4. 用 `decoding_score` 给两种结果打分，返回得分更高的
fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    // UTF-8 严格解码：如果完全成功，检查是否看起来像乱码
    // （GBK 编码的中文可能恰好也是合法 UTF-8，但解码为扩展拉丁字符）
    if let Ok(utf8) = String::from_utf8(bytes.to_vec()) {
        // 如果 UTF-8 解码结果中没有可疑字符，直接返回
        if !looks_like_mojibake(&utf8) {
            return utf8;
        }
        // UTF-8 虽然合法但看起来像乱码，尝试 GBK 作为候选
        let (gbk, _, gbk_had_errors) = encoding_rs::GBK.decode(bytes);
        let gbk = gbk.into_owned();
        if !gbk_had_errors && decoding_score(&gbk) > decoding_score(&utf8) {
            return gbk;
        }
        return utf8;
    }

    // UTF-8 失败，用 lossy 解码作为候选
    let utf8 = String::from_utf8_lossy(bytes).into_owned();
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

/// 判断文本是否看起来像 GBK 被误读为 UTF-8 后的乱码
///
/// 典型特征：包含扩展拉丁字符（U+0100 ~ U+024F）而没有对应的中文内容
fn looks_like_mojibake(text: &str) -> bool {
    let has_latin_ext = text.chars().any(|ch| matches!(ch, '\u{0100}'..='\u{024F}'));
    let has_cjk = text.chars().any(|ch| matches!(ch, '\u{4E00}'..='\u{9FFF}'));
    has_latin_ext && !has_cjk
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

/// 列出已安装技能目录的文件树结构
///
/// # 参数
/// - `skill_name`: 技能名称（对应目录名）
/// - `skill_dirs`: 技能搜索目录列表
///
/// # 返回值
/// 目录树文本，让 Claude 了解技能的完整文件结构并自行判断调用方式
fn list_skill_tree(skill_name: &str, skill_dirs: &[PathBuf]) -> String {
    // 在所有技能目录中查找，后加载的优先（项目目录覆盖用户目录）
    let skill_dir = skill_dirs
        .iter()
        .rev()
        .map(|d| d.join(skill_name))
        .find(|d| d.exists());

    let skill_dir = match skill_dir {
        Some(d) => d,
        None => return String::new(),
    };

    let mut lines = vec![format!("技能目录结构 ({}):", skill_dir.display())];
    for entry in walkdir::WalkDir::new(&skill_dir)
        .sort_by(|a, b| a.file_name().cmp(b.file_name()))
        .into_iter()
        .filter_map(Result::ok)
    {
        let depth = entry.depth();
        let indent = "  ".repeat(depth);
        let name = entry.file_name().to_string_lossy();
        if depth == 0 {
            lines.push(format!("{indent}{name}/"));
        } else if entry.file_type().is_dir() {
            lines.push(format!("{indent}{name}/"));
        } else {
            lines.push(format!("{indent}{name}"));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, RwLock};

    use encoding_rs::GBK;
    use serde_json::json;

    use super::{AgentToolbox, decode_command_output};
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
