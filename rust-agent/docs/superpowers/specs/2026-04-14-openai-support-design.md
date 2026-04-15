# OpenAI 接口支持设计文档

## 目标

1. 让 Agent 可以调用 OpenAI 兼容的 API（如 Ollama、vLLM、DeepSeek、OpenAI 官方）作为 LLM 后端
2. 暴露 OpenAI Chat Completions 兼容的 HTTP API，供 Cursor、Continue 等工具调用
3. 通过环境变量 `LLM_PROVIDER=anthropic|openai` 切换后端

## 方案：Trait 抽象 + 两个独立客户端

### 核心抽象

```rust
pub trait LlmProvider: Send + Sync {
    async fn create_message(&self, request: &ProviderRequest) -> AgentResult<ProviderResponse>;
}

pub struct ProviderRequest<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub messages: &'a [ApiMessage],
    pub tools: &'a [Value],
    pub max_tokens: u32,
}

pub struct ProviderResponse {
    pub content: Vec<ResponseContentBlock>,
    pub stop_reason: String,
}
```

### OpenAI 客户端

- 认证：`Authorization: Bearer <api_key>`
- 端点：`POST {base_url}/v1/chat/completions`
- 内部处理 Anthropic ↔ OpenAI 格式转换

### 格式差异映射

| 概念 | Anthropic | OpenAI |
|------|-----------|--------|
| 系统提示词 | 顶层 `system` | messages 中 `role: "system"` |
| 工具调用 | `tool_use` 块 | `tool_calls` 数组 |
| 工具结果 | `tool_result` 块 | `role: "tool"` + `tool_call_id` |
| 停止原因 | `stop_reason: "tool_use"` | `finish_reason: "tool_calls"` |

### 环境变量

```
LLM_PROVIDER=anthropic|openai
ANTHROPIC_API_KEY=...
OPENAI_API_KEY=...
OPENAI_BASE_URL=...  # 可选，默认 https://api.openai.com
MODEL_ID=...         # 通用
```

### 文件变更

- `core/src/api/mod.rs` → 重构：LlmProvider trait + 工厂
- `core/src/api/anthropic.rs` → 从 mod.rs 拆出
- `core/src/api/openai.rs` → 新建
- `core/src/api/types.rs` → 添加 ProviderRequest/ProviderResponse
- `core/src/agent.rs` → 改用 `Box<dyn LlmProvider>`
- `core/src/infra/compact.rs` → 改用 `&dyn LlmProvider`
- `server/src/routes.rs` → 新增 /v1/chat/completions
- `server/src/openai_compat.rs` → 新建：OpenAI 兼容 API
