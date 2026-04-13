use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use async_recursion::async_recursion;
use dotenvy::dotenv;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::AgentResult;
use crate::anthropic::{AnthropicClient, ApiMessage, MessagesRequest, ResponseContentBlock};
use crate::compact;
use crate::skillhub;
use crate::skills::SkillLoader;
use crate::tools::AgentToolbox;

/// Agent 执行过程中产生的事件，通过 channel 发送给消费者（CLI 终端或 HTTP SSE）
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// AI 回复的文本片段
    TextDelta(String),
    /// 即将调用工具
    ToolCall { name: String, input: serde_json::Value },
    /// 工具执行结果
    ToolResult { name: String, output: String },
    /// 本轮对话结束
    TurnEnd,
    /// Agent 完成全部任务
    Done,
}

/// 每次调用 API 时请求的最大 token 数量
const MAX_TOKENS: u32 = 8_000;

/// Agent 工具调用轮数的安全上限，防止无限循环
const MAX_TOOL_ROUNDS: usize = 30;

/// Agent 应用的主结构体，持有运行所需的全部核心资源
#[derive(Clone, Debug)]
pub struct AgentApp {
    /// Anthropic API 客户端，负责与 Claude API 通信
    client: AnthropicClient,
    /// 工作区根目录的绝对路径，所有文件操作以此为基准
    workspace_root: PathBuf,
    /// 技能加载器，用 RwLock 包装以支持安装后热更新
    skills: Arc<RwLock<SkillLoader>>,
    /// 技能加载目录列表（用户目录 + 工作区目录），用于安装后重新加载
    skill_dirs: Vec<PathBuf>,
    /// 使用的模型 ID（如 "claude-sonnet-4-20250514"）
    model: String,
}

/// Agent 运行配置，控制本次运行允许哪些能力
#[derive(Clone, Copy, Debug)]
struct AgentRunConfig {
    /// 是否允许使用 `task` 工具（子代理派生）。父代理允许，子代理不允许（防止无限嵌套）
    allow_task: bool,
    /// 是否启用 todo 提醒（连续 3 轮没更新 todo 时自动提醒）
    use_todo_reminder: bool,
}

impl AgentRunConfig {
    /// 创建父代理（顶层 Agent）的运行配置
    ///
    /// # 返回值
    /// 允许 task 工具、启用 todo 提醒的配置
    fn parent() -> Self {
        Self {
            allow_task: true,
            use_todo_reminder: true,
        }
    }

    /// 创建子代理（subagent）的运行配置
    ///
    /// # 返回值
    /// 禁止 task 工具（子代理不能再派生子代理）、启用 todo 提醒的配置
    fn child() -> Self {
        Self {
            allow_task: false,
            use_todo_reminder: true,
        }
    }
}

impl AgentApp {
    /// 从环境变量和 .env 文件中初始化 Agent 应用
    ///
    /// # 读取的环境变量
    /// - `ANTHROPIC_API_KEY`: API 密钥（通过 `AnthropicClient::from_env()` 读取）
    /// - `ANTHROPIC_BASE_URL`: API 基础 URL（可选，通过 `AnthropicClient::from_env()` 读取）
    /// - `MODEL_ID`: 要使用的模型 ID
    ///
    /// # 返回值
    /// 初始化完成的 `AgentApp` 实例
    ///
    /// # 使用场景
    /// 在 `run_repl()` 启动时调用一次，是整个 Agent 的入口初始化
    ///
    /// # 运作原理
    /// 1. 加载 `.env` 文件中的环境变量
    /// 2. 获取当前工作目录作为工作区根路径
    /// 3. 初始化 Anthropic API 客户端
    /// 4. 读取 `MODEL_ID` 环境变量
    /// 5. 从用户目录的 `.skills` 文件夹和工作区下的 `skills/` 目录加载所有技能文件
    pub async fn from_env() -> AgentResult<Self> {
        let _ = dotenv();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let client = AnthropicClient::from_env()?;
        let model = std::env::var("MODEL_ID").context("Missing MODEL_ID in environment or .env")?;

        // 检查并安装 SkillHub CLI
        let skillhub_available = skillhub::ensure_cli_installed().await;
        if skillhub_available {
            println!("SkillHub CLI 已就绪。");
        }

        let user_skills_dir = dirs::home_dir()
            .map(|p| p.join(".rust-agent").join("skills"))
            .unwrap_or_default();
        let skill_dirs = vec![user_skills_dir.clone(), workspace_root.join("skills")];
        let skills = SkillLoader::load_from_dirs(
            &skill_dirs.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        )?;

        Ok(Self {
            client,
            workspace_root,
            skills: Arc::new(RwLock::new(skills)),
            skill_dirs,
            model,
        })
    }

