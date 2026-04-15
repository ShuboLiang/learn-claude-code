use axum::response::sse::Event;
use rust_agent_core::agent::AgentEvent;
use serde_json::json;

/// 将 AgentEvent 转换为 SSE Event
pub fn agent_event_to_sse(event: AgentEvent) -> Event {
    match event {
        AgentEvent::TextDelta(content) => Event::default()
            .event("text_delta")
            .data(json!({ "content": content }).to_string()),
        AgentEvent::ToolCall { name, input, parallel_index } => {
            let mut data = json!({ "name": name, "input": input });
            if let Some((idx, total)) = parallel_index {
                data["parallel_index"] = json!({ "index": idx, "total": total });
            }
            Event::default().event("tool_call").data(data.to_string())
        }
        AgentEvent::ToolResult { name, output, parallel_index } => {
            let mut data = json!({ "name": name, "output": output });
            if let Some((idx, total)) = parallel_index {
                data["parallel_index"] = json!({ "index": idx, "total": total });
            }
            Event::default().event("tool_result").data(data.to_string())
        }
        AgentEvent::TurnEnd { api_calls } => Event::default()
            .event("turn_end")
            .data(json!({ "api_calls": api_calls }).to_string()),
        AgentEvent::Done => Event::default()
            .event("done")
            .data("{}"),
    }
}
