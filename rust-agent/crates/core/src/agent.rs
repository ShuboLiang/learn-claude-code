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
use crate::context::ContextService;
use crate::context::compact;
use crate::infra::circuit_breaker::ToolCircuitBreaker;
use crate::infra::logging::ConversationLogger;
use crate::infra::storage;
use crate::infra::utils::preview_text;
use crate::skills::SkillLoader;
use crate::skills::hub as skillhub;
use crate::tools::AgentToolbox;

#[derive(Clone, Debug)]
pub enum AgentEvent {
    TextDelta(String),
    ToolCall {
        name: String,
        input: serde_json::Value,
        parallel_index: Option<(usize, usize)>,
    },
    ToolResult {
        name: String,
        output: String,
        parallel_index: Option<(usize, usize)>,
    },
    TurnEnd {
        api_calls: usize,
    },
    Done,
}

const MAX_TOOL_ROUNDS: usize = 30;
const MAX_PARALLEL_TASKS: usize = 5;

#[derive(Clone, Debug)]
pub struct AgentApp {
    client: crate::api::LlmProvider,
    workspace_root: PathBuf,
    skills: Arc<RwLock<SkillLoader>>,
    skill_dirs: Vec<PathBuf>,
    model: String,
    max_tokens: u32,
}

#[derive(Clone, Copy, Debug)]
struct AgentRunConfig {
    allow_task: bool,
    use_todo_reminder: bool,
    /// 是否向客户端发送 SSE 事件（子 agent 静默执行，不输出到 CLI）
    emit_events: bool,
}

impl AgentRunConfig {
    fn parent() -> Self {
        Self {
            allow_task: true,
            use_todo_reminder: true,
            emit_events: true,
        }
    }
    fn child() -> Self {
        Self {
            allow_task: false,
            use_todo_reminder: true,
            emit_events: false,
        }
    }
}

