use axum::response::sse::Event;
use rust_agent_core::agent::AgentEvent;
use serde_json::json;

/// 将 AgentEvent 转换为 SSE Event
pub fn agent_event_to_sse(event: AgentEvent) -> Event {
    match event {
        AgentEvent::TextDelta(content) => Event::default()
            .event("text_delta")
            .data(json!({ "content": content }).to_string()),
        AgentEvent::ToolCall { name, input } => Event::default()
            .event("tool_call")
            .data(json!({ "name": name, "input": input }).to_string()),
        AgentEvent::ToolResult { name, output } => Event::default()
            .event("tool_result")
            .data(json!({ "name": name, "output": output }).to_string()),
        AgentEvent::TurnEnd => Event::default()
            .event("turn_end")
            .data("{}"),
        AgentEvent::Done => Event::default()
            .event("done")
            .data("{}"),
    }
}