    /// 处理用户的一次对话输入，返回 Agent 的最终回复文本
    ///
    /// # 参数
    /// - `history`: 对话历史消息列表，函数会将新的用户消息和 Agent 回复追加进去
    /// - `user_input`: 用户输入的文本内容
    ///
    /// # 返回值
    /// Agent 最终输出的文本（可能是直接回答，也可能是多轮工具调用后的结果）
    ///
    /// # 使用场景
    /// 在 `run_repl()` 的主循环中，每读取一行用户输入就调用一次
    ///
    /// # 运作原理
    /// 将用户消息包装成 `ApiMessage` 追加到历史中，然后启动 Agent 循环
    pub async fn handle_user_turn(
        &self,
        history: &mut Vec<ApiMessage>,
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> AgentResult<String> {
        let mut logger = ConversationLogger::create();

        history.push(ApiMessage::user_text(user_input));
        logger.log(&format!("=== 用户 ===\n{user_input}"));

        let system_prompt = self.system_prompt();
        logger.log(&format!("=== 系统提示词 ===\n{system_prompt}"));

        let result = self
            .run_agent_loop(
                history,
                system_prompt,
                AgentRunConfig::parent(),
                &mut logger,
                &event_tx,
            )
            .await;

        match &result {
            Ok(text) => logger.log(&format!("=== 助手 ===\n{text}")),
            Err(e) => logger.log(&format!("=== 错误 ===\n{e}")),
        }

        result
    }

    /// Agent 的核心循环：反复调用 Claude API → 执行工具 → 回传结果，直到得到最终文本回复
    ///
    /// # 参数
    /// - `messages`: 对话消息列表（包含系统提示、用户消息、助手回复、工具结果等）
    /// - `system_prompt`: 系统提示词，告诉 Claude 它的身份和可用技能
    /// - `config`: 运行配置，控制是否允许 task 工具和 todo 提醒
    ///
    /// # 返回值
    /// Agent 最终的文本回复
    ///
    /// # 使用场景
    /// 被 `handle_user_turn` 和 `run_subagent` 调用，是 Agent 的核心执行引擎
    ///
    /// # 运作原理
    /// 最多循环 `MAX_TOOL_ROUNDS`（30）轮：
    /// 1. 构建请求（包含模型、系统提示、历史消息、工具定义）
    /// 2. 调用 Claude API 获取回复
    /// 3. 如果回复的 stop_reason 不是 "tool_use"，说明 Claude 给出了最终文本，直接返回
    /// 4. 如果是 "tool_use"，说明 Claude 想调用工具，遍历所有 ToolUse 块执行对应工具
    /// 5. 特殊处理 `task` 工具：调用 `run_subagent` 启动子代理
    /// 6. 将工具执行结果回传给 Claude，继续下一轮
    /// 7. 如果连续 3 轮没有更新 todo，自动插入提醒
    #[async_recursion]
    async fn run_agent_loop(
        &self,
        messages: &mut Vec<ApiMessage>,
        system_prompt: String,
        config: AgentRunConfig,
        logger: &mut ConversationLogger,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) -> AgentResult<String> {
        let mut toolbox = AgentToolbox::new(
            self.workspace_root.clone(),
            Arc::clone(&self.skills),
            self.skill_dirs.clone(),
        );
        let mut rounds_since_todo = 0usize;

        for _ in 0..MAX_TOOL_ROUNDS {
            // 第一层 + 第二层：每次 API 调用前执行压缩
            compact::micro_compact(messages);
            if compact::estimate_tokens(messages) > compact::TOKEN_THRESHOLD {
                println!("[auto_compact 已触发]");
                *messages = compact::auto_compact(
                    &self.client,
                    &self.model,
                    &self.workspace_root,
                    &*messages,
                )
                .await?;
            }

            let tools = toolbox.tool_schemas(config.allow_task);
            let request = MessagesRequest {
                model: &self.model,
                system: &system_prompt,
                messages,
                tools: &tools,
                max_tokens: MAX_TOKENS,
            };
            let response = match self.client.create_message(&request).await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("[Agent] create_message 失败！错误: {e}");
                    return Err(e);
                }
            };
            let stop_reason = response.stop_reason().to_owned();
            messages.push(ApiMessage::assistant_blocks(&response.content)?);

            if stop_reason != "tool_use" {
                let _ = event_tx.send(AgentEvent::TurnEnd).await;
                return Ok(response.final_text());
            }

            let mut results = Vec::new();
            let mut used_todo = false;
            let mut manual_compact = false;

            for block in &response.content {
                if let ResponseContentBlock::ToolUse { id, name, input } = block {
                    // 记录工具调用日志
                    let input_preview = preview(&input.to_string());
                    logger.log(&format!("=== 工具调用: {name} ===\n输入: {input_preview}"));

                    let output = if name == "task" {
                        if !config.allow_task {
                            "错误：task 工具在子代理中不可用".to_owned()
                        } else {
                            let prompt = input
                                .get("prompt")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            let _description = input
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or("subtask");
                            let _ = event_tx.send(AgentEvent::ToolCall { name: "task".to_owned(), input: input.clone() }).await;
                            self.run_subagent(prompt, logger, event_tx).await?
                        }
                    } else if name == "compact" {
                        manual_compact = true;
                        let _ = event_tx.send(AgentEvent::ToolCall { name: "compact".to_owned(), input: input.clone() }).await;
                        "正在压缩...".to_owned()
                    } else {
                        match toolbox.dispatch(name, input).await {
                            Ok(dispatch) => {
                                used_todo |= dispatch.used_todo;
                                let _ = event_tx.send(AgentEvent::ToolCall { name: name.clone(), input: input.clone() }).await;
                                let _ = event_tx.send(AgentEvent::ToolResult { name: name.clone(), output: preview(&dispatch.output) }).await;
                                dispatch.output
                            }
                            Err(e) => {
                                let msg = format!("Error: {e}");
                                let _ = event_tx.send(AgentEvent::ToolResult { name: name.clone(), output: msg.clone() }).await;
                                msg
                            }
                        }
                    };

                    // 记录工具结果日志
                    logger.log(&format!("=== 工具结果: {name} ===\n{output}"));

                    results.push(tool_result_block(id, output));
                }
            }

            rounds_since_todo = if used_todo { 0 } else { rounds_since_todo + 1 };
            if config.use_todo_reminder && rounds_since_todo >= 3 {
                results.push(json!({
                    "type": "text",
                    "text": "<reminder>请更新你的待办事项。</reminder>"
                }));
            }

            messages.push(ApiMessage::user_blocks(results));

            // 第三层：手动压缩（AI 主动调用 compact 工具）
            if manual_compact {
                println!("[手动压缩]");
                *messages = compact::auto_compact(
                    &self.client,
                    &self.model,
                    &self.workspace_root,
                    &*messages,
                )
                .await?;
                let _ = event_tx.send(AgentEvent::TurnEnd).await;
                return Ok("对话已手动压缩。".to_owned());
            }
        }

