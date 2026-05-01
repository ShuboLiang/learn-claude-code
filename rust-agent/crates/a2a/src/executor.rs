use std::sync::Arc;

use a2a::*;
use a2a_server::executor::{AgentExecutor, ExecutorContext};
use dashmap::DashMap;
use futures::stream::{self, BoxStream};
use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::context::ContextService;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Clone)]
pub struct RustAgentExecutor {
    agent: AgentApp,
    contexts: Arc<DashMap<String, ContextService>>,
}

impl RustAgentExecutor {
    pub fn new(agent: AgentApp) -> Self {
        Self {
            agent,
            contexts: Arc::new(DashMap::new()),
        }
    }
}

impl AgentExecutor for RustAgentExecutor {
    fn execute(
        &self,
        ctx: ExecutorContext,
    ) -> BoxStream<'static, Result<StreamResponse, A2AError>> {
        let agent = self.agent.clone();
        let contexts = self.contexts.clone();
        let task_id = ctx.task_id.clone();
        let context_id = ctx.context_id.clone();
        let is_new_task = ctx.stored_task.is_none();
        let user_input = extract_user_input(&ctx.message);
        let history = ctx.stored_task.and_then(|t| t.history).unwrap_or_default();

        let (stream_tx, stream_rx) = mpsc::channel::<Result<StreamResponse, A2AError>>(64);

        tokio::spawn(async move {
            // Get or create context service
            let ctx_service = if is_new_task {
                ContextService::new()
            } else {
                contexts
                    .get(&task_id)
                    .map(|e| e.value().clone())
                    .unwrap_or_default()
            };

            let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(64);

            // Run agent in background
            let agent_clone = agent.clone();
            let mut ctx_service_for_agent = ctx_service.clone();
            let agent_task = tokio::spawn(async move {
                agent_clone
                    .handle_user_turn(&mut ctx_service_for_agent, &user_input, event_tx)
                    .await
            });

            // 收集中间事件到 buffer，不发送到 stream
            let mut buffer = String::new();
            while let Some(event) = event_rx.recv().await {
                match &event {
                    AgentEvent::TextDelta(text) => buffer.push_str(text),
                    AgentEvent::ToolCall {
                        id: _,
                        name,
                        input,
                        parallel_index,
                    } => {
                        let prefix = match parallel_index {
                            Some((idx, total)) => format!("[{}/{}] ", idx, total),
                            None => "".to_string(),
                        };
                        buffer.push_str(&format!(
                            "{}调用工具: `{}`\n参数: `{}`\n",
                            prefix, name, input
                        ));
                    }
                    AgentEvent::ToolResult {
                        id: _,
                        name,
                        output,
                        parallel_index,
                    } => {
                        let prefix = match parallel_index {
                            Some((idx, total)) => format!("[{}/{}] ", idx, total),
                            None => "".to_string(),
                        };
                        buffer.push_str(&format!("{}工具 `{}` 结果:\n{}\n", prefix, name, output));
                    }
                    AgentEvent::TurnEnd { .. } => {}
                    AgentEvent::Done => {}
                    AgentEvent::Error { .. } => {}
                    AgentEvent::Retrying { .. } => {}
                }
            }

            // Wait for agent completion and emit final Task snapshot
            match agent_task.await {
                Ok(Ok(final_text)) => {
                    contexts.insert(task_id.clone(), ctx_service);

                    let content = if buffer.trim().is_empty() {
                        final_text
                    } else {
                        buffer
                    };

                    let mut final_history = history;
                    final_history
                        .push(Message::new(Role::Agent, vec![Part::text(content.clone())]));

                    let reply = Message::new(Role::Agent, vec![Part::text(content)]);

                    let task = Task {
                        id: task_id.clone(),
                        context_id: context_id.clone(),
                        status: TaskStatus {
                            state: TaskState::Completed,
                            message: Some(reply),
                            timestamp: Some(chrono::Utc::now()),
                        },
                        artifacts: None,
                        history: Some(final_history),
                        metadata: None,
                    };
                    let _ = stream_tx.send(Ok(StreamResponse::Task(task))).await;
                }
                Ok(Err(e)) => {
                    let msg = e.to_string();
                    let mut final_history = history;
                    final_history.push(Message::new(Role::Agent, vec![Part::text(msg.clone())]));

                    let task = Task {
                        id: task_id.clone(),
                        context_id: context_id.clone(),
                        status: TaskStatus {
                            state: TaskState::Failed,
                            message: Some(Message::new(Role::Agent, vec![Part::text(msg.clone())])),
                            timestamp: Some(chrono::Utc::now()),
                        },
                        history: Some(final_history),
                        artifacts: None,
                        metadata: None,
                    };
                    let _ = stream_tx.send(Ok(StreamResponse::Task(task))).await;
                }
                Err(e) => {
                    let _ = stream_tx
                        .send(Err(A2AError::internal(format!("agent task panicked: {e}"))))
                        .await;
                }
            }
        });

