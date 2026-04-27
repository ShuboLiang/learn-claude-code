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
use crate::bots::BotRegistry;
use crate::context::ContextService;
use crate::context::compact;
use crate::infra::circuit_breaker::ToolCircuitBreaker;
use crate::infra::logging::ConversationLogger;
use crate::infra::storage;
use crate::infra::utils::preview_text;
use crate::skills::SkillLoader;
use crate::skills::hub as skillhub;
use crate::tools::AgentToolbox;
use crate::tools::extension::ToolExtension;

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
        token_usage: Option<crate::api::types::TokenUsage>,
    },
    Done,
    Error {
        code: String,
        message: String,
    },
}

const MAX_TOOL_ROUNDS: usize = 30;
const MAX_PARALLEL_TASKS: usize = 5;

/// Agent 身份信息
#[derive(Clone, Debug, Default)]
pub struct AgentIdentity {
    /// 昵称，如 "小明"
    pub nickname: String,
    /// 职位/角色，如 "代码审查"
    pub role: String,
}

impl AgentIdentity {
    pub fn new(nickname: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            nickname: nickname.into(),
            role: role.into(),
        }
    }

    /// 完整称呼，如 "小明（代码审查）"
    pub fn display_name(&self) -> String {
        if self.nickname.is_empty() && self.role.is_empty() {
            return "Agent".to_owned();
        }
        if self.role.is_empty() {
            return self.nickname.clone();
        }
        if self.nickname.is_empty() {
            return self.role.clone();
        }
        format!("{}（{}）", self.nickname, self.role)
    }
}

#[derive(Clone)]
pub struct AgentApp {
    client: crate::api::LlmProvider,
    workspace_root: PathBuf,
    skills: Arc<RwLock<SkillLoader>>,
    skill_dirs: Vec<PathBuf>,
    model: String,
    max_tokens: u32,
    tool_extension: Option<Arc<dyn ToolExtension>>,
    identity: AgentIdentity,
    token_tracker: crate::infra::token_tracker::TokenTracker,
    bots: BotRegistry,
}