        let _ = event_tx.send(AgentEvent::TurnEnd).await;
        Ok("已达到工具调用轮数安全上限，自动停止。".to_owned())
    }

    /// 启动一个子代理来执行独立的子任务
    ///
    /// # 参数
    /// - `prompt`: 子代理的任务描述
    ///
    /// # 返回值
    /// 子代理完成后的最终文本回复
    ///
    /// # 使用场景
    /// 在 `run_agent_loop` 中，当 Claude 调用 `task` 工具时触发。
    /// 子代理拥有独立的对话上下文（不共享父代理的历史），但共享同一个工作区
    ///
    /// # 运作原理
    /// 创建一个新的消息列表（只包含任务 prompt），以子代理系统提示和 `child` 配置
    /// 调用 `run_agent_loop`，子代理不能再用 `task` 工具（防止递归嵌套）
    async fn run_subagent(&self, prompt: String, logger: &mut ConversationLogger, event_tx: &mpsc::Sender<AgentEvent>) -> AgentResult<String> {
        let system_prompt = self.subagent_system_prompt();
        logger.log(&format!("=== 子代理系统提示词 ===\n{system_prompt}"));
        let mut messages = vec![ApiMessage::user_text(prompt)];
        self.run_agent_loop(
            &mut messages,
            system_prompt,
            AgentRunConfig::child(),
            logger,
            event_tx,
        )
        .await
    }

    /// 生成父代理（顶层 Agent）的系统提示词
    ///
    /// # 返回值
    /// 包含工作区路径、行为指引和可用技能列表的完整系统提示
    ///
    /// # 使用场景
    /// 在 `handle_user_turn` 中调用，传给 `run_agent_loop` 作为系统提示
    ///
    /// # 运作原理
    /// 拼接固定模板字符串，填入工作区路径和技能描述列表
    fn system_prompt(&self) -> String {
        let platform = if cfg!(windows) {
            "Windows (PowerShell)。使用 PowerShell 语法：用 Get-ChildItem 代替 ls，Get-Content 代替 cat，-Command 代替 -lc，; 代替 &&"
        } else {
            "Unix (bash)"
        };
        let skills_desc = self.skills.read().unwrap().descriptions_for_system_prompt();
        format!(
            "你是一个编程助手，工作目录：{}。\n平台：{platform}\n优先使用工具解决问题，避免冗长解释。\n\n\
            任务执行流程 — 每个任务必须按以下顺序执行：\n\
            0. 先了解项目：读取目录结构和关键文件，理解项目上下文。\n\
            1. 检查已安装的技能是否覆盖当前任务。如果有，调用 load_skill。\n\
            2. 如果没有匹配的已安装技能，必须调用 search_skillhub 搜索。\n\
            3. 如果 search_skillhub 返回了相关技能，调用 install_skill 安装它。\n\
            4. 只有在步骤 0-3 完成（且未找到技能）后，才能使用 bash 或其他工具执行具体操作。\n\
            5. 绝对不能跳过技能检查直接使用 bash/curl 等工具。\n\
            6. 在完成技能流程之前，绝对不能声称无法完成任务。\n\n\
            其他工具：\n\
            - 使用 todo 工具规划多步骤工作。\n\
            - 使用 task 工具委派子任务（子任务拥有独立上下文）。\n\n\
            可用技能：\n{}",
            self.workspace_root.display(),
            skills_desc
        )
    }

    /// 生成子代理的系统提示词
    ///
    /// # 返回值
    /// 简短的任务执行提示，告知子代理完成任务后返回摘要，且不能调用 task 工具
    ///
    /// # 使用场景
    /// 在 `run_subagent` 中调用，传给 `run_agent_loop` 作为系统提示
    fn subagent_system_prompt(&self) -> String {
        let skills_desc = self.skills.read().unwrap().descriptions_for_system_prompt();
        format!(
            "你是一个编程子代理，工作目录：{}。\n完成给定任务，按需使用工具，然后返回简洁的摘要。不能调用 task 工具。\n\n\
            已安装的技能：\n{skills_desc}\n\n\
            如果已安装的技能覆盖当前任务，直接调用 load_skill 加载；否则跳过技能流程，直接执行。",
            self.workspace_root.display()
        )
    }
}