        Box::pin(ReceiverStream::new(stream_rx))
    }

    fn cancel(&self, ctx: ExecutorContext) -> BoxStream<'static, Result<StreamResponse, A2AError>> {
        let task_id = ctx.task_id.clone();
        let context_id = ctx.context_id.clone();

        Box::pin(stream::once(async move {
            Ok(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id,
                context_id,
                status: TaskStatus {
                    state: TaskState::Canceled,
                    message: None,
                    timestamp: Some(chrono::Utc::now()),
                },
                metadata: None,
            }))
        }))
    }
}

fn extract_user_input(message: &Option<Message>) -> String {
    message
        .as_ref()
        .map(|msg| {
            msg.parts
                .iter()
                .filter_map(|p| p.as_text())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn agent_event_to_stream_responses(
    task_id: &str,
    context_id: &str,
    event: AgentEvent,
) -> Vec<StreamResponse> {
    match event {
        AgentEvent::TextDelta(text) => {
            vec![StreamResponse::Message(Message {
                message_id: uuid::Uuid::new_v4().to_string(),
                context_id: Some(context_id.to_string()),
                task_id: Some(task_id.to_string()),
                role: Role::Agent,
                parts: vec![Part::text(text)],
                metadata: None,
                extensions: None,
                reference_task_ids: None,
            })]
        }
        AgentEvent::ToolCall {
            id: _,
            name,
            input,
            parallel_index,
        } => {
            let prefix = match parallel_index {
                Some((idx, total)) => format!("[{}/{}] ", idx, total),
                None => "".to_string(),
            };
            let text = format!("{}调用工具: `{}`\n参数: `{}`", prefix, name, input);
            vec![StreamResponse::Message(Message {
                message_id: uuid::Uuid::new_v4().to_string(),
                context_id: Some(context_id.to_string()),
                task_id: Some(task_id.to_string()),
                role: Role::Agent,
                parts: vec![Part::text(text)],
                metadata: None,
                extensions: None,
                reference_task_ids: None,
            })]
        }
        AgentEvent::ToolResult {
            id: _,
            name,
            output,
            parallel_index,
        } => {
            let prefix = match parallel_index {
                Some((idx, total)) => format!("[{}/{}] ", idx, total),
                None => "".to_string(),
            };
            let text = format!("{}工具 `{}` 结果:\n{}\n", prefix, name, output);

            let mut responses = vec![StreamResponse::Message(Message {
                message_id: uuid::Uuid::new_v4().to_string(),
                context_id: Some(context_id.to_string()),
                task_id: Some(task_id.to_string()),
                role: Role::Agent,
                parts: vec![Part::text(text)],
                metadata: None,
                extensions: None,
                reference_task_ids: None,
            })];

            responses.extend(detect_artifacts(task_id, context_id, &output));
            responses
        }
        AgentEvent::TurnEnd { .. } => {
            vec![StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: task_id.to_string(),
                context_id: context_id.to_string(),
                status: TaskStatus {
                    state: TaskState::Working,
                    message: None,
                    timestamp: Some(chrono::Utc::now()),
                },
                metadata: None,
            })]
        }
        AgentEvent::Done => vec![],
        AgentEvent::Retrying { .. } => vec![],
        AgentEvent::Error { code, message } => {
            vec![StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: task_id.to_string(),
                context_id: context_id.to_string(),
                status: TaskStatus {
                    state: TaskState::Failed,
                    message: Some(Message::new(
                        Role::Agent,
                        vec![Part::text(format!("[{code}] {message}"))],
                    )),
                    timestamp: Some(chrono::Utc::now()),
                },
                metadata: None,
            })]
        }
    }
}

fn detect_artifacts(task_id: &str, context_id: &str, output: &str) -> Vec<StreamResponse> {
    let mut responses = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("<persisted-output") {
        let after = &rest[start..];
        if let Some(path_start) = after.find("path=\"") {
            let after_path = &after[path_start + 6..];
            if let Some(path_end) = after_path.find('"') {
                let path = &after_path[..path_end];
                responses.push(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: task_id.to_string(),
                    context_id: context_id.to_string(),
                    artifact: Artifact {
                        artifact_id: uuid::Uuid::new_v4().to_string(),
                        name: Some(
                            std::path::Path::new(path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("output.txt")
                                .to_string(),
                        ),
                        description: Some("Persisted tool output".to_string()),
                        parts: vec![Part::url(path)],
                        metadata: None,
                        extensions: None,
                    },
                    append: None,
                    last_chunk: None,
                    metadata: None,
                }));
                rest = &after_path[path_end..];
                continue;
            }
        }
        break;
    }
    responses
}