impl std::fmt::Debug for AgentApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentApp")
            .field("workspace_root", &self.workspace_root)
            .field("skills", &self.skills)
            .field("skill_dirs", &self.skill_dirs)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field(
                "tool_extension",
                &self.tool_extension.as_ref().map(|_| "<dyn ToolExtension>"),
            )
            .field("identity", &self.identity)
            .finish()
    }
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
        // 先加载配置并注入 extra_env，确保后续读取的环境变量生效
        let _ = crate::infra::config::AppConfig::load().ok();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let info = crate::api::create_provider()?;
        let model = info.model;
        let max_tokens = info.max_tokens;

        let skillhub_available = skillhub::ensure_cli_installed().await;
        if skillhub_available {
            println!("SkillHub CLI 已就绪。");
        }

        // 技能目录优先级：AGENT_SKILLS_DIRS 环境变量 > config.json skills_dirs > 默认目录
        let skill_dirs = if let Ok(dirs_env) = std::env::var("AGENT_SKILLS_DIRS") {
            parse_skill_dirs(&dirs_env)
        } else {
            let config_skill_dirs = crate::infra::config::AppConfig::load()
                .ok()
                .and_then(|cfg| {
                    if cfg.skills_dirs.is_empty() {
                        None
                    } else {
                        Some(parse_skill_dirs(&cfg.skills_dirs.join(",")))
                    }
                });
            config_skill_dirs.unwrap_or_else(|| {
                let user_skills_dir = dirs::home_dir()
                    .map(|p| p.join(".rust-agent").join("skills"))
                    .unwrap_or_default();
                vec![user_skills_dir, workspace_root.join("skills")]
            })
        };

        for dir in &skill_dirs {
            println!("[Agent] 技能目录: {}", dir.display());
        }

        let skills = SkillLoader::load_from_dirs(
            &skill_dirs.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        )?;

        let identity = Self::load_identity();

        let bots = BotRegistry::load()?;
        if bots.is_empty() {
            println!("[Agent] 未找到任何 Bot 定义");
        } else {
            println!("[Agent] 已加载 {} 个 Bot", bots.len());
        }

        Ok(Self {
            client: info.provider,
            workspace_root,
            skills: Arc::new(RwLock::new(skills)),
            skill_dirs,
            model,
            max_tokens,
            tool_extension: None,
            identity,
            token_tracker: crate::infra::token_tracker::TokenTracker::new(),
            bots,
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

    /// 注入外部工具扩展
    pub fn with_extension(mut self, ext: Arc<dyn ToolExtension>) -> Self {
        self.tool_extension = Some(ext);
        self
    }

    /// 设置 Agent 身份信息
    pub fn with_identity(mut self, identity: AgentIdentity) -> Self {
        self.identity = identity;
        self
    }

    /// 获取当前身份信息
    pub fn identity(&self) -> &AgentIdentity {
        &self.identity
    }

    /// 从环境变量和配置文件加载身份信息
    fn load_identity() -> AgentIdentity {
        // 先加载配置并注入 extra_env，确保后续读取的环境变量生效
        let _ = crate::infra::config::AppConfig::load().ok();
        // 1. 环境变量最高优先级
        if let (Ok(nick), Ok(role)) = (std::env::var("AGENT_NICKNAME"), std::env::var("AGENT_ROLE"))
        {
            return AgentIdentity::new(nick, role);
        }
        if let Ok(nick) = std::env::var("AGENT_NICKNAME") {
            return AgentIdentity::new(nick, "");
        }
        if let Ok(role) = std::env::var("AGENT_ROLE") {
            return AgentIdentity::new("", role);
        }

        // 2. config.json 次之
        if let Ok(cfg) = crate::infra::config::AppConfig::load() {
            return AgentIdentity::new(
                cfg.agent_nickname.unwrap_or_default(),
                cfg.agent_role.unwrap_or_default(),
            );
        }

        AgentIdentity::default()
    }

    /// 获取工具 schema 列表（用于 A2A 协议 Agent Card 生成）
    pub fn tool_schemas(&self) -> Vec<serde_json::Value> {
        let mut toolbox = crate::tools::AgentToolbox::new(
            self.workspace_root.clone(),
            Arc::clone(&self.skills),
            self.skill_dirs.clone(),
        );
        if let Some(ext) = &self.tool_extension {
            toolbox = toolbox.with_extension(Arc::clone(ext));
        }
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

        let bot_list = self.bots.descriptions_for_system_prompt();
        let system_prompt = build_system_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
            &self.model,
            &self.identity,
            &bot_list,
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
        if let Some(ext) = &self.tool_extension {
            toolbox = toolbox.with_extension(Arc::clone(ext));
        }
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
                Ok(resp) => {
                    self.token_tracker.record(&self.model, &resp.usage);
                    resp
                }
                Err(e) => {
                    eprintln!("[Agent] create_message 失败！错误: {e:#}");
                    if config.emit_events {
                        let _ = event_tx
                            .send(AgentEvent::Error {
                                code: "llm_api_error".to_owned(),
                                message: format!("{e:#}"),
                            })
                            .await;
                    }
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
                            token_usage: Some(self.token_tracker.snapshot().total),
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
            let mut bot_calls: Vec<ToolCallInfo> = Vec::new();
            let mut other_calls: Vec<ToolCallInfo> = Vec::new();
            for tc in tool_calls {
                if tc.name == "task" {
                    task_calls.push(tc);
                } else if tc.name == "call_bot" {
                    bot_calls.push(tc);
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

            if !bot_calls.is_empty() {
                for tc in &bot_calls {
                    let bot_name = tc
                        .input
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let input_preview = preview_text(&tc.input.to_string(), 200);
                    logger.log(&format!(
                        "=== 工具调用: call_bot(name={bot_name}) ===\n输入: {input_preview}"
                    ));
                    let _ = event_tx
                        .send(AgentEvent::ToolCall {
                            name: "call_bot".to_owned(),
                            input: tc.input.clone(),
                            parallel_index: None,
                        })
                        .await;
                }

                let mut bot_handles = Vec::new();
                for tc in &bot_calls {
                    let bot_name = tc
                        .input
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let bot_task = tc
                        .input
                        .get("task")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let app = self.clone();
                    let event_tx = Arc::clone(event_tx);
                    bot_handles.push(tokio::spawn(async move {
                        app.run_bot(&bot_name, &bot_task, &event_tx).await
                    }));
                }

                for (idx, handle) in bot_handles.into_iter().enumerate() {
                    let tc_id = bot_calls[idx].id.clone();
                    let output = match handle.await {
                        Ok(Ok(out)) => out,
                        Ok(Err(e)) => format!("Bot 子代理执行失败: {e}"),
                        Err(e) => format!("Bot 子代理异常: {e}"),
                    };
                    let _ = event_tx
                        .send(AgentEvent::ToolResult {
                            name: "call_bot".to_owned(),
                            output: preview_text(&output, 200),
                            parallel_index: if bot_calls.len() > 1 {
                                Some((idx + 1, bot_calls.len()))
                            } else {
                                None
                            },
                        })
                        .await;
                    logger.log(&format!(
                        "=== 工具结果: call_bot (name={}) ===\n{output}",
                        bot_calls[idx]
                            .input
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    ));
                    let processed = storage::maybe_persist(&tc_id, &output);
                    results.push(tool_result_block(&tc_id, processed));
                }
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
                                token_usage: Some(self.token_tracker.snapshot().total),
                            })
                            .await;
                        return Err(e);
                    }
                }
                let _ = event_tx
                    .send(AgentEvent::TurnEnd {
                        api_calls: api_call_count,
                        token_usage: Some(self.token_tracker.snapshot().total),
                    })
                    .await;
                return Ok("对话已手动压缩。".to_owned());
            }
        }

        let _ = event_tx
            .send(AgentEvent::TurnEnd {
                api_calls: api_call_count,
                token_usage: Some(self.token_tracker.snapshot().total),
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
            &self.identity,
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

    /// 运行 Bot 子代理：用 Bot 的 BOT.md body 作为 system prompt，Bot 专属技能运行
    async fn run_bot(
        &self,
        bot_name: &str,
        task: &str,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    ) -> AgentResult<String> {
        let available = self.bots.list();
        let bot_names: Vec<&str> = available.iter().map(|b| b.name.as_str()).collect();
        let bot = self.bots.find(bot_name).ok_or_else(|| {
            anyhow::anyhow!(
                "找不到 Bot: '{bot_name}'。可用 Bot：{}",
                bot_names.join(", ")
            )
        })?;

        // 克隆 AgentApp 并把技能加载器替换为 Bot 专属技能（不继承全局技能）
        let mut bot_app = self.clone();
        bot_app.skills = Arc::new(RwLock::new(bot.skills.clone()));

        let skills_desc = bot.skills.descriptions_for_system_prompt();

        let identity_line = if bot.metadata.nickname.is_empty() && bot.metadata.role.is_empty() {
            format!("你是专业的 {}", bot_name)
        } else if bot.metadata.role.is_empty() {
            format!("你是 {}（{}）", bot.metadata.nickname, bot_name)
        } else if bot.metadata.nickname.is_empty() {
            format!("你是 {}（{}）", bot.metadata.role, bot_name)
        } else {
            format!(
                "你是 {}（{}，{}）",
                bot.metadata.nickname, bot_name, bot.metadata.role
            )
        };

        let platform = if cfg!(windows) { "Windows (PowerShell)" } else { "Unix (bash)" };

        // 将 BOT.md 的 body 内容（行为指令）注入 system prompt
        // body 可能为空（BOT.md 只有 frontmatter 没有正文）
        let bot_body_section = if bot.body.trim().is_empty() {
            String::new()
        } else {
            format!("\n## 行为指令（来自 BOT.md）\n\n{bot_body}\n", bot_body = bot.body)
        };

        let system_prompt = format!(
            r#"{identity_line}。
工作目录：{workspace}。
平台：{platform}。
你是一个具备独立上下文的 Bot 子代理，拥有专属技能。
完成用户交给你的任务，按需使用工具，然后返回完整的处理结果。

工具限制：task 和 call_bot 工具已为你禁用，不可调用。

专属技能：
{skills_desc}
{bot_body_section}
提示：
- 如果已安装的技能覆盖当前任务，直接调用 load_skill 加载后再执行。
- 否则跳过技能流程，直接使用 bash 等工具执行。
- **脚本执行规则**：
  - 已安装技能的脚本（已在 skills/ 目录下）→ 用 bash 直接从技能目录运行。
  - 只有凭空生成的临时代码片段才使用 exec_script 工具执行。
  - 执行前禁止先检查环境，直接运行，失败再报告。
  - 禁止 write_file 写临时脚本到工作区再用 bash 执行。
- 完成后返回详细的结果，不要只说"已完成"。
- **信息不明确时必须询问用户**：当任务存在多种可行方案（如不同的算法、权重模型、模板风格等），或关键信息缺失导致无法做出唯一判断时，**必须**先向用户确认，**禁止**擅自选择默认值直接执行。"#,
            identity_line = identity_line,
            workspace = self.workspace_root.display(),
            skills_desc = skills_desc,
            bot_body_section = bot_body_section,
        );

        let mut sub_logger = ConversationLogger::create();
        sub_logger.log(&format!("=== Bot 子代理系统提示词 ===\n{system_prompt}"));

        let mut bot_ctx = ContextService::new();
        bot_ctx.push_user_text(task);
        bot_app.run_agent_loop(
            &mut bot_ctx,
            system_prompt,
            AgentRunConfig::child(),
            &mut sub_logger,
            event_tx,
        )
        .await
    }
}

/// 将逗号/分号分隔的技能目录字符串解析为 PathBuf 列表，支持 ~/ 展开
fn parse_skill_dirs(raw: &str) -> Vec<PathBuf> {
    raw.split([',', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with("~/") {
                dirs::home_dir()
                    .map(|p| p.join(&s[2..]))
                    .unwrap_or_else(|| PathBuf::from(s))
            } else {
                PathBuf::from(s)
            }
        })
        .collect()
}

fn tool_result_block(tool_use_id: &str, content: String) -> Value {
    json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content })
}

