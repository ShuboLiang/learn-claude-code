//! 自定义工具扩展示例
//!
//! 这里演示如何在不修改 rust-agent-core 的前提下，为 A2A Agent 添加业务专属工具。
//! 实际使用时，可把此模块移入独立的 `tools-custom` crate。

use async_trait::async_trait;
use rust_agent_core::AgentResult;
use rust_agent_core::ToolExtension;
use serde_json::{Value, json};

/// 天气查询工具扩展（示例）
pub struct WeatherToolExtension;

#[async_trait]
impl ToolExtension for WeatherToolExtension {
    fn schemas(&self) -> Vec<Value> {
        vec![json!({
            "name": "get_weather",
            "description": "查询指定城市的实时天气",
            "input_schema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string", "description": "城市名称，如 Beijing、Shanghai" }
                },
                "required": ["city"]
            }
        })]
    }

    fn can_handle(&self, name: &str) -> bool {
        name == "get_weather"
    }

    async fn dispatch(&self, name: &str, input: &Value) -> AgentResult<String> {
        match name {
            "get_weather" => {
                let city = input["city"].as_str().unwrap_or("未知城市");
                // 实际场景中这里调外部天气 API
                Ok(format!("{city} 今天晴，25°C，空气质量优。"))
            }
            other => Err(anyhow::anyhow!("未知工具: {other}")),
        }
    }
}
