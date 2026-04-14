use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use async_recursion::async_recursion;
use dotenvy::dotenv;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::AgentResult;
use crate::api::types::{ApiMessage, ProviderRequest, ResponseContentBlock};
use crate::infra::compact;
use crate::infra::logging::ConversationLogger;
use crate::infra::storage;
use crate::infra::utils::preview_text;
use crate::skills::SkillLoader;
use crate::skills::hub as skillhub;
use crate::tools::AgentToolbox;

/// Agent 执行过程中产生的事件，通过 channel 发送给消费者（CLI 终端或 HTTP SSE）
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// AI 回复的文本片段
    TextDelta(String),
    /// 即将调用工具
    ToolCall {
        name: String,
        input: serde_json::Value,
    },
    /// 工具执行结果
    ToolResult { name: String, output: String },
    /// 本轮对话结束
    TurnEnd,
    /// Agent 完成全部任务
    Done,
}

/// Agent 工具调用轮数的安全上限，防止无限循环
const MAX_TOOL_ROUNDS: usize = 30;

/// Agent 应用的主结构体，持有运行所需的全部核心资源
#[derive(Clone, Debug)]
pub struct AgentApp {
    /// LLM Provider，负责与 LLM API 通信（支持 Anthropic/OpenAI 等）
    client: crate::api::LlmProvider,
    /// 工作区根目录的绝对路径，所有文件操作以此为基准
    workspace_root: PathBuf,
    /// 技能加载器，用 RwLock 包装以支持安装后热更新
    skills: Arc<RwLock<SkillLoader>>,
    /// 技能加载目录列表（用户目录 + 工作区目录），用于安装后重新加载
    skill_dirs: Vec<PathBuf>,
    /// 使用的模型 ID（如 "claude-sonnet-4-20250514" 或 "gpt-4o"）
    model: String,
    /// 每次调用 API 时请求的最大 token 数量
    max_tokens: u32,
    /// 当前 profile 的配额规则
    quotas: Vec<crate::infra::usage::QuotaRule>,
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
    fn parent() -> Self {
        Self {
            allow_task: true,
            use_todo_reminder: true,
        }
    }

    /// 创建子代理（subagent）的运行配置
    fn child() -> Self {
        Self {
            allow_task: false,
            use_todo_reminder: true,
        }
    }
}

