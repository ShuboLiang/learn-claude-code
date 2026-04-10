use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_recursion::async_recursion;
use dotenvy::dotenv;
use serde_json::{Value, json};

use crate::AgentResult;
use crate::anthropic::{AnthropicClient, ApiMessage, MessagesRequest, ResponseContentBlock};
use crate::skills::SkillLoader;
use crate::tools::AgentToolbox;

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
    /// 技能加载器，管理所有可用的 skill 文件
    skills: Arc<SkillLoader>,
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
    /// 5. 从工作区下的 `skills/` 目录加载所有技能文件
    pub fn from_env() -> AgentResult<Self> {
        let _ = dotenv();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let client = AnthropicClient::from_env()?;
        let model = std::env::var("MODEL_ID").context("Missing MODEL_ID in environment or .env")?;
        let user_skills_dir = dirs::home_dir()
            .map(|p| p.join(".claude/skills"))
            .unwrap_or_default();
        let skills = SkillLoader::load_from_dirs(&[
            &user_skills_dir,
            &workspace_root.join("skills"),
        ])?;

        Ok(Self {
            client,
            workspace_root,
            skills: Arc::new(skills),
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
    ) -> AgentResult<String> {
        history.push(ApiMessage::user_text(user_input));
        self.run_agent_loop(history, self.system_prompt(), AgentRunConfig::parent())
            .await
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
    ) -> AgentResult<String> {
        let toolbox = AgentToolbox::new(self.workspace_root.clone(), Arc::clone(&self.skills));
        let mut rounds_since_todo = 0usize;

        for _ in 0..MAX_TOOL_ROUNDS {
            let tools = toolbox.tool_schemas(config.allow_task);
            let request = MessagesRequest {
                model: &self.model,
                system: &system_prompt,
                messages,
                tools: &tools,
                max_tokens: MAX_TOKENS,
            };
            let response = self.client.create_message(&request).await?;
            let stop_reason = response.stop_reason().to_owned();
            messages.push(ApiMessage::assistant_blocks(&response.content)?);

            if stop_reason != "tool_use" {
                return Ok(response.final_text());
            }

            let mut results = Vec::new();
            let mut used_todo = false;

            for block in &response.content {
                if let ResponseContentBlock::ToolUse { id, name, input } = block {
                    let output = if name == "task" {
                        if !config.allow_task {
                            "Error: task tool unavailable in subagent".to_owned()
                        } else {
                            let prompt = input
                                .get("prompt")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            let description = input
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or("subtask");
                            println!("> task ({description}): {}", preview(&prompt));
                            self.run_subagent(prompt).await?
                        }
                    } else {
                        match toolbox.dispatch(name, input).await {
                            Ok(dispatch) => {
                                used_todo |= dispatch.used_todo;
                                println!("> {name}:");
                                println!("{}", preview(&dispatch.output));
                                dispatch.output
                            }
                            Err(e) => {
                                let msg = format!("Error: {e}");
                                println!("> {name}: {msg}");
                                msg
                            }
                        }
                    };

                    results.push(tool_result_block(id, output));
                }
            }

            rounds_since_todo = if used_todo { 0 } else { rounds_since_todo + 1 };
            if config.use_todo_reminder && rounds_since_todo >= 3 {
                results.push(json!({
                    "type": "text",
                    "text": "<reminder>Update your todos.</reminder>"
                }));
            }

            messages.push(ApiMessage::user_blocks(results));
        }

        Ok("Stopped after hitting the tool round safety limit.".to_owned())
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
    async fn run_subagent(&self, prompt: String) -> AgentResult<String> {
        let mut messages = vec![ApiMessage::user_text(prompt)];
        self.run_agent_loop(
            &mut messages,
            self.subagent_system_prompt(),
            AgentRunConfig::child(),
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
            "Windows (PowerShell). Use PowerShell syntax for shell commands: use `Get-ChildItem` instead of `ls`, `Get-Content` instead of `cat`, `-Command` instead of `-lc`, `;` instead of `&&`"
        } else {
            "Unix (bash)"
        };
        format!(
            "You are a coding agent at {}.\nPlatform: {platform}\nUse tools to solve tasks. Prefer acting over long explanations.\nUse the todo tool to plan multi-step work. Use the task tool to delegate subtasks with fresh context. Use load_skill before unfamiliar domain work.\n\nSkills available:\n{}",
            self.workspace_root.display(),
            self.skills.descriptions_for_system_prompt()
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
        format!(
            "You are a coding subagent at {}.\nComplete the given task, use tools as needed, then return a concise summary. Do not call the task tool.",
            self.workspace_root.display()
        )
    }
}

/// 启动交互式 REPL（读取-求值-打印循环），是程序的运行入口
///
/// # 返回值
/// 正常退出返回 `Ok(())`，出错返回错误信息
///
/// # 使用场景
/// 被 `main.rs` 和 `lib.rs` 的 `run_repl()` 调用，是整个 Agent 程序的主入口
///
/// # 运作原理
/// 1. 调用 `AgentApp::from_env()` 初始化 Agent
/// 2. 进入循环：打印 `agent >>` 提示符 → 读取用户输入 → 调用 `handle_user_turn`
/// 3. 输入 `q`/`quit`/`exit` 或 EOF（Ctrl+C/Ctrl+D）时退出
pub async fn run_repl() -> AgentResult<()> {
    let app = AgentApp::from_env()?;
    let mut history = Vec::new();
    let stdin = io::stdin();

    loop {
        print!("agent >> ");
        io::stdout().flush().ok();

        let mut line = String::new();
        let read = stdin.read_line(&mut line)?;
        if read == 0 {
            break;
        }

        let query = line.trim();
        if query.is_empty() || matches!(query, "q" | "quit" | "exit") {
            break;
        }

        match app.handle_user_turn(&mut history, query).await {
            Ok(text) => {
                if !text.trim().is_empty() {
                    println!("{text}");
                }
                println!();
            }
            Err(error) => {
                eprintln!("Error: {error}");
                println!();
            }
        }
    }

    Ok(())
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