impl AgentApp {
    pub async fn from_env() -> AgentResult<Self> {
        let _ = dotenv();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let info = crate::api::create_provider()?;
        let model = info.model;
        let max_tokens = info.max_tokens;

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
        })
    }

    /// 获取 LLM Provider 的引用（供 /compact 等命令使用）
    pub fn client(&self) -> &crate::api::LlmProvider {
        &self.client
    }

    /// 获取模型 ID（供 /compact 等命令使用）
    pub fn model(&self) -> &str {
        &self.model
    }

    /// 获取工作区根目录（供 /compact 等命令使用）
    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    /// 获取工具 schema 列表（用于 A2A 协议 Agent Card 生成）
    pub fn tool_schemas(&self) -> Vec<serde_json::Value> {
        let toolbox = crate::tools::AgentToolbox::new(
            self.workspace_root.clone(),
            Arc::clone(&self.skills),
            self.skill_dirs.clone(),
        );
        toolbox
            .tool_schemas(true)
            .iter()
            .map(|v| (*v).clone())
            .collect()
    }

    pub fn list_skills(&self) -> Vec<crate::skills::SkillSummary> {
        self.skills.read().unwrap().list_skills()
    }

    pub async fn handle_user_turn(
        &self,
        ctx: &mut ContextService,
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> AgentResult<String> {
        let mut logger = ConversationLogger::create();

        ctx.push_user_text(user_input);
        logger.log(&format!("=== 用户 ===\n{user_input}"));

        let system_prompt = build_system_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
            &self.model,
        );
        logger.log(&format!("=== 系统提示词 ===\n{system_prompt}"));

        let event_tx = Arc::new(event_tx);

        let result = self
            .run_agent_loop(
                ctx,
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

    #[async_recursion]
    async fn run_agent_loop(
        &self,
        ctx: &mut ContextService,
        system_prompt: String,
        config: AgentRunConfig,
        logger: &mut ConversationLogger,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    ) -> AgentResult<String> {
        let mut toolbox = AgentToolbox::new(
            self.workspace_root.clone(),
            Arc::clone(&self.skills),
            self.skill_dirs.clone(),
        );
        let mut rounds_since_todo = 0usize;
        let mut last_micro_compact = Instant::now();
        let mut breaker = ToolCircuitBreaker::new();
        let micro_compact_interval = Duration::from_secs(60 * 60);

        let mut api_call_count: usize = 0;

        for _ in 0..MAX_TOOL_ROUNDS {
            if last_micro_compact.elapsed() >= micro_compact_interval {
                println!("[micro_compact 已触发]");
                ctx.micro_compact();
                last_micro_compact = Instant::now();
            }
            if ctx.estimate_tokens() > compact::TOKEN_THRESHOLD {
                println!("[auto_compact 已触发]");
                match ctx
                    .auto_compact(&self.client, &self.model, &self.workspace_root)
                    .await
                {
                    Ok(new_messages) => ctx.replace(new_messages),
                    Err(e) => eprintln!("[auto_compact 失败: {e:#}]"),
                }
            }

            let tools = toolbox.tool_schemas(config.allow_task);
            let request = ProviderRequest {
                model: &self.model,
                system: &system_prompt,
                messages: ctx.messages(),
                tools: &tools,
                max_tokens: self.max_tokens,
            };
            let response = match self.client.create_message(&request).await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("[Agent] create_message 失败！错误: {e:#}");
                    return Err(e);
                }
            };
            api_call_count += 1;
            let stop_reason = response.stop_reason.clone();
            ctx.push(ApiMessage::assistant_blocks(&response.content)?);

            if stop_reason != "tool_calls" {
                let text = response.final_text();
                // 将文本响应通过 SSE 通道发送给客户端（子 agent 静默）
                if config.emit_events {
                    if !text.is_empty() {
                        let _ = event_tx.send(AgentEvent::TextDelta(text)).await;
                    }
                    let _ = event_tx
                        .send(AgentEvent::TurnEnd {
                            api_calls: api_call_count,
                        })
                        .await;
                }
                return Ok(response.final_text());
            }

            let mut results = Vec::new();
            let mut used_todo = false;
            let mut manual_compact = false;

            struct ToolCallInfo {
                id: String,
                name: String,
                input: Value,
            }

            let tool_calls: Vec<ToolCallInfo> = response
                .content
                .iter()
                .filter_map(|block| {
                    if let ResponseContentBlock::ToolUse { id, name, input } = block {
                        Some(ToolCallInfo {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            let mut task_calls: Vec<ToolCallInfo> = Vec::new();
            let mut other_calls: Vec<ToolCallInfo> = Vec::new();
            for tc in tool_calls {
                if tc.name == "task" {
                    task_calls.push(tc);
                } else {
                    other_calls.push(tc);
                }
            }

            for tc in &other_calls {
                let input_preview = preview_text(&tc.input.to_string(), 200);
                logger.log(&format!(
                    "=== 工具调用: {} ===\n输入: {input_preview}",
                    tc.name
                ));

                let output = if tc.name == "compact" {
                    manual_compact = true;
                    if config.emit_events {
                        let _ = event_tx
                            .send(AgentEvent::ToolCall {
                                name: tc.name.clone(),
                                input: tc.input.clone(),
                                parallel_index: None,
                            })
                            .await;
                    }
                    "正在压缩...".to_owned()
                } else if breaker.is_open(&tc.name) {
                    // 工具已熔断，直接返回提示信息，不执行
                    let count = breaker.failure_count(&tc.name);
                    let msg = ToolCircuitBreaker::blocked_message(&tc.name, count);
                    if config.emit_events {
                        let _ = event_tx
                            .send(AgentEvent::ToolResult {
                                name: tc.name.clone(),
                                output: msg.clone(),
                                parallel_index: None,
                            })
                            .await;
                    }
                    msg
                } else {
                    if config.emit_events {
                        let _ = event_tx
                            .send(AgentEvent::ToolCall {
                                name: tc.name.clone(),
                                input: tc.input.clone(),
                                parallel_index: None,
                            })
                            .await;
                    }
                    match toolbox.dispatch(&tc.name, &tc.input).await {
                        Ok(dispatch) => {
                            breaker.record_success(&tc.name);
                            used_todo |= dispatch.used_todo;
                            if config.emit_events {
                                let _ = event_tx
                                    .send(AgentEvent::ToolResult {
                                        name: tc.name.clone(),
                                        output: preview_text(&dispatch.output, 200),
                                        parallel_index: None,
                                    })
                                    .await;
                            }
                            dispatch.output
                        }
                        Err(e) => {
                            breaker.record_failure(&tc.name);
                            let msg = format!("Error: {e}");
                            if config.emit_events {
                                let _ = event_tx
                                    .send(AgentEvent::ToolResult {
                                        name: tc.name.clone(),
                                        output: msg.clone(),
                                        parallel_index: None,
                                    })
                                    .await;
                            }
                            msg
                        }
                    }
                };

                logger.log(&format!("=== 工具结果: {} ===\n{output}", tc.name));
                let processed_output = storage::maybe_persist(&tc.id, &output);
                results.push(tool_result_block(&tc.id, processed_output));
            }

            if !task_calls.is_empty() {
                if !config.allow_task {
                    for tc in &task_calls {
                        results.push(tool_result_block(
                            &tc.id,
                            "错误：task 工具在子代理中不可用".to_owned(),
                        ));
                    }
                } else {
                    let total = task_calls.len().min(MAX_PARALLEL_TASKS);
                    let actual_calls: Vec<_> = task_calls.into_iter().take(total).collect();
                    let is_parallel = actual_calls.len() > 1;

                    for (idx, tc) in actual_calls.iter().enumerate() {
                        let input_preview = preview_text(&tc.input.to_string(), 200);
                        logger.log(&format!(
                            "=== 工具调用: task (并行 {}/{}) ===\n输入: {input_preview}",
                            idx + 1,
                            actual_calls.len()
                        ));
                        let _ = event_tx
                            .send(AgentEvent::ToolCall {
                                name: "task".to_owned(),
                                input: tc.input.clone(),
                                parallel_index: if is_parallel {
                                    Some((idx + 1, actual_calls.len()))
                                } else {
                                    None
                                },
                            })
                            .await;
                    }

                    let mut handles = Vec::new();
                    for tc in &actual_calls {
                        let prompt = tc
                            .input
                            .get("prompt")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        let app = self.clone();
                        let event_tx = Arc::clone(event_tx);
                        handles.push(tokio::spawn(async move {
                            let mut sub_logger = ConversationLogger::create();
                            let result = app.run_subagent(prompt, &mut sub_logger, &event_tx).await;
                            (result, sub_logger)
                        }));
                    }

                    let mut sub_results: Vec<(String, ConversationLogger)> = Vec::new();
                    for handle in handles {
                        match handle.await {
                            Ok((Ok(output), sub_logger)) => sub_results.push((output, sub_logger)),
                            Ok((Err(e), sub_logger)) => {
                                sub_results.push((format!("子代理执行失败: {e}"), sub_logger))
                            }
                            Err(e) => sub_results.push((
                                format!("子代理任务异常: {e}"),
                                ConversationLogger::create(),
                            )),
                        }
                    }

                    for (idx, (output, _sub_logger)) in sub_results.iter().enumerate() {
                        let _ = event_tx
                            .send(AgentEvent::ToolResult {
                                name: "task".to_owned(),
                                output: preview_text(output, 200),
                                parallel_index: if is_parallel {
                                    Some((idx + 1, actual_calls.len()))
                                } else {
                                    None
                                },
                            })
                            .await;
                        logger.log(&format!(
                            "=== 工具结果: task (并行 {}/{}) ===\n{output}",
                            idx + 1,
                            actual_calls.len()
                        ));
                        let tc_id = &actual_calls[idx].id;
                        let processed = storage::maybe_persist(tc_id, output);
                        results.push(tool_result_block(tc_id, processed));
                    }
                }
            }

            rounds_since_todo = if used_todo { 0 } else { rounds_since_todo + 1 };
            if config.use_todo_reminder && rounds_since_todo >= 3 {
                results.push(
                    json!({ "type": "text", "text": "<reminder>请更新你的待办事项。</reminder>" }),
                );
            }

            ctx.push_user_blocks(results);

            if manual_compact {
                println!("[手动压缩]");
                match ctx
                    .auto_compact(&self.client, &self.model, &self.workspace_root)
                    .await
                {
                    Ok(new_messages) => ctx.replace(new_messages),
                    Err(e) => {
                        eprintln!("[手动压缩失败: {e:#}]");
                        let _ = event_tx
                            .send(AgentEvent::TurnEnd {
                                api_calls: api_call_count,
                            })
                            .await;
                        return Err(e);
                    }
                }
                let _ = event_tx
                    .send(AgentEvent::TurnEnd {
                        api_calls: api_call_count,
                    })
                    .await;
                return Ok("对话已手动压缩。".to_owned());
            }
        }

        let _ = event_tx
            .send(AgentEvent::TurnEnd {
                api_calls: api_call_count,
            })
            .await;
        Ok("已达到工具调用轮数安全上限，自动停止。".to_owned())
    }

    async fn run_subagent(
        &self,
        prompt: String,
        logger: &mut ConversationLogger,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    ) -> AgentResult<String> {
        let system_prompt = build_subagent_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
        );
        logger.log(&format!("=== 子代理系统提示词 ===\n{system_prompt}"));
        let mut sub_ctx = ContextService::new();
        sub_ctx.push_user_text(&prompt);
        self.run_agent_loop(
            &mut sub_ctx,
            system_prompt,
            AgentRunConfig::child(),
            logger,
            event_tx,
        )
        .await
    }
}

fn tool_result_block(tool_use_id: &str, content: String) -> Value {
    json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content })
}

fn build_system_prompt(workspace_root: &std::path::Path, skills_desc: &str, model: &str) -> String {
    let platform = if cfg!(windows) {
        "Windows (PowerShell)。使用 PowerShell 语法：用 Get-ChildItem 代替 ls，Get-Content 代替 cat，-Command 代替 -lc，; 代替 &&"
    } else {
        "Unix (bash)"
    };
    format!(
        "你是 {model}，一个编程助手，工作目录：{}。\n平台：{platform}\n优先使用工具解决问题，避免冗长解释。\n\n\
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
        子代理并行执行规则：\n\
        - 你可以在一次响应中返回多个 task 工具调用来并行执行多个子代理。\n\
        - **并行执行条件**（需全部满足）：2+ 个独立任务、任务间无依赖、无共享文件冲突。\n\
        - **串行执行条件**（任一触发）：任务间有依赖、共享文件/状态、范围不明确需先了解。\n\
        - 典型并行场景：同时研究多个不相关主题、同时探索不同模块、同时分析多个文件。\n\
        - 典型串行场景：先调研再实现、先写 schema 再写 API、需要前一步结果才能决定下一步。\n\
        - 并行上限为 5 个子代理，超出部分将被忽略。\n\n\
        其他工具：\n\
        - 使用 todo 工具规划多步骤工作。\n\
        - 使用 task 工具委派子任务（子代理拥有独立上下文，支持并行）。\n\n\
        可用技能：\n{}",
        workspace_root.display(),
        skills_desc
    )
}

fn build_subagent_prompt(workspace_root: &std::path::Path, skills_desc: &str) -> String {
    format!(
        "你是一个编程子代理，工作目录：{}。\n完成给定任务，按需使用工具，然后返回简洁的摘要。不能调用 task 工具。\n\n\
        已安装的技能：\n{skills_desc}\n\n\
        如果已安装的技能覆盖当前任务，直接调用 load_skill 加载；否则跳过技能流程，直接执行。",
        workspace_root.display()
    )
}
