use axum::response::sse::Event;
use rust_agent_core::agent::AgentEvent;
use serde_json::json;

/// 将 AgentEvent 转换为 SSE Event
pub fn agent_event_to_sse(event: AgentEvent) -> Event {
    match event {
        AgentEvent::TextDelta { content, source } => Event::default()
            .event("text_delta")
            .data(json!({ "content": content, "source": source }).to_string()),
        AgentEvent::ThinkingDelta { content, source } => Event::default()
            .event("thinking_delta")
            .data(json!({ "content": content, "source": source }).to_string()),
        AgentEvent::ToolCall {
            id,
            name,
            input,
            parallel_index,
            source,
        } => {
            let mut data = json!({ "name": name, "input": input, "source": source });
            if let Some(id) = id {
                data["id"] = json!(id);
            }
            if let Some((idx, total)) = parallel_index {
                data["parallel_index"] = json!({ "index": idx, "total": total });
            }
            Event::default().event("tool_call").data(data.to_string())
        }
        AgentEvent::ToolResult {
            id,
            name,
            output,
            parallel_index,
            source,
        } => {
            let mut data = json!({ "name": name, "output": output, "source": source });
            if let Some(id) = id {
                data["id"] = json!(id);
            }
            if let Some((idx, total)) = parallel_index {
                data["parallel_index"] = json!({ "index": idx, "total": total });
            }
            Event::default().event("tool_result").data(data.to_string())
        }
        AgentEvent::TurnEnd {
            api_calls,
            token_usage,
            source,
        } => {
            let mut data = json!({ "api_calls": api_calls, "source": source });
            if let Some(usage) = token_usage {
                data["token_usage"] = json!({
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cache_read_tokens": usage.cache_read_tokens,
                    "cache_creation_tokens": usage.cache_creation_tokens,
                });
            }
            Event::default().event("turn_end").data(data.to_string())
        }
        AgentEvent::Done { source } => Event::default()
            .event("done")
            .data(json!({ "source": source }).to_string()),
        AgentEvent::Error {
            code,
            message,
            source,
        } => Event::default()
            .event("error")
            .data(json!({ "code": code, "message": message, "source": source }).to_string()),
        AgentEvent::Retrying {
            attempt,
            max_retries,
            wait_seconds,
            detail,
            source,
        } => Event::default().event("retrying").data(
            json!({
                "attempt": attempt,
                "max_retries": max_retries,
                "wait_seconds": wait_seconds,
                "detail": detail,
                "source": source,
            })
            .to_string(),
        ),
    }
}
