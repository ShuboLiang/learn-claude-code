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

const MAX_TOKENS: u32 = 8_000;
const MAX_TOOL_ROUNDS: usize = 30;

#[derive(Clone, Debug)]
pub struct AgentApp {
    client: AnthropicClient,
    workspace_root: PathBuf,
    skills: Arc<SkillLoader>,
    model: String,
}

#[derive(Clone, Copy, Debug)]
struct AgentRunConfig {
    allow_task: bool,
    use_todo_reminder: bool,
}

impl AgentRunConfig {
    fn parent() -> Self {
        Self {
            allow_task: true,
            use_todo_reminder: true,
        }
    }

    fn child() -> Self {
        Self {
            allow_task: false,
            use_todo_reminder: true,
        }
    }
}

impl AgentApp {
    pub fn from_env() -> AgentResult<Self> {
        let _ = dotenv();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let client = AnthropicClient::from_env()?;
        let model = std::env::var("MODEL_ID").context("Missing MODEL_ID in environment or .env")?;
        let skills = SkillLoader::load_from_dir(&workspace_root.join("skills"))?;

        Ok(Self {
            client,
            workspace_root,
            skills: Arc::new(skills),
            model,
        })
    }

    pub async fn handle_user_turn(
        &self,
        history: &mut Vec<ApiMessage>,
        user_input: &str,
    ) -> AgentResult<String> {
        history.push(ApiMessage::user_text(user_input));
        self.run_agent_loop(history, self.system_prompt(), AgentRunConfig::parent())
            .await
    }

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
                        let dispatch = toolbox.dispatch(name, input).await?;
                        used_todo |= dispatch.used_todo;
                        println!("> {name}:");
                        println!("{}", preview(&dispatch.output));
                        dispatch.output
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

    async fn run_subagent(&self, prompt: String) -> AgentResult<String> {
        let mut messages = vec![ApiMessage::user_text(prompt)];
        self.run_agent_loop(
            &mut messages,
            self.subagent_system_prompt(),
            AgentRunConfig::child(),
        )
        .await
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a coding agent at {}.\nUse tools to solve tasks. Prefer acting over long explanations.\nUse the todo tool to plan multi-step work. Use the task tool to delegate subtasks with fresh context. Use load_skill before unfamiliar domain work.\n\nSkills available:\n{}",
            self.workspace_root.display(),
            self.skills.descriptions_for_system_prompt()
        )
    }

    fn subagent_system_prompt(&self) -> String {
        format!(
            "You are a coding subagent at {}.\nComplete the given task, use tools as needed, then return a concise summary. Do not call the task tool.",
            self.workspace_root.display()
        )
    }
}

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

fn tool_result_block(tool_use_id: &str, content: String) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content,
    })
}

fn preview(text: &str) -> String {
    const LIMIT: usize = 200;
    if text.chars().count() <= LIMIT {
        return text.to_owned();
    }
    let head = text.chars().take(LIMIT).collect::<String>();
    format!("{head}...")
}