fn build_system_prompt(
    workspace_root: &std::path::Path,
    skills_desc: &str,
    model: &str,
    identity: &AgentIdentity,
    bot_list: &str,
) -> String {
    let platform = if cfg!(windows) {
        "Windows (PowerShell)。"
    } else {
        "Unix (bash)"
    };
    let identity_line = if identity.nickname.is_empty() && identity.role.is_empty() {
        format!("你是 {model}，一个编程助手。")
    } else if identity.role.is_empty() {
        format!("你是 {model}，昵称是 {}。", identity.nickname)
    } else if identity.nickname.is_empty() {
        format!("你是 {model}，担任{}。", identity.role)
    } else {
        format!(
            "你是 {model}，昵称是 {}，担任{}。",
            identity.nickname, identity.role
        )
    };

    let bot_section = if bot_list.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n\
            ## Bot 子代理（专业领域专家）\n\
            Bot 是拥有专属技能的专家子代理，优先级高于普通技能。\n\
            **委派规则**：当任务匹配某个 Bot 的职责范围时，优先调用 call_bot 委派，不要改用技能。\n\
            **多轮编排**：复杂任务可分多轮委派：先调用数据收集 Bot → 拿到结果 → 调用分析/总结 Bot。\n\
            **并行调度**：多个独立的子任务可在同一轮并行调用多个 Bot（上限 5 个）。\n\
            **存在依赖**的任务必须串行：等上一个 Bot 返回后再调用下一个。\n\n\
            可用 Bot：\n{bot_list}"
        )
    };

    format!(
        "{identity_line}\n工作目录：{}。\n平台：{platform}\n优先使用工具解决问题，避免冗长解释。{bot_section}\n\n\
            任务执行流程 — 每个任务必须按以下顺序执行：\n\
            0. 先了解项目：读取目录结构和关键文件，理解项目上下文。\n\
            1. 检查任务是否匹配某个 Bot 的职责范围。如果匹配，直接调用 call_bot 委派。\n\
               **不可用技能替代 Bot，Bot 优先级高于技能。**\n\
            2. 检查已安装的技能是否覆盖当前任务。如果有，调用 load_skill。\n\
            3. 如果任务需要领域知识但没有匹配的已安装技能，调用 search_skillhub 搜索。\n\
               简单操作（文件读写、命令执行、搜索等）可直接跳过此步骤。\n\
            4. 如果 search_skillhub 返回了相关技能，调用 install_skill 安装它。\n\
            5. 只有在步骤 0-4 完成后，才能使用 bash 或其他工具执行具体操作。\n\
            6. 绝对不能跳过 Bot/技能检查直接使用 bash/curl 等工具（简单文件操作除外）。\n\
            7. 在完成 Bot/技能流程之前，绝对不能声称无法完成任务。\n\n\
            脚本执行规则：\n\
            - 已安装技能的脚本（已在 skills/ 目录下）→ 用 bash 直接从技能目录运行。\n\
            - 只有凭空生成的临时代码片段才使用 exec_script 工具执行。\n\
            - 禁止 write_file 写临时脚本到工作区再用 bash 执行。\n\n\
            输出规则：\n\
            - **交付产物优先**：在将某个任务步骤标记为完成（completed）时，你必须在同一条回复中展示该步骤产出的核心数据（如解析后的 JSON、代码、分析结论等）。\n\
            - **禁止隐瞒结果**：严禁在未展示关键数据的情况下仅回复“已完成”。\n\
            - **结果记录**：调用 todo 工具更新状态时，务必在 result_summary 字段中填入该任务产出的简要描述或数据预览，以便于后续流程引用。\n\
            - 研究类任务：收集完资料后必须输出完整内容，不能只说整理完毕。\n\
            - 长篇内容（>500字）应写入文件并告知用户文件路径。\n\
            - 如果工具结果被持久化到磁盘（包含 <persisted-output> 标签），可以随时用 read_file 读取完整内容。\n\n\
            子代理并行执行规则：\n\
            - 你可以在一次响应中返回多个 task 或 call_bot 调用来并行执行多个子代理/Bot。\n\
            - **并行执行条件**（需全部满足）：2+ 个独立任务、任务间无依赖、无共享文件冲突。\n\
            - **串行执行条件**（任一触发）：任务间有依赖、共享文件/状态、范围不明确需先了解。\n\
            - 典型并行场景：同时研究多个不相关主题、同时探索不同模块、同时分析多个文件。\n\
            - 典型串行场景：先调研再实现、先写 schema 再写 API、当任务需要前一步结果才能决定下一步。\n\
            - 并行上限为 5 个子代理/Bot，超出部分将被忽略。\n\n\
            其他工具：\n\
            - 使用 todo 工具规划多步骤工作。\n\
            - 使用 task 工具委派普通子任务（子代理拥有独立上下文，支持并行）。\n\
            - 使用 call_bot 工具将专业任务委派给拥有专属技能的 Bot 子代理。\n\n\
            可用技能：\n{skills_desc}",
        workspace_root.display(),
    )
}

fn build_subagent_prompt(
    workspace_root: &std::path::Path,
    skills_desc: &str,
    identity: &AgentIdentity,
) -> String {
    let identity_line = if identity.nickname.is_empty() && identity.role.is_empty() {
        "你是一个编程子代理".to_owned()
    } else {
        format!("你是 {} 的子代理", identity.display_name())
    };
    format!(
        "{identity_line}，工作目录：{}。\n完成给定任务，按需使用工具，然后返回简洁的摘要。\n\n\
        工具限制：task 和 call_bot 工具已为你禁用，不可调用。\n\n\
        脚本执行规则：\n\
        - 已安装技能的脚本（已在 skills/ 目录下）→ 用 bash 直接从技能目录运行。\n\
        - 只有凭空生成的临时代码片段才使用 exec_script 工具执行。\n\
        - 执行前禁止先检查环境，直接运行，失败再报告。\n\
        - 禁止 write_file 写临时脚本到工作区再用 bash 执行。\n\n\
        已安装的技能：\n{skills_desc}\n\n\
        如果已安装的技能覆盖当前任务，直接调用 load_skill 加载；否则跳过技能流程，直接执行。",
        workspace_root.display()
    )
}
