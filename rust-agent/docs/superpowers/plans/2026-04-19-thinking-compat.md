# Thinking 兼容实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Anthropic 响应中的 `thinking` 内容块可以被成功反序列化，同时不影响最终文本展示和工具调用流程。

**Architecture:** 在 `ResponseContentBlock` 中显式建模 `Thinking` 变体，让反序列化层识别该内容块；消费层继续只处理 `Text` 与 `ToolUse`，从而把兼容性修复限制在最小范围。整个实现严格走 TDD：先写一个含 `thinking + tool_use` 的失败测试，确认当前失败，再做最小实现，最后验证测试通过且原有错误信息测试不受影响。

**Tech Stack:** Rust、serde、serde_json、tokio、cargo test

---

## 文件结构

- 修改：`crates/core/src/api/types.rs`
  - 为 `ResponseContentBlock` 新增 `Thinking` 变体
  - 更新 `ProviderResponse::final_text()` 的匹配逻辑，显式忽略 `Thinking`

- 修改：`crates/core/src/api/anthropic.rs`
  - 新增解析 `thinking + tool_use` 组合响应的单元测试
  - 保留并复用现有 `parse_messages_response` 测试辅助函数

## 任务 1：为 thinking 兼容补失败测试

**Files:**
- Modify: `crates/core/src/api/anthropic.rs:279-330`
- Test: `crates/core/src/api/anthropic.rs:279-330`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn parse_messages_response_should_accept_thinking_blocks_without_exposing_them_in_final_text() {
    let body = r#"
{
  "content": [
    {
      "type": "thinking",
      "thinking": "这是内部思考"
    },
    {
      "type": "tool_use",
      "id": "call_123",
      "name": "todo",
      "input": {"items": []}
    }
  ],
  "stop_reason": "tool_use"
}
"#;

    let response = parse_messages_response(body).expect("含 thinking 的响应应能被解析");
    let provider_response = ProviderResponse {
        content: response.content,
        stop_reason: "tool_calls".to_owned(),
    };

    assert_eq!(provider_response.final_text(), "");
    assert!(matches!(
        provider_response.content.get(1),
        Some(ResponseContentBlock::ToolUse { name, .. }) if name == "todo"
    ));
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run: `cargo test -p rust-agent-core parse_messages_response_should_accept_thinking_blocks_without_exposing_them_in_final_text -- --nocapture`
Expected: FAIL，并出现 `unknown variant 'thinking'` 或等价的反序列化失败信息

- [ ] **Step 3: 提交失败测试**

```bash
git add crates/core/src/api/anthropic.rs
git commit -m "test: cover thinking blocks in anthropic response"
```

## 任务 2：最小实现 thinking 反序列化兼容

**Files:**
- Modify: `crates/core/src/api/types.rs:77-132`
- Test: `crates/core/src/api/anthropic.rs:279-330`

- [ ] **Step 1: 在响应块枚举中新增 Thinking 变体**

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContentBlock {
    /// 普通文本内容
    Text {
        /// 文本内容
        text: String,
    },
    /// 思考内容块，仅用于协议兼容，不直接展示给用户
    Thinking {
        /// 模型返回的思考内容
        thinking: String,
    },
    /// 工具调用请求：Claude 想要调用某个工具
    ToolUse {
        /// 本次工具调用的唯一标识（用于将结果回传给正确的调用）
        id: String,
        /// 要调用的工具名称
        name: String,
        /// 传给工具的参数（JSON 对象）
        input: Value,
    },
}
```

- [ ] **Step 2: 更新 final_text 逻辑，显式忽略 Thinking**

```rust
pub fn final_text(&self) -> String {
    self.content
        .iter()
        .filter_map(|block| match block {
            ResponseContentBlock::Text { text } => Some(text.as_str()),
            ResponseContentBlock::Thinking { .. } => None,
            ResponseContentBlock::ToolUse { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
```

- [ ] **Step 3: 运行新增测试，确认通过**

Run: `cargo test -p rust-agent-core parse_messages_response_should_accept_thinking_blocks_without_exposing_them_in_final_text -- --nocapture`
Expected: PASS

- [ ] **Step 4: 提交最小实现**

```bash
git add crates/core/src/api/types.rs crates/core/src/api/anthropic.rs
git commit -m "fix: support thinking blocks in anthropic responses"
```

## 任务 3：回归验证现有行为未受影响

**Files:**
- Modify: 无
- Test: `crates/core/src/api/anthropic.rs:287-300`

- [ ] **Step 1: 运行原有错误信息测试**

Run: `cargo test -p rust-agent-core parse_messages_response_should_include_body_when_json_is_invalid -- --nocapture`
Expected: PASS

- [ ] **Step 2: 运行两个 Anthropic 相关单元测试**

Run: `cargo test -p rust-agent-core parse_messages_response_should_ -- --nocapture`
Expected: 看到两个 `parse_messages_response_should_...` 测试均为 PASS

- [ ] **Step 3: 记录验证结论并提交**

```bash
git add crates/core/src/api/types.rs crates/core/src/api/anthropic.rs
git commit -m "test: verify thinking compatibility regression coverage"
```

## 自检

- **Spec 覆盖**
  - 已覆盖 `thinking` 可解析：任务 1、任务 2
  - 已覆盖 `thinking` 不参与 `final_text()`：任务 1、任务 2
  - 已覆盖 `tool_use` 仍保留：任务 1
  - 未包含展示 thinking、OpenAI 兼容、通用未知块兜底，符合 spec 边界

- **占位符检查**
  - 无 `TODO`、`TBD`、"适当处理" 之类占位描述
  - 每个改动步骤都给出实际代码或精确命令

- **类型一致性检查**
  - 新测试使用 `ProviderResponse`、`ResponseContentBlock`、`parse_messages_response`，均与现有代码一致
  - `Thinking { thinking: String }` 与 spec 中确认的字段命名一致
