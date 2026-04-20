use rust_agent_core::agent::AgentEvent;

use crate::types::{
    Artifact, Message, Part, Role, StreamResponse, TaskArtifactUpdateEvent, TaskState,
    TaskStatus, TaskStatusUpdateEvent,
};

pub fn agent_event_to_stream_response(
    task_id: &str,
    context_id: &str,
    event: AgentEvent,
) -> Vec<StreamResponse> {
    match event {
        AgentEvent::TextDelta(text) => {
            vec![StreamResponse {
                task: None,
                message: Some(Message {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    context_id: None,
                    task_id: Some(task_id.to_string()),
                    role: Role::Agent,
                    parts: vec![Part::Text { text }],
                    metadata: None,
                    ..Default::default()
                }),
                status_update: None,
                artifact_update: None,
            }]
        }
        AgentEvent::ToolCall {
            name,
            input,
            parallel_index,
        } => {
            let prefix = match parallel_index {
                Some((idx, total)) => format!("🔧 [{}/{}] ", idx, total),
                None => "🔧 ".to_string(),
            };
            let text = format!("{}调用工具: `{}`\n参数: `{}`", prefix, name, input);
            vec![StreamResponse {
                task: None,
                message: Some(Message {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    context_id: None,
                    task_id: Some(task_id.to_string()),
                    role: Role::Agent,
                    parts: vec![Part::Text { text }],
                    metadata: None,
                    ..Default::default()
                }),
                status_update: None,
                artifact_update: None,
            }]
        }
        AgentEvent::ToolResult {
            name,
            output,
            parallel_index,
        } => {
            let prefix = match parallel_index {
                Some((idx, total)) => format!("✅ [{}/{}] ", idx, total),
                None => "✅ ".to_string(),
            };
            let text = format!("{}工具 `{}` 结果:\n{}\n", prefix, name, output);

            let mut responses = vec![StreamResponse {
                task: None,
                message: Some(Message {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    context_id: None,
                    task_id: Some(task_id.to_string()),
                    role: Role::Agent,
                    parts: vec![Part::Text { text }],
                    metadata: None,
                    ..Default::default()
                }),
                status_update: None,
                artifact_update: None,
            }];

            responses.extend(detect_artifacts(task_id, context_id, &output));
            responses
        }
        AgentEvent::TurnEnd { .. } => {
            vec![StreamResponse {
                task: None,
                message: None,
                status_update: Some(TaskStatusUpdateEvent {
                    task_id: task_id.to_string(),
                    context_id: context_id.to_string(),
                    status: TaskStatus {
                        state: TaskState::Working,
                        message: None,
                        timestamp: Some(chrono::Utc::now()),
                    },
                    metadata: None,
                }),
                artifact_update: None,
            }]
        }
        AgentEvent::Done => {
            vec![StreamResponse {
                task: None,
                message: None,
                status_update: Some(TaskStatusUpdateEvent {
                    task_id: task_id.to_string(),
                    context_id: context_id.to_string(),
                    status: TaskStatus {
                        state: TaskState::Completed,
                        message: None,
                        timestamp: Some(chrono::Utc::now()),
                    },
                    metadata: None,
                }),
                artifact_update: None,
            }]
        }
    }
}

fn detect_artifacts(
    task_id: &str,
    context_id: &str,
    output: &str,
) -> Vec<StreamResponse> {
    let mut responses = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("<persisted-output") {
        let after = &rest[start..];
        if let Some(path_start) = after.find("path=\"") {
            let after_path = &after[path_start + 6..];
            if let Some(path_end) = after_path.find('"') {
                let path = &after_path[..path_end];
                responses.push(StreamResponse {
                    task: None,
                    message: None,
                    status_update: None,
                    artifact_update: Some(TaskArtifactUpdateEvent {
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
                            parts: vec![Part::File {
                                name: None,
                                media_type: None,
                                raw: None,
                                url: Some(path.to_string()),
                            }],
                            metadata: None,
                            extensions: None,
                        },
                        append: None,
                        last_chunk: None,
                        metadata: None,
                    }),
                });
                rest = &after_path[path_end..];
                continue;
            }
        }
        break;
    }
    responses
}

impl StreamResponse {
    pub fn into_sse_event(self) -> axum::response::sse::Event {
        axum::response::sse::Event::default()
            .data(serde_json::to_string(&self).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta_becomes_message() {
        let responses =
            agent_event_to_stream_response("t1", "ctx-1", AgentEvent::TextDelta("hello".into()));
        assert_eq!(responses.len(), 1);
        assert!(responses[0].message.is_some());
    }

    #[test]
    fn detects_persisted_output_artifact() {
        let output = r#"Result:
<persisted-output path="/tmp/out.md">
Some content
"#;
        let responses = agent_event_to_stream_response(
            "t1",
            "ctx-1",
            AgentEvent::ToolResult {
                name: "bash".into(),
                output: output.into(),
                parallel_index: None,
            },
        );

        assert_eq!(responses.len(), 2);
        assert!(responses[1].artifact_update.is_some());
        let artifact_update = responses[1].artifact_update.as_ref().unwrap();
        assert_eq!(artifact_update.task_id, "t1");
        assert_eq!(artifact_update.context_id, "ctx-1");
        assert!(!artifact_update.artifact.artifact_id.is_empty());
    }
}
