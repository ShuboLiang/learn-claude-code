use crate::types::{AgentCard, Capabilities, Skill};

pub fn build_agent_card(base_url: &str, tool_schemas: &[serde_json::Value]) -> AgentCard {
    let skills = tool_schemas.iter().map(schema_to_skill).collect();

    AgentCard {
        name: "rust-agent".to_string(),
        description: "A Rust-based programming assistant with tool execution capabilities."
            .to_string(),
        url: base_url.to_string(),
        provider: None,
        version: env!("CARGO_PKG_VERSION").to_string(),
        documentation_url: None,
        capabilities: Capabilities {
            streaming: true,
            push_notifications: Some(false),
            state_transition_history: Some(false),
            extensions: None,
        },
        authentication: None,
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills,
    }
}

fn schema_to_skill(schema: &serde_json::Value) -> Skill {
    let name = schema
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let id = if name == "task" {
        "delegate_task".to_string()
    } else {
        name.clone()
    };

    let description = schema
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Skill {
        id,
        name,
        description,
        tags: None,
        examples: None,
        input_modes: Some(vec!["text/plain".to_string()]),
        output_modes: Some(vec!["text/plain".to_string()]),
        input_schema: schema.get("input_schema").cloned(),
        output_schema: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_agent_card_maps_tools_to_skills() {
        let schemas = vec![
            serde_json::json!({
                "name": "bash",
                "description": "Run bash commands",
                "input_schema": {"type": "object"}
            }),
            serde_json::json!({
                "name": "task",
                "description": "Delegate to sub-agent",
                "input_schema": {"type": "object"}
            }),
        ];

        let card = build_agent_card("http://localhost:3001", &schemas);
        assert_eq!(card.skills.len(), 2);
        assert_eq!(card.skills[0].id, "bash");
        assert_eq!(
            card.skills[0].input_schema,
            Some(serde_json::json!({"type": "object"}))
        );
        assert_eq!(card.skills[1].id, "delegate_task"); // renamed
        assert!(card.capabilities.extensions.is_none());
    }
}