impl AgentApp {
    /// 从配置文件和环境变量初始化 Agent 应用
    pub async fn from_env() -> AgentResult<Self> {
        let _ = dotenv();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let info = crate::api::create_provider()?;
        let model = info.model;
        let max_tokens = info.max_tokens;

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
            client: info.provider,
            workspace_root,
            skills: Arc::new(RwLock::new(skills)),
            skill_dirs,
            model,
            max_tokens,
            quotas: info.quotas,
        })
    }

    /// 获取当前 profile 的配额规则
    pub fn quotas(&self) -> &[crate::infra::usage::QuotaRule] {
        &self.quotas
    }

    /// 列出所有已安装技能的摘要信息
    pub fn list_skills(&self) -> Vec<crate::skills::SkillSummary> {
        self.skills.read().unwrap().list_skills()
    }

    /// 处理用户的一次对话输入，返回 Agent 的最终回复文本
    pub async fn handle_user_turn(
        &self,
        history: &mut Vec<ApiMessage>,
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> AgentResult<String> {
        let mut logger = ConversationLogger::create();

        history.push(ApiMessage::user_text(user_input));
        logger.log(&format!("=== 用户 ===\n{user_input}"));

        let system_prompt = build_system_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
        );
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
        let mut last_micro_compact = Instant::now();
        let micro_compact_interval = Duration::from_secs(60 * 60); // 1 小时

        for _ in 0..MAX_TOOL_ROUNDS {
            // 第一层：micro_compact — 距上次压缩超过 1 小时才执行
            if last_micro_compact.elapsed() >= micro_compact_interval {
                println!("[micro_compact 已触发]");
                compact::micro_compact(messages);
                last_micro_compact = Instant::now();
            }
            // 第二层：auto_compact — token 超阈值时触发
            if compact::estimate_tokens(messages) > compact::TOKEN_THRESHOLD {
                println!("[auto_compact 已触发]");
                *messages = compact::auto_compact(
                    &self.client,
                    &self.model,
                    &self.quotas,
                    &self.workspace_root,
                    &*messages,
                )
                .await?;
            }

            let tools = toolbox.tool_schemas(config.allow_task);
            let request = ProviderRequest {
                model: &self.model,
                system: &system_prompt,
                messages,
                tools: &tools,
                max_tokens: self.max_tokens,
            };
            let response = match self.client.create_message(&request, &self.quotas).await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("[Agent] create_message 失败！错误: {e:#}");
                    return Err(e);
                }
            };
            let stop_reason = response.stop_reason.clone();
            messages.push(ApiMessage::assistant_blocks(&response.content)?);

            if stop_reason != "tool_calls" {
                let _ = event_tx.send(AgentEvent::TurnEnd).await;
                return Ok(response.final_text());
            }

            let mut results = Vec::new();
            let mut used_todo = false;
            let mut manual_compact = false;

            for block in &response.content {
                if let ResponseContentBlock::ToolUse { id, name, input } = block {
                    let input_preview = preview_text(&input.to_string(), 200);
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
                            let _ = event_tx
                                .send(AgentEvent::ToolCall {
                                    name: "task".to_owned(),
                                    input: input.clone(),
                                })
                                .await;
                            self.run_subagent(prompt, logger, event_tx).await?
                        }
                    } else if name == "compact" {
                        manual_compact = true;
                        let _ = event_tx
                            .send(AgentEvent::ToolCall {
                                name: "compact".to_owned(),
                                input: input.clone(),
                            })
                            .await;
                        "正在压缩...".to_owned()
                    } else {
                        match toolbox.dispatch(name, input).await {
                            Ok(dispatch) => {
                                used_todo |= dispatch.used_todo;
                                let _ = event_tx
                                    .send(AgentEvent::ToolCall {
                                        name: name.clone(),
                                        input: input.clone(),
                                    })
                                    .await;
                                let _ = event_tx
                                    .send(AgentEvent::ToolResult {
                                        name: name.clone(),
                                        output: preview_text(&dispatch.output, 200),
                                    })
                                    .await;
                                dispatch.output
                            }
                            Err(e) => {
                                let msg = format!("Error: {e}");
                                let _ = event_tx
                                    .send(AgentEvent::ToolResult {
                                        name: name.clone(),
                                        output: msg.clone(),
                                    })
                                    .await;
                                msg
                            }
                        }
                    };

                    logger.log(&format!("=== 工具结果: {name} ===\n{output}"));

                    // 大结果持久化到磁盘，消息中只保留预览
                    let processed_output = storage::maybe_persist(id, &output);

                    results.push(tool_result_block(id, processed_output));
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
                    &self.quotas,
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
    async fn run_subagent(
        &self,
        prompt: String,
        logger: &mut ConversationLogger,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) -> AgentResult<String> {
        let system_prompt = build_subagent_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
        );
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
}

/// 构造一个工具执行结果的 JSON 块，用于回传给 Claude API
fn tool_result_block(tool_use_id: &str, content: String) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content,
    })
}

/// 生成父代理（顶层 Agent）的系统提示词
fn build_system_prompt(workspace_root: &std::path::Path, skills_desc: &str) -> String {
    let platform = if cfg!(windows) {
        "Windows (PowerShell)。使用 PowerShell 语法：用 Get-ChildItem 代替 ls，Get-Content 代替 cat，-Command 代替 -lc，; 代替 &&"
    } else {
        "Unix (bash)"
    };
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
        输出规则：\n\
        - 研究类任务：收集完资料后必须输出完整内容，不能只说整理完毕。\n\
        - 长篇内容（>500字）应写入文件并告知用户文件路径。\n\
        - 如果工具结果被持久化到磁盘（包含 <persisted-output> 标签），可以随时用 read_file 读取完整内容。\n\n\
        其他工具：\n\
        - 使用 todo 工具规划多步骤工作。\n\
        - 使用 task 工具委派子任务（子代理拥有独立上下文）。\n\n\
        可用技能：\n{}",
        workspace_root.display(),
        skills_desc
    )
}

/// 生成子代理的系统提示词
fn build_subagent_prompt(workspace_root: &std::path::Path, skills_desc: &str) -> String {
    format!(
        "你是一个编程子代理，工作目录：{}。\n完成给定任务，按需使用工具，然后返回简洁的摘要。不能调用 task 工具。\n\n\
        已安装的技能：\n{skills_desc}\n\n\
        如果已安装的技能覆盖当前任务，直接调用 load_skill 加载；否则跳过技能流程，直接执行。",
        workspace_root.display()
    )
}