/// 构造一个工具执行结果的 JSON 块，用于回传给 Claude API
///
/// # 参数
/// - `tool_use_id`: Claude 请求中工具调用的唯一标识（用于匹配请求和结果）
/// - `content`: 工具执行后返回的文本内容
///
/// # 返回值
/// 格式为 `{"type": "tool_result", "tool_use_id": "...", "content": "..."}` 的 JSON 值
///
/// # 使用场景
/// 在 `run_agent_loop` 中，每执行完一个工具调用后，用此函数将结果包装成 Claude API
/// 要求的格式，追加到消息历史中
fn tool_result_block(tool_use_id: &str, content: String) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content,
    })
}

/// 截取文本的前 200 个字符，超出部分用 "..." 省略
///
/// # 参数
/// - `text`: 待截取的文本
///
/// # 返回值
/// 如果不超过 200 字符则原样返回，否则截断并加 "..."
///
/// # 使用场景
/// 在 `run_agent_loop` 中打印工具执行结果和子代理任务描述时使用，
/// 避免在终端中打印过长的内容
fn preview(text: &str) -> String {
    const LIMIT: usize = 200;
    if text.chars().count() <= LIMIT {
        return text.to_owned();
    }
    let head = text.chars().take(LIMIT).collect::<String>();
    format!("{head}...")
}

/// 对话日志记录器，实时写入文件
///
/// 每条日志写入后立即 flush，确保即使程序崩溃也能保留已记录的内容
struct ConversationLogger {
    file: Option<std::fs::File>,
}

impl ConversationLogger {
    /// 创建新的日志记录器，在 `~/.rust-agent/logs/` 下创建以时间戳命名的日志文件
    fn create() -> Self {
        let log_dir = match dirs::home_dir() {
            Some(home) => home.join(".rust-agent").join("logs"),
            None => return Self { file: None },
        };

        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            eprintln!("创建日志目录失败: {e}");
            return Self { file: None };
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let datetime = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d_%H-%M-%S").to_string())
            .unwrap_or_else(|| format!("{}", now.as_secs()));

        let filename = log_dir.join(format!("{datetime}.log"));
        let file = std::fs::File::create(&filename)
            .map_err(|e| eprintln!("创建日志文件失败: {e}"))
            .ok();

        Self { file }
    }

    /// 写入一条日志，立即 flush 到磁盘
    fn log(&mut self, entry: &str) {
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{entry}\n---");
            let _ = file.flush();
        }
    }
}
