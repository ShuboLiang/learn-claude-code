use rust_agent_core::agent::AgentEvent;

use crate::types::{
    Artifact, FileContent, Message, Part, Role, TaskArtifactEvent, TaskMessageEvent,
    TaskStatus, TaskStatusEvent,
};

pub fn agent_event_to_a2a(
    task_id: &str,
    event: AgentEvent,
    artifact_counter: &mut u32,
) -> Vec<SsePayload> {
    match event {
        AgentEvent::TextDelta(text) => {
            vec![SsePayload::Message(TaskMessageEvent {
                id: task_id.to_string(),
                message: Message {
                    role: Role::Agent,
                    parts: vec![Part::Text { text }],
                },
                final_: None,
            })]
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
            vec![SsePayload::Message(TaskMessageEvent {
                id: task_id.to_string(),
                message: Message {
                    role: Role::Agent,
                    parts: vec![Part::Text { text }],
                },
                final_: None,
            })]
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

            let mut payloads = vec![SsePayload::Message(TaskMessageEvent {
                id: task_id.to_string(),
                message: Message {
                    role: Role::Agent,
                    parts: vec![Part::Text { text }],
                },
                final_: None,
            })];

            payloads.extend(detect_artifacts(task_id, &output, artifact_counter));
            payloads
        }
        AgentEvent::TurnEnd { .. } => {
            vec![SsePayload::Status(TaskStatusEvent {
                id: task_id.to_string(),
                status: TaskStatus::Working,
                final_: Some(false),
            })]
        }
        AgentEvent::Done => {
            vec![SsePayload::Status(TaskStatusEvent {
                id: task_id.to_string(),
                status: TaskStatus::Completed,
                final_: Some(true),
            })]
        }
    }
}

fn detect_artifacts(task_id: &str, output: &str, counter: &mut u32) -> Vec<SsePayload> {
    let mut payloads = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("<persisted-output") {
        let after = &rest[start..];
        if let Some(path_start) = after.find("path=\"") {
            let after_path = &after[path_start + 6..];
            if let Some(path_end) = after_path.find('"') {
                let path = &after_path[..path_end];
                let idx = *counter;
                *counter += 1;
                payloads.push(SsePayload::Artifact(TaskArtifactEvent {
                    id: task_id.to_string(),
                    artifact: Artifact {
                        name: Some(
                            std::path::Path::new(path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("output.txt")
                                .to_string(),
                        ),
                        description: Some("Persisted tool output".to_string()),
                        parts: vec![Part::File {
                            file: FileContent {
                                name: None,
                                mime_type: None,
                                bytes: None,
                                uri: Some(path.to_string()),
                            },
                        }],
                        metadata: None,
                        index: idx,
                        append: None,
                    },
                    final_: None,
                }));
                rest = &after_path[path_end..];
                continue;
            }
        }
        break;
    }
    payloads
}

#[derive(Debug, Clone)]
pub enum SsePayload {
    Status(TaskStatusEvent),
    Message(TaskMessageEvent),
    Artifact(TaskArtifactEvent),
}

impl SsePayload {
    pub fn into_sse_event(self) -> axum::response::sse::Event {
        match self {
            SsePayload::Status(ev) => axum::response::sse::Event::default()
                .event("task-status")
                .data(serde_json::to_string(&ev).unwrap()),
            SsePayload::Message(ev) => axum::response::sse::Event::default()
                .event("task-message")
                .data(serde_json::to_string(&ev).unwrap()),
            SsePayload::Artifact(ev) => axum::response::sse::Event::default()
                .event("task-artifact")
                .data(serde_json::to_string(&ev).unwrap()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_delta_becomes_message() {
        let mut c = 0;
        let payloads = agent_event_to_a2a("t1", AgentEvent::TextDelta("hello".into()), &mut c);
        assert_eq!(payloads.len(), 1);
        assert!(matches!(payloads[0], SsePayload::Message(_)));
    }

    #[test]
    fn detects_persisted_output_artifact() {
        let mut c = 0;
        let output = r#"Result:
<persisted-output path="/tmp/out.md">
Some content
"#;
        let payloads = agent_event_to_a2a(
            "t1",
            AgentEvent::ToolResult {
                name: "bash".into(),
                output: output.into(),
                parallel_index: None,
            },
            &mut c,
        );

        assert_eq!(payloads.len(), 2);
        assert!(matches!(payloads[1], SsePayload::Artifact(_)));
    }
}
