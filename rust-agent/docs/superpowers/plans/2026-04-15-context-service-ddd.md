# 上下文服务 DDD 重构 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将散落在 `agent.rs` 和入口端的对话历史管理、压缩策略、命令解析统一归入 `context/` 和 `command/` 域，`AgentApp` 简化为编排层。

**Architecture:** 新建 `context/` 域（Conversation + CompactStrategy + ContextService）封装对话历史和压缩逻辑；新建 `command/` 域（CommandDispatcher + handlers）统一命令解析执行。`AgentApp` 不再持有 `Vec<ApiMessage>`，改为持有 `ContextService`。CLI 和 Server 的 REPL 循环使用 `CommandDispatcher` 处理 `/clear`、`/compact`、`/stats` 等指令。`handle_user_turn` 签名去掉 `history` 参数，由 `ContextService` 内部管理。

**与设计文档的偏差说明：** 设计文档定义 `CommandDispatcher<'a>` 带生命周期参数，实际实现采用无状态 `CommandDispatcher` + 独立函数，更简洁且避免了生命周期复杂性。`/compact` 命令需要 `&LlmProvider`，但 `AgentApp.client` 是私有字段，因此 CLI 端通过在 `AgentApp` 上暴露 `pub fn client(&self) -> &LlmProvider` 方法来解决。

**Tech Stack:** Rust, tokio (已有), anyhow (已有)

---

## 文件变更清单

| 文件 | 操作 | 职责 |
|------|------|------|
| `crates/core/src/context/mod.rs` | 新建 | ContextService — 上下文域统一入口 |
| `crates/core/src/context/history.rs` | 新建 | Conversation — 对话历史封装 |
| `crates/core/src/context/compact.rs` | 新建 | 压缩策略迁移（micro_compact + auto_compact） |
| `crates/core/src/context/types.rs` | 新建 | ContextStats、CompactionResult |
| `crates/core/src/command/mod.rs` | 新建 | CommandDispatcher — 指令解析与分发 |
| `crates/core/src/command/handlers.rs` | 新建 | 各指令处理器实现 |
| `crates/core/src/agent.rs` | 修改 | 编排层：使用 ContextService，去掉 history 参数，暴露 `client()` 方法 |
| `crates/core/src/infra/compact.rs` | 修改 | 改为 re-export（向后兼容），原测试迁移到 context/compact.rs |
| `crates/core/src/lib.rs` | 修改 | 导出 context、command 模块 |
| `crates/cli/src/main.rs` | 修改 | 使用 CommandDispatcher + ContextService |
| `crates/server/src/session.rs` | 修改 | Session 持有 ContextService 替代 Vec<ApiMessage> |
| `crates/server/src/routes.rs` | 修改 | 适配新 API |
| `crates/server/src/openai_compat.rs` | 修改 | `Vec::new()` 改为 `ContextService::new()` |

---

### Task 1: 创建 context/history.rs — 对话历史封装

**Files:**
- Create: `crates/core/src/context/history.rs`

- [ ] **Step 1: 创建 `crates/core/src/context/` 目录**

Run: `mkdir -p crates/core/src/context`

- [ ] **Step 2: 编写 `history.rs`**

```rust
//! 对话历史管理，封装 Vec<ApiMessage> 的所有操作

use crate::api::types::ApiMessage;
use serde_json::Value;

/// 对话历史，封装消息列表的所有操作
#[derive(Clone, Debug)]
pub struct Conversation {
    messages: Vec<ApiMessage>,
}

impl Conversation {
    /// 创建空的对话历史
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// 追加一条消息
    pub fn push(&mut self, msg: ApiMessage) {
        self.messages.push(msg);
    }

    /// 清空所有消息，返回被清除的消息数量
    pub fn clear(&mut self) -> usize {
        let count = self.messages.len();
        self.messages.clear();
        count
    }

    /// 保留最后 N 条消息，截断前面的
    pub fn truncate(&mut self, keep_last: usize) {
        if self.messages.len() > keep_last {
            let drain_count = self.messages.len() - keep_last;
            self.messages.drain(0..drain_count);
        }
    }

    /// 获取消息数量
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// 粗略估算 token 数（约 4 字符/token）
    pub fn estimate_tokens(&self) -> usize {
        let json = serde_json::to_string(&self.messages).unwrap_or_default();
        json.len() / 4
    }

    /// 获取消息的不可变引用
    pub fn messages(&self) -> &[ApiMessage] {
        &self.messages
    }

    /// 获取消息的可变引用（供 run_agent_loop 直接操作）
    pub fn messages_mut(&mut self) -> &mut Vec<ApiMessage> {
        &mut self.messages
    }

    /// 替换所有消息（用于 auto_compact 后）
    pub fn replace(&mut self, new_messages: Vec<ApiMessage>) {
        self.messages = new_messages;
    }

    /// 添加一条纯文本用户消息
    pub fn push_user_text(&mut self, text: &str) {
        self.push(ApiMessage::user_text(text));
    }

    /// 添加一条包含内容块的用户消息（工具结果等）
    pub fn push_user_blocks(&mut self, blocks: Vec<Value>) {
        self.push(ApiMessage::user_blocks(blocks));
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}
```

---

### Task 2: 创建 context/types.rs — 域内类型

**Files:**
- Create: `crates/core/src/context/types.rs`

- [ ] **Step 1: 编写 `types.rs`**

```rust
//! 上下文域的类型定义

/// 上下文统计信息
#[derive(Clone, Debug)]
pub struct ContextStats {
    /// 当前消息数量
    pub message_count: usize,
    /// 粗略估算的 token 数
    pub estimated_tokens: usize,
    /// 清空时的消息数（仅 clear 操作时有值）
    pub cleared_count: Option<usize>,
}

/// 压缩结果
#[derive(Clone, Debug)]
pub struct CompactionResult {
    /// 是否执行了压缩
    pub compacted: bool,
    /// 使用的压缩策略："micro" | "auto" | "manual"
    pub strategy: String,
    /// 压缩前的消息数
    pub messages_before: usize,
    /// 压缩后的消息数
    pub messages_after: usize,
}
```

---

### Task 3: 创建 context/compact.rs — 压缩策略迁移

**Files:**
- Create: `crates/core/src/context/compact.rs`
- Reference: `crates/core/src/infra/compact.rs`（现有逻辑，完整迁移）

- [ ] **Step 1: 编写 `context/compact.rs`**

将 `infra/compact.rs` 中的函数迁移到新文件，调整为使用 `Conversation` 类型。
注意：移除 `run_auto_compact` 函数（agent.rs 中是分开调用 micro_compact 和 auto_compact 的，不需要组合函数）。

```rust
//! 三层上下文压缩策略
//!
//! - 第一层（micro_compact）：将旧的工具结果替换为占位符
//! - 第二层（auto_compact）：token 超阈值时，保存完整对话并生成摘要
//! - 第三层（manual_compact）：AI 主动调用 compact 工具触发

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde_json::Value;

use super::history::Conversation;
use crate::api::types::{ApiMessage, ProviderRequest};
use crate::AgentResult;

/// auto_compact 触发的 token 估算阈值
pub const TOKEN_THRESHOLD: usize = 50_000;

/// micro_compact 保留的最近工具结果数量
const KEEP_RECENT: usize = 5;

/// 压缩后保留的摘要长度（字符数）
const SUMMARY_LEN: usize = 300;

/// transcript 保存目录名
const TRANSCRIPT_DIR_NAME: &str = ".transcripts";

/// 需要保留完整结果的工具名称
const PRESERVE_RESULT_TOOLS: &[&str] = &["read_file"];

/// 向后兼容的独立函数：估算 token 数（约 4 字符/token）
///
/// 保留此函数以支持 `infra::compact::estimate_tokens` 的 re-export
pub fn estimate_tokens(messages: &[ApiMessage]) -> usize {
    let json = serde_json::to_string(messages).unwrap_or_default();
    json.len() / 4
}

/// 按行截断文本，累计不超过 max_chars 字符
fn truncate_by_lines(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for line in text.lines() {
        let line_len = line.len() + 1;
        if result.len() + line_len > max_chars {
            break;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
    }
    if result.len() < text.len() {
        result.push_str("\n...");
    }
    result
}

/// 第一层压缩：将旧的工具结果替换为简短的占位符
pub fn micro_compact(conv: &mut Conversation) {
    let messages = conv.messages_mut();

    let mut tool_results: Vec<(usize, usize)> = Vec::new();
    for (msg_idx, msg) in messages.iter().enumerate() {
        if msg.role != "user" {
            continue;
        }
        if let Value::Array(ref parts) = msg.content {
            for (part_idx, part) in parts.iter().enumerate() {
                if part.get("type").and_then(Value::as_str) == Some("tool_result") {
                    tool_results.push((msg_idx, part_idx));
                }
            }
        }
    }

    if tool_results.len() <= KEEP_RECENT {
        return;
    }

    let mut tool_name_map: HashMap<String, String> = HashMap::new();
    for msg in messages.iter() {
        if msg.role != "assistant" {
            continue;
        }
        if let Value::Array(ref blocks) = msg.content {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let (Some(id), Some(name)) = (
                        block.get("id").and_then(Value::as_str),
                        block.get("name").and_then(Value::as_str),
                    ) {
                        tool_name_map.insert(id.to_owned(), name.to_owned());
                    }
                }
            }
        }
    }

    let to_clear = &tool_results[..tool_results.len() - KEEP_RECENT];
    for &(msg_idx, part_idx) in to_clear {
        if let Value::Array(ref mut parts) = messages[msg_idx].content {
            if let Some(part) = parts.get_mut(part_idx) {
                if let Some(content) = part.get("content").and_then(Value::as_str) {
                    if content.len() <= 100 {
                        continue;
                    }
                }

                let tool_id = part
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let tool_name = tool_name_map
                    .get(tool_id)
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");

                if PRESERVE_RESULT_TOOLS.contains(&tool_name) {
                    continue;
                }

                let original = part
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let summary = truncate_by_lines(original, SUMMARY_LEN);
                let original_chars = original.chars().count();
                part["content"] = Value::String(format!(
                    "[已压缩: {tool_name}, 原文 {original_chars} 字符]\n{summary}"
                ));
            }
        }
    }
}

/// 第二层/第三层压缩：保存完整对话到磁盘，调用 LLM 生成摘要
pub async fn auto_compact(
    client: &crate::api::LlmProvider,
    model: &str,
    quotas: &[crate::infra::usage::QuotaRule],
    workspace_root: &Path,
    conv: &Conversation,
) -> AgentResult<Vec<ApiMessage>> {
    let transcript_dir = workspace_root.join(TRANSCRIPT_DIR_NAME);
    std::fs::create_dir_all(&transcript_dir)
        .with_context(|| format!("创建 {} 目录失败", TRANSCRIPT_DIR_NAME))?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let transcript_path = transcript_dir.join(format!("transcript_{timestamp}.jsonl"));

    {
        let mut file = std::fs::File::create(&transcript_path)
            .with_context(|| format!("创建 transcript 文件失败: {}", transcript_path.display()))?;
        for msg in conv.messages() {
            let line = serde_json::to_string(msg)?;
            writeln!(file, "{line}")?;
        }
    }
    println!("[transcript 已保存: {}]", transcript_path.display());

    let conversation_text = serde_json::to_string(conv.messages())?;
    let truncated: &str = if conversation_text.len() > 80_000 {
        &conversation_text[conversation_text.len() - 80_000..]
    } else {
        &conversation_text
    };

    let summary_messages = vec![ApiMessage::user_text(format!(
        "请总结这段对话，以便后续继续工作。包括：\n\
         1) 完成了什么\n\
         2) 当前状态\n\
         3) 做了哪些关键决策\n\n\
         请简洁但保留关键细节。\n\n{truncated}"
    ))];

    let request = ProviderRequest {
        model,
        system: "你是一个对话摘要助手。请简洁地总结对话内容。",
        messages: &summary_messages,
        tools: &[],
        max_tokens: 2000,
    };

    let response = client.create_message(&request, quotas).await?;
    let summary = response.final_text();

    Ok(vec![ApiMessage::user_text(format!(
        "[对话已压缩。完整记录: {}]\n\n{summary}",
        transcript_path.display()
    ))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn make_assistant_with_tool_use(id: &str, name: &str) -> ApiMessage {
        ApiMessage {
            role: "assistant".to_owned(),
            content: json!([
                { "type": "tool_use", "id": id, "name": name, "input": {} }
            ]),
        }
    }

    fn make_user_with_tool_result(tool_use_id: &str, content: &str) -> ApiMessage {
        ApiMessage {
            role: "user".to_owned(),
            content: json!([
                { "type": "tool_result", "tool_use_id": tool_use_id, "content": content }
            ]),
        }
    }

    #[test]
    fn micro_compact_preserves_recent_results() {
        let mut conv = Conversation::new();
        conv.push(make_assistant_with_tool_use("id1", "bash"));
        conv.push(make_user_with_tool_result("id1", &"x".repeat(200)));
        conv.push(make_assistant_with_tool_use("id2", "bash"));
        conv.push(make_user_with_tool_result("id2", &"y".repeat(200)));

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            assert!(parts[0].get("content").unwrap().as_str().unwrap().starts_with("x"));
        }
        if let Value::Array(ref parts) = msgs[3].content {
            assert!(parts[0].get("content").unwrap().as_str().unwrap().starts_with("y"));
        }
    }

    #[test]
    fn micro_compact_replaces_old_results() {
        let mut conv = Conversation::new();
        for (id, ch) in [("id1", "a"), ("id2", "b"), ("id3", "c"), ("id4", "d"), ("id5", "e"), ("id6", "f"), ("id7", "g")] {
            conv.push(make_assistant_with_tool_use(id, "bash"));
            conv.push(make_user_with_tool_result(id, &ch.repeat(200)));
        }

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert!(content.contains("[已压缩: bash"));
        }
        if let Value::Array(ref parts) = msgs[5].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert!(content.starts_with("c"));
        }
    }

    #[test]
    fn micro_compact_preserves_read_file_results() {
        let mut conv = Conversation::new();
        conv.push(make_assistant_with_tool_use("id1", "read_file"));
        conv.push(make_user_with_tool_result("id1", &"file content here".repeat(20)));
        for (id, ch) in [("id2", "o"), ("id3", "o"), ("id4", "o"), ("id5", "o")] {
            conv.push(make_assistant_with_tool_use(id, "bash"));
            conv.push(make_user_with_tool_result(id, &ch.repeat(30)));
        }

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert!(content.starts_with("file content here"));
        }
    }

    #[test]
    fn micro_compact_skips_short_content() {
        let mut conv = Conversation::new();
        conv.push(make_assistant_with_tool_use("id1", "bash"));
        conv.push(make_user_with_tool_result("id1", "short"));
        for (id, ch) in [("id2", "x"), ("id3", "x"), ("id4", "x"), ("id5", "x")] {
            conv.push(make_assistant_with_tool_use(id, "bash"));
            conv.push(make_user_with_tool_result(id, &ch.repeat(200)));
        }

        micro_compact(&mut conv);

        let msgs = conv.messages();
        if let Value::Array(ref parts) = msgs[1].content {
            let content = parts[0].get("content").unwrap().as_str().unwrap();
            assert_eq!(content, "short");
        }
    }

    #[test]
    fn conversation_estimate_tokens() {
        let mut conv = Conversation::new();
        conv.push_user_text("hello world");
        let tokens = conv.estimate_tokens();
        assert!(tokens > 0 && tokens < 100);
    }

    #[test]
    fn standalone_estimate_tokens() {
        let messages = vec![ApiMessage::user_text("hello world")];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0 && tokens < 100);
    }

    #[test]
    fn conversation_clear() {
        let mut conv = Conversation::new();
        conv.push_user_text("msg1");
        conv.push_user_text("msg2");
        conv.push_user_text("msg3");
        let cleared = conv.clear();
        assert_eq!(cleared, 3);
        assert!(conv.is_empty());
    }
}
```

---

### Task 4: 创建 context/mod.rs — ContextService 统一入口

**Files:**
- Create: `crates/core/src/context/mod.rs`

- [ ] **Step 1: 编写 `mod.rs`**

```rust
//! 上下文域：对话历史管理 + 压缩策略
//!
//! 对外暴露 ContextService 作为统一入口，封装 Conversation（历史）和压缩逻辑。

pub mod compact;
pub mod history;
pub mod types;

use std::path::Path;

use crate::AgentResult;
use crate::api::types::ApiMessage;

pub use history::Conversation;
pub use types::ContextStats;

/// 上下文服务：管理对话历史和压缩策略
#[derive(Clone, Debug)]
pub struct ContextService {
    conversation: Conversation,
}

impl ContextService {
    /// 创建空的上下文服务
    pub fn new() -> Self {
        Self {
            conversation: Conversation::new(),
        }
    }

    // ── 读取操作 ──

    /// 获取消息的不可变引用
    pub fn messages(&self) -> &[ApiMessage] {
        self.conversation.messages()
    }

    /// 获取消息的可变引用（供 agent loop 直接操作）
    pub fn messages_mut(&mut self) -> &mut Vec<ApiMessage> {
        self.conversation.messages_mut()
    }

    /// 获取上下文统计信息
    pub fn stats(&self) -> ContextStats {
        ContextStats {
            message_count: self.conversation.len(),
            estimated_tokens: self.conversation.estimate_tokens(),
            cleared_count: None,
        }
    }

    /// 获取消息数量
    pub fn len(&self) -> usize {
        self.conversation.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.conversation.is_empty()
    }

    // ── 写入操作 ──

    /// 追加一条消息
    pub fn push(&mut self, msg: ApiMessage) {
        self.conversation.push(msg);
    }

    /// 添加纯文本用户消息
    pub fn push_user_text(&mut self, text: &str) {
        self.conversation.push_user_text(text);
    }

    /// 添加包含内容块的用户消息
    pub fn push_user_blocks(&mut self, blocks: Vec<serde_json::Value>) {
        self.conversation.push_user_blocks(blocks);
    }

    /// 清空对话历史，返回统计信息
    pub fn clear(&mut self) -> ContextStats {
        let cleared = self.conversation.clear();
        ContextStats {
            message_count: 0,
            estimated_tokens: 0,
            cleared_count: Some(cleared),
        }
    }

    /// 替换所有消息（用于 auto_compact 后）
    pub fn replace(&mut self, new_messages: Vec<ApiMessage>) {
        self.conversation.replace(new_messages);
    }

    /// 粗略估算 token 数
    pub fn estimate_tokens(&self) -> usize {
        self.conversation.estimate_tokens()
    }

    // ── 压缩操作 ──

    /// 执行 micro_compact（原地修改）
    pub fn micro_compact(&mut self) {
        compact::micro_compact(&mut self.conversation);
    }

    /// 执行 auto_compact（异步，需要 LLM），返回压缩后的新消息列表
    pub async fn auto_compact(
        &self,
        client: &crate::api::LlmProvider,
        model: &str,
        quotas: &[crate::infra::usage::QuotaRule],
        workspace_root: &Path,
    ) -> AgentResult<Vec<ApiMessage>> {
        compact::auto_compact(client, model, quotas, workspace_root, &self.conversation).await
    }
}

impl Default for ContextService {
    fn default() -> Self {
        Self::new()
    }
}
```

---

### Task 5: 创建 command/ 模块 — 命令解析与分发

**Files:**
- Create: `crates/core/src/command/mod.rs`
- Create: `crates/core/src/command/handlers.rs`

- [ ] **Step 1: 编写 `command/mod.rs`**

```rust
//! 命令域：解析和执行用户指令（/clear、/compact、/stats、/help、/quit）

mod handlers;

use crate::context::ContextService;

/// 用户指令枚举
#[derive(Clone, Debug)]
pub enum UserCommand {
    /// 清空对话历史
    Clear,
    /// 手动触发压缩
    Compact,
    /// 显示上下文统计
    Stats,
    /// 显示帮助信息
    Help,
    /// 退出程序
    Quit,
}

/// 命令执行结果
#[derive(Clone, Debug)]
pub struct CommandResult {
    /// 给用户的反馈文本
    pub output: String,
    /// 是否退出程序
    pub should_quit: bool,
}

/// 命令分发器（无状态，通过方法参数传入依赖）
pub struct CommandDispatcher;

impl CommandDispatcher {
    /// 解析用户输入，匹配 /command 格式
    ///
    /// 返回 None 表示不是命令（普通对话输入）
    pub fn parse(input: &str) -> Option<UserCommand> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        match trimmed.to_lowercase().as_str() {
            "/clear" => Some(UserCommand::Clear),
            "/compact" => Some(UserCommand::Compact),
            "/stats" => Some(UserCommand::Stats),
            "/help" | "/?" | "/h" => Some(UserCommand::Help),
            "/quit" | "/exit" | "/q" => Some(UserCommand::Quit),
            _ => None,
        }
    }

    /// 执行命令，返回结果
    ///
    /// `client` 为 None 时，/compact 命令将返回"不可用"提示
    pub async fn execute(
        cmd: UserCommand,
        ctx: &mut ContextService,
        client: Option<&crate::api::LlmProvider>,
        model: &str,
        quotas: &[crate::infra::usage::QuotaRule],
        workspace_root: &std::path::Path,
    ) -> CommandResult {
        handlers::handle(cmd, ctx, client, model, quotas, workspace_root).await
    }
}
```

- [ ] **Step 2: 编写 `command/handlers.rs`**

```rust
//! 命令处理器实现

use super::{CommandResult, UserCommand};
use crate::context::ContextService;

pub(super) async fn handle(
    cmd: UserCommand,
    ctx: &mut ContextService,
    client: Option<&crate::api::LlmProvider>,
    model: &str,
    quotas: &[crate::infra::usage::QuotaRule],
    workspace_root: &std::path::Path,
) -> CommandResult {
    match cmd {
        UserCommand::Clear => handle_clear(ctx),
        UserCommand::Compact => handle_compact(ctx, client, model, quotas, workspace_root).await,
        UserCommand::Stats => handle_stats(ctx),
        UserCommand::Help => handle_help(),
        UserCommand::Quit => CommandResult {
            output: String::new(),
            should_quit: true,
        },
    }
}

fn handle_clear(ctx: &mut ContextService) -> CommandResult {
    let stats = ctx.clear();
    let cleared = stats.cleared_count.unwrap_or(0);
    CommandResult {
        output: format!("上下文已清空（清除 {cleared} 条消息）"),
        should_quit: false,
    }
}

async fn handle_compact(
    ctx: &mut ContextService,
    client: Option<&crate::api::LlmProvider>,
    model: &str,
    quotas: &[crate::infra::usage::QuotaRule],
    workspace_root: &std::path::Path,
) -> CommandResult {
    let Some(client) = client else {
        return CommandResult {
            output: "压缩功能不可用（缺少 LLM 客户端）".to_owned(),
            should_quit: false,
        };
    };

    match ctx.auto_compact(client, model, quotas, workspace_root).await {
        Ok(new_messages) => {
            let before = ctx.len();
            ctx.replace(new_messages);
            let after = ctx.len();
            CommandResult {
                output: format!("压缩完成（{before} 条 → {after} 条）"),
                should_quit: false,
            }
        }
        Err(e) => CommandResult {
            output: format!("压缩失败: {e}"),
            should_quit: false,
        },
    }
}

fn handle_stats(ctx: &ContextService) -> CommandResult {
    let stats = ctx.stats();
    CommandResult {
        output: format!("消息数: {} | 预估 token: {}", stats.message_count, stats.estimated_tokens),
        should_quit: false,
    }
}

fn handle_help() -> CommandResult {
    CommandResult {
        output: concat!(
            "可用命令：\n",
            "  /clear    清空对话历史\n",
            "  /compact  手动压缩上下文\n",
            "  /stats    显示上下文统计\n",
            "  /help     显示此帮助\n",
            "  /quit     退出程序",
        )
        .to_owned(),
        should_quit: false,
    }
}
```

---

### Task 6: 更新 lib.rs — 导出新模块 + 编译验证

**Files:**
- Modify: `crates/core/src/lib.rs`

- [ ] **Step 1: 添加模块声明和导出**

```rust
pub mod agent;
pub mod api;
pub mod command;
pub mod context;
pub mod infra;
pub mod skills;
pub mod tools;

/// 统一的 Agent 结果类型别名，简化错误处理
pub type AgentResult<T> = anyhow::Result<T>;

/// 重新导出 tokio mpsc channel，供 CLI 和 server 使用
pub use tokio::sync::mpsc;

// ── 公共 API 统一导出 ──
pub use agent::{AgentApp, AgentEvent};
pub use api::{LlmProvider, ProviderInfo};
pub use api::types::{ApiMessage, ProviderRequest, ProviderResponse, ResponseContentBlock};
pub use skills::SkillLoader;
pub use infra::todo::TodoManager;
pub use context::ContextService;
pub use command::{CommandDispatcher, CommandResult, UserCommand};
```

- [ ] **Step 2: 编译验证**

Run: `cargo check 2>&1 | head -30`
Expected: 编译通过（新模块独立，不影响现有代码）

- [ ] **Step 3: 运行新模块测试**

Run: `cargo test -p rust-agent-core context 2>&1 | tail -20`
Expected: 所有 7 个 context 模块测试通过（6 个 compact + 1 个 history）

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/context/ crates/core/src/command/ crates/core/src/lib.rs
git commit -m "refactor: 新建 context/ 和 command/ 域模块（DDD 重构第一阶段）"
```

---

### Task 7: 重构 agent.rs — 使用 ContextService + 暴露 client()

**Files:**
- Modify: `crates/core/src/agent.rs`（全面修改）

核心变更：
1. `handle_user_turn` 签名：`history: &mut Vec<ApiMessage>` → `ctx: &mut ContextService`
2. `run_agent_loop` 使用 `ContextService` 的方法
3. 压缩逻辑使用 `ctx.micro_compact()` 和 `ctx.auto_compact()`
4. 新增 `pub fn client(&self) -> &LlmProvider` 方法（供 CommandDispatcher::execute 调用 /compact）
5. `run_subagent` 创建独立的 `ContextService::new()` 作为子代理上下文

- [ ] **Step 1: 重写 `agent.rs`**

完整替换为以下内容（仅标注关键变更点）：

```rust
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Context;
use async_recursion::async_recursion;
use dotenvy::dotenv;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::AgentResult;
use crate::api::types::{ApiMessage, ProviderRequest, ResponseContentBlock};
use crate::context::compact;
use crate::context::ContextService;
use crate::infra::logging::ConversationLogger;
use crate::infra::storage;
use crate::infra::utils::preview_text;
use crate::skills::SkillLoader;
use crate::skills::hub as skillhub;
use crate::tools::AgentToolbox;

#[derive(Clone, Debug)]
pub enum AgentEvent {
    TextDelta(String),
    ToolCall {
        name: String,
        input: serde_json::Value,
        parallel_index: Option<(usize, usize)>,
    },
    ToolResult {
        name: String,
        output: String,
        parallel_index: Option<(usize, usize)>,
    },
    TurnEnd,
    Done,
}

const MAX_TOOL_ROUNDS: usize = 30;
const MAX_PARALLEL_TASKS: usize = 5;

#[derive(Clone, Debug)]
pub struct AgentApp {
    client: crate::api::LlmProvider,
    workspace_root: PathBuf,
    skills: Arc<RwLock<SkillLoader>>,
    skill_dirs: Vec<PathBuf>,
    model: String,
    max_tokens: u32,
    quotas: Vec<crate::infra::usage::QuotaRule>,
}

#[derive(Clone, Copy, Debug)]
struct AgentRunConfig {
    allow_task: bool,
    use_todo_reminder: bool,
}

impl AgentRunConfig {
    fn parent() -> Self {
        Self { allow_task: true, use_todo_reminder: true }
    }
    fn child() -> Self {
        Self { allow_task: false, use_todo_reminder: true }
    }
}

impl AgentApp {
    pub async fn from_env() -> AgentResult<Self> {
        let _ = dotenv();
        let workspace_root =
            std::env::current_dir().context("Failed to determine current directory")?;
        let info = crate::api::create_provider()?;
        let model = info.model;
        let max_tokens = info.max_tokens;

        let skillhub_available = skillhub::ensure_cli_installed().await;
        if skillhub_available {
            println!("SkillHub CLI 已就绪。");
        }

        let user_skills_dir = dirs::home_dir()
            .map(|p| p.join(".rust-agent").join("skills"))
            .unwrap_or_default();
        let skill_dirs = vec![user_skills_dir.clone(), workspace_root.join("skills")];
        let skills = SkillLoader::load_from_dirs(
            &skill_dirs.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        )?;

        Ok(Self {
            client: info.provider,
            workspace_root,
            skills: Arc::new(RwLock::new(skills)),
            skill_dirs,
            model,
            max_tokens,
            quotas: info.quotas,
        })
    }

    pub fn quotas(&self) -> &[crate::infra::usage::QuotaRule] {
        &self.quotas
    }

    /// 获取 LLM Provider 的引用（供 /compact 等命令使用）
    pub fn client(&self) -> &crate::api::LlmProvider {
        &self.client
    }

    /// 获取模型 ID（供 /compact 等命令使用）
    pub fn model(&self) -> &str {
        &self.model
    }

    /// 获取工作区根目录（供 /compact 等命令使用）
    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    pub fn list_skills(&self) -> Vec<crate::skills::SkillSummary> {
        self.skills.read().unwrap().list_skills()
    }

    /// 处理用户的一次对话输入
    ///
    /// 变更：`history: &mut Vec<ApiMessage>` → `ctx: &mut ContextService`
    pub async fn handle_user_turn(
        &self,
        ctx: &mut ContextService,
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> AgentResult<String> {
        let mut logger = ConversationLogger::create();

        ctx.push_user_text(user_input);
        logger.log(&format!("=== 用户 ===\n{user_input}"));

        let system_prompt = build_system_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
        );
        logger.log(&format!("=== 系统提示词 ===\n{system_prompt}"));

        let event_tx = Arc::new(event_tx);

        let result = self
            .run_agent_loop(ctx, system_prompt, AgentRunConfig::parent(), &mut logger, &event_tx)
            .await;

        match &result {
            Ok(text) => logger.log(&format!("=== 助手 ===\n{text}")),
            Err(e) => logger.log(&format!("=== 错误 ===\n{e}")),
        }

        result
    }

    #[async_recursion]
    async fn run_agent_loop(
        &self,
        ctx: &mut ContextService,
        system_prompt: String,
        config: AgentRunConfig,
        logger: &mut ConversationLogger,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    ) -> AgentResult<String> {
        let mut toolbox = AgentToolbox::new(
            self.workspace_root.clone(),
            Arc::clone(&self.skills),
            self.skill_dirs.clone(),
        );
        let mut rounds_since_todo = 0usize;
        let mut last_micro_compact = Instant::now();
        let micro_compact_interval = Duration::from_secs(60 * 60);

        for _ in 0..MAX_TOOL_ROUNDS {
            // 第一层：micro_compact
            if last_micro_compact.elapsed() >= micro_compact_interval {
                println!("[micro_compact 已触发]");
                ctx.micro_compact();
                last_micro_compact = Instant::now();
            }
            // 第二层：auto_compact
            if ctx.estimate_tokens() > compact::TOKEN_THRESHOLD {
                println!("[auto_compact 已触发]");
                match ctx.auto_compact(&self.client, &self.model, &self.quotas, &self.workspace_root).await {
                    Ok(new_messages) => ctx.replace(new_messages),
                    Err(e) => eprintln!("[auto_compact 失败: {e:#}]"),
                }
            }

            let tools = toolbox.tool_schemas(config.allow_task);
            let request = ProviderRequest {
                model: &self.model,
                system: &system_prompt,
                messages: ctx.messages(),
                tools: &tools,
                max_tokens: self.max_tokens,
            };
            let response = match self.client.create_message(&request, &self.quotas).await {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("[Agent] create_message 失败！错误: {e:#}");
                    return Err(e);
                }
            };
            let stop_reason = response.stop_reason.clone();
            ctx.push(ApiMessage::assistant_blocks(&response.content)?);

            if stop_reason != "tool_calls" {
                let _ = event_tx.send(AgentEvent::TurnEnd).await;
                return Ok(response.final_text());
            }

            let mut results = Vec::new();
            let mut used_todo = false;
            let mut manual_compact = false;

            struct ToolCallInfo {
                id: String,
                name: String,
                input: Value,
            }

            let tool_calls: Vec<ToolCallInfo> = response
                .content
                .iter()
                .filter_map(|block| {
                    if let ResponseContentBlock::ToolUse { id, name, input } = block {
                        Some(ToolCallInfo { id: id.clone(), name: name.clone(), input: input.clone() })
                    } else {
                        None
                    }
                })
                .collect();

            let mut task_calls: Vec<ToolCallInfo> = Vec::new();
            let mut other_calls: Vec<ToolCallInfo> = Vec::new();
            for tc in tool_calls {
                if tc.name == "task" {
                    task_calls.push(tc);
                } else {
                    other_calls.push(tc);
                }
            }

            // === 串行执行非 task 工具 ===
            for tc in &other_calls {
                let input_preview = preview_text(&tc.input.to_string(), 200);
                logger.log(&format!("=== 工具调用: {} ===\n输入: {input_preview}", tc.name));

                let output = if tc.name == "compact" {
                    manual_compact = true;
                    let _ = event_tx.send(AgentEvent::ToolCall { name: tc.name.clone(), input: tc.input.clone(), parallel_index: None }).await;
                    "正在压缩...".to_owned()
                } else {
                    match toolbox.dispatch(&tc.name, &tc.input).await {
                        Ok(dispatch) => {
                            used_todo |= dispatch.used_todo;
                            let _ = event_tx.send(AgentEvent::ToolCall { name: tc.name.clone(), input: tc.input.clone(), parallel_index: None }).await;
                            let _ = event_tx.send(AgentEvent::ToolResult { name: tc.name.clone(), output: preview_text(&dispatch.output, 200), parallel_index: None }).await;
                            dispatch.output
                        }
                        Err(e) => {
                            let msg = format!("Error: {e}");
                            let _ = event_tx.send(AgentEvent::ToolResult { name: tc.name.clone(), output: msg.clone(), parallel_index: None }).await;
                            msg
                        }
                    }
                };

                logger.log(&format!("=== 工具结果: {} ===\n{output}", tc.name));
                let processed_output = storage::maybe_persist(&tc.id, &output);
                results.push(tool_result_block(&tc.id, processed_output));
            }

            // === 并行执行 task 工具（subagent） ===
            if !task_calls.is_empty() {
                if !config.allow_task {
                    for tc in &task_calls {
                        results.push(tool_result_block(&tc.id, "错误：task 工具在子代理中不可用".to_owned()));
                    }
                } else {
                    let total = task_calls.len().min(MAX_PARALLEL_TASKS);
                    let actual_calls: Vec<_> = task_calls.into_iter().take(total).collect();
                    let is_parallel = actual_calls.len() > 1;

                    for (idx, tc) in actual_calls.iter().enumerate() {
                        let input_preview = preview_text(&tc.input.to_string(), 200);
                        logger.log(&format!("=== 工具调用: task (并行 {}/{}) ===\n输入: {input_preview}", idx + 1, actual_calls.len()));
                        let _ = event_tx.send(AgentEvent::ToolCall {
                            name: "task".to_owned(),
                            input: tc.input.clone(),
                            parallel_index: if is_parallel { Some((idx + 1, actual_calls.len())) } else { None },
                        }).await;
                    }

                    let mut handles = Vec::new();
                    for tc in &actual_calls {
                        let prompt = tc.input.get("prompt").and_then(Value::as_str).unwrap_or_default().to_owned();
                        let app = self.clone();
                        let event_tx = Arc::clone(event_tx);
                        handles.push(tokio::spawn(async move {
                            let mut sub_logger = ConversationLogger::create();
                            let result = app.run_subagent(prompt, &mut sub_logger, &event_tx).await;
                            (result, sub_logger)
                        }));
                    }

                    let mut sub_results: Vec<(String, ConversationLogger)> = Vec::new();
                    for handle in handles {
                        match handle.await {
                            Ok((Ok(output), sub_logger)) => sub_results.push((output, sub_logger)),
                            Ok((Err(e), sub_logger)) => sub_results.push((format!("子代理执行失败: {e}"), sub_logger)),
                            Err(e) => sub_results.push((format!("子代理任务异常: {e}"), ConversationLogger::create())),
                        }
                    }

                    for (idx, (output, _sub_logger)) in sub_results.iter().enumerate() {
                        let _ = event_tx.send(AgentEvent::ToolResult {
                            name: "task".to_owned(),
                            output: preview_text(output, 200),
                            parallel_index: if is_parallel { Some((idx + 1, actual_calls.len())) } else { None },
                        }).await;
                        logger.log(&format!("=== 工具结果: task (并行 {}/{}) ===\n{output}", idx + 1, actual_calls.len()));
                        let tc_id = &actual_calls[idx].id;
                        let processed = storage::maybe_persist(tc_id, output);
                        results.push(tool_result_block(tc_id, processed));
                    }
                }
            }

            rounds_since_todo = if used_todo { 0 } else { rounds_since_todo + 1 };
            if config.use_todo_reminder && rounds_since_todo >= 3 {
                results.push(json!({ "type": "text", "text": "<reminder>请更新你的待办事项。</reminder>" }));
            }

            ctx.push_user_blocks(results);

            // 第三层：手动压缩
            if manual_compact {
                println!("[手动压缩]");
                match ctx.auto_compact(&self.client, &self.model, &self.quotas, &self.workspace_root).await {
                    Ok(new_messages) => ctx.replace(new_messages),
                    Err(e) => {
                        eprintln!("[手动压缩失败: {e:#}]");
                        let _ = event_tx.send(AgentEvent::TurnEnd).await;
                        return Err(e);
                    }
                }
                let _ = event_tx.send(AgentEvent::TurnEnd).await;
                return Ok("对话已手动压缩。".to_owned());
            }
        }

        let _ = event_tx.send(AgentEvent::TurnEnd).await;
        Ok("已达到工具调用轮数安全上限，自动停止。".to_owned())
    }

    /// 启动子代理（使用独立的 ContextService）
    async fn run_subagent(
        &self,
        prompt: String,
        logger: &mut ConversationLogger,
        event_tx: &Arc<mpsc::Sender<AgentEvent>>,
    ) -> AgentResult<String> {
        let system_prompt = build_subagent_prompt(
            &self.workspace_root,
            &self.skills.read().unwrap().descriptions_for_system_prompt(),
        );
        logger.log(&format!("=== 子代理系统提示词 ===\n{system_prompt}"));
        let mut sub_ctx = ContextService::new();
        sub_ctx.push_user_text(&prompt);
        self.run_agent_loop(&mut sub_ctx, system_prompt, AgentRunConfig::child(), logger, event_tx).await
    }
}

fn tool_result_block(tool_use_id: &str, content: String) -> Value {
    json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content })
}

fn build_system_prompt(workspace_root: &std::path::Path, skills_desc: &str) -> String {
    let platform = if cfg!(windows) {
        "Windows (PowerShell)。使用 PowerShell 语法：用 Get-ChildItem 代替 ls，Get-Content 代替 cat，-Command 代替 -lc，; 代替 &&"
    } else {
        "Unix (bash)"
    };
    format!(
        "你是一个编程助手，工作目录：{}。\n平台：{platform}\n优先使用工具解决问题，避免冗长解释。\n\n\
        任务执行流程 — 每个任务必须按以下顺序执行：\n\
        0. 先了解项目：读取目录结构和关键文件，理解项目上下文。\n\
        1. 检查已安装的技能是否覆盖当前任务。如果有，调用 load_skill。\n\
        2. 如果没有匹配的已安装技能，必须调用 search_skillhub 搜索。\n\
        3. 如果 search_skillhub 返回了相关技能，调用 install_skill 安装它。\n\
        4. 只有在步骤 0-3 完成（且未找到技能）后，才能使用 bash 或其他工具执行具体操作。\n\
        5. 绝对不能跳过技能检查直接使用 bash/curl 等工具。\n\
        6. 在完成技能流程之前，绝对不能声称无法完成任务。\n\n\
        输出规则：\n\
        - 研究类任务：收集完资料后必须输出完整内容，不能只说整理完毕。\n\
        - 长篇内容（>500字）应写入文件并告知用户文件路径。\n\
        - 如果工具结果被持久化到磁盘（包含 <persisted-output> 标签），可以随时用 read_file 读取完整内容。\n\n\
        子代理并行执行规则：\n\
        - 你可以在一次响应中返回多个 task 工具调用来并行执行多个子代理。\n\
        - **并行执行条件**（需全部满足）：2+ 个独立任务、任务间无依赖、无共享文件冲突。\n\
        - **串行执行条件**（任一触发）：任务间有依赖、共享文件/状态、范围不明确需先了解。\n\
        - 典型并行场景：同时研究多个不相关主题、同时探索不同模块、同时分析多个文件。\n\
        - 典型串行场景：先调研再实现、先写 schema 再写 API、需要前一步结果才能决定下一步。\n\
        - 并行上限为 5 个子代理，超出部分将被忽略。\n\n\
        其他工具：\n\
        - 使用 todo 工具规划多步骤工作。\n\
        - 使用 task 工具委派子任务（子代理拥有独立上下文，支持并行）。\n\n\
        可用技能：\n{}",
        workspace_root.display(),
        skills_desc
    )
}

fn build_subagent_prompt(workspace_root: &std::path::Path, skills_desc: &str) -> String {
    format!(
        "你是一个编程子代理，工作目录：{}。\n完成给定任务，按需使用工具，然后返回简洁的摘要。不能调用 task 工具。\n\n\
        已安装的技能：\n{skills_desc}\n\n\
        如果已安装的技能覆盖当前任务，直接调用 load_skill 加载；否则跳过技能流程，直接执行。",
        workspace_root.display()
    )
}
```

---

### Task 8: 更新 infra/compact.rs — re-export 向后兼容

**Files:**
- Modify: `crates/core/src/infra/compact.rs`

- [ ] **Step 1: 替换为 re-export**

原文件中的函数已迁移到 `context/compact.rs`。`estimate_tokens` 独立函数保留在 `context/compact.rs` 中，通过 re-export 向后兼容。
原文件中的测试也已迁移到 `context/compact.rs`。

```rust
//! 向后兼容模块：所有压缩逻辑已迁移到 `context::compact`
//!
//! 此模块保留 re-export 以避免破坏外部依赖，后续版本将移除。

pub use crate::context::compact::*;
```

---

### Task 9: 重构 CLI — 使用 CommandDispatcher + ContextService

**Files:**
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: 重写 `cli/src/main.rs`**

关键变更：
- `Vec<ApiMessage>` → `ContextService`
- 使用 `CommandDispatcher::parse` + `CommandDispatcher::execute` 处理命令
- 通过 `app.client()` 获取 LLM Provider 引用传给 execute

```rust
use rustyline::error::ReadlineError;
use rustyline::{Cmd, Config, DefaultEditor, Event, KeyCode, KeyEvent, Modifiers};

use rust_agent_core::agent::{AgentApp, AgentEvent};
use rust_agent_core::command::{CommandDispatcher, UserCommand};
use rust_agent_core::context::ContextService;
use rust_agent_core::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = AgentApp::from_env().await?;

    rust_agent_core::infra::usage::UsageTracker::display_with_quotas(app.quotas());

    let mut ctx = ContextService::new();
    let config = Config::builder()
        .bracketed_paste(true)
        .build();
    let mut rl = DefaultEditor::with_config(config)?;

    rl.bind_sequence(
        Event::from(KeyEvent(KeyCode::Enter, Modifiers::NONE)),
        Cmd::AcceptLine,
    );
    rl.bind_sequence(
        Event::from(KeyEvent(KeyCode::Enter, Modifiers::CTRL)),
        Cmd::Newline,
    );

    loop {
        let line = match rl.readline("agent >> ") {
            Ok(line) => line,
            Err(ReadlineError::Eof | ReadlineError::Interrupted) => break,
            Err(e) => return Err(e.into()),
        };

        let query = line.trim();
        if query.is_empty() {
            continue;
        }
        if matches!(query, "q" | "quit" | "exit") {
            break;
        }

        // /skills 命令由 CLI 专有处理
        if query == "/skills" {
            rl.add_history_entry(query)?;
            let skills = app.list_skills();
            if skills.is_empty() {
                println!("（没有已安装的技能）");
            } else {
                println!("已安装的技能（{} 个）：", skills.len());
                for s in &skills {
                    let desc = if s.description.is_empty() { String::new() } else { format!(": {}", s.description) };
                    let tags = if s.tags.is_empty() { String::new() } else { format!(" [{}]", s.tags) };
                    println!("  - {}{desc}{tags}", s.name);
                }
            }
            println!();
            continue;
        }

        // 通用命令分发（通过 CommandDispatcher）
        if let Some(cmd) = CommandDispatcher::parse(query) {
            rl.add_history_entry(query)?;
            let result = CommandDispatcher::execute(
                cmd,
                &mut ctx,
                Some(app.client()),
                app.model(),
                app.quotas(),
                app.workspace_root(),
            ).await;
            if result.should_quit {
                break;
            }
            println!("{}\n", result.output);
            continue;
        }

        // 普通对话
        rl.add_history_entry(query)?;

        let (event_tx, mut event_rx) = mpsc::channel(64);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let app_clone = app.clone();
        let input = query.to_owned();

        tokio::spawn(async move {
            let mut ctx_clone = ctx.clone();
            let result = app_clone.handle_user_turn(&mut ctx_clone, &input, event_tx).await;
            let _ = result_tx.send((result, ctx_clone));
        });

        // 前台渲染事件
        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::TextDelta(_) => {}
                AgentEvent::ToolCall { name, input, parallel_index } => {
                    let detail = match name.as_str() {
                        "bash" => input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "read_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "write_file" => format!("{} ({} 字节)", input.get("path").and_then(|v| v.as_str()).unwrap_or(""), input.get("content").map(|v| v.as_str().map(|s| s.len()).unwrap_or(0)).unwrap_or(0)),
                        "edit_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "glob" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        "grep" => {
                            let mut parts = vec![input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned()];
                            if let Some(p) = input.get("path").and_then(|v| v.as_str()) { parts.push(p.to_owned()); }
                            parts.join(" in ")
                        }
                        "todo" => {
                            let items = input.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
                            format!("{items} 项")
                        }
                        "task" => input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
                        _ => input.to_string(),
                    };
                    let tag = match parallel_index {
                        Some((idx, total)) => format!("[并行 {idx}/{total}] "),
                        None => String::new(),
                    };
                    println!("┌─ {tag}{name}: `{detail}`");
                }
                AgentEvent::ToolResult { name: _, output, parallel_index } => {
                    let tag = match parallel_index {
                        Some((idx, total)) => format!("[并行 {idx}/{total}] "),
                        None => String::new(),
                    };
                    for line in output.lines() {
                        println!("│  {tag}{line}");
                    }
                    println!("└─");
                }
                AgentEvent::TurnEnd => {}
                AgentEvent::Done => {}
            }
        }

        match result_rx.await {
            Ok((Ok(text), updated_ctx)) => {
                if !text.trim().is_empty() {
                    termimad::print_text(&text);
                }
                ctx = updated_ctx;
                println!();
            }
            Ok((Err(error), _)) => {
                eprintln!("Error: {error}");
                println!();
            }
            Err(_) => {
                eprintln!("Error: agent 任务异常终止");
                println!();
            }
        }

        rust_agent_core::infra::usage::UsageTracker::display_with_quotas(app.quotas());
    }

    Ok(())
}
```

---

### Task 10: 更新 Server — Session + Routes + OpenAI 兼容

**Files:**
- Modify: `crates/server/src/session.rs`
- Modify: `crates/server/src/routes.rs`
- Modify: `crates/server/src/openai_compat.rs`

- [ ] **Step 1: 修改 `session.rs`**

```rust
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rust_agent_core::agent::AgentApp;
use rust_agent_core::context::ContextService;

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub context: ContextService,
    pub agent: Arc<AgentApp>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<DashMap<String, Session>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self { sessions: Arc::new(DashMap::new()) }
    }

    pub fn create(&self, agent: Arc<AgentApp>) -> Session {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let session = Session {
            id: id.clone(),
            context: ContextService::new(),
            agent,
            created_at: now,
            last_active: now,
        };
        self.sessions.insert(id, session.clone());
        session
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).map(|r| r.value().clone())
    }

    pub fn update(&self, id: &str, context: ContextService) {
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.context = context;
            session.last_active = Utc::now();
        }
    }

    pub fn remove(&self, id: &str) -> bool {
        self.sessions.remove(id).is_some()
    }
}
```

- [ ] **Step 2: 修改 `routes.rs`**

`get_session` 中 `session.messages.len()` → `session.context.len()`
`send_message` 中 `&mut messages` → `&mut ctx`（ContextService）

```rust
// get_session 中变更：
"message_count": session.context.len(),

// send_message 中变更：
let mut ctx = session.context.clone();
// ...
tokio::spawn(async move {
    let _ = agent.handle_user_turn(&mut ctx, &content, event_tx).await;
    store_clone.update(&session_id, ctx);
});
```

- [ ] **Step 3: 修改 `openai_compat.rs` 第 186-191 行**

将 `Vec::new()` 替换为 `ContextService::new()`：

```rust
// 原代码：
let mut messages = Vec::new();
tokio::spawn(async move {
    let _ = agent
        .handle_user_turn(&mut messages, &user_input, event_tx)
        .await;
});

// 修改为：
use rust_agent_core::context::ContextService;
// ...
let mut ctx = ContextService::new();
tokio::spawn(async move {
    let _ = agent
        .handle_user_turn(&mut ctx, &user_input, event_tx)
        .await;
});
```

---

### Task 11: 全面编译与测试

- [ ] **Step 1: 全量编译**

Run: `cargo build --release 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 2: 运行所有测试**

Run: `cargo test 2>&1 | tail -30`
Expected: 所有测试通过

- [ ] **Step 3: 检查 clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: 无 warning

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/agent.rs crates/core/src/infra/compact.rs crates/core/src/lib.rs \
        crates/cli/src/main.rs \
        crates/server/src/session.rs crates/server/src/routes.rs crates/server/src/openai_compat.rs
git commit -m "refactor: DDD 重构 — context/ 域 + command/ 域，AgentApp 使用 ContextService"
```

---

### Task 12: 端到端手动验证

- [ ] **Step 1: 验证 CLI 基本对话**

启动 `cargo run -p rust-agent-cli`，发送一条普通消息，确认正常回复。

- [ ] **Step 2: 验证 /clear 命令**

连续发送几条消息后执行 `/clear`，确认显示"上下文已清空（清除 N 条消息）"。

- [ ] **Step 3: 验证 /stats 命令**

发送几条消息后执行 `/stats`，确认显示正确的消息数和 token 估算。

- [ ] **Step 4: 验证 /help 命令**

执行 `/help`，确认显示所有可用命令。

- [ ] **Step 5: 验证 /compact 命令**

发送几条消息后执行 `/compact`，确认调用 LLM 进行压缩。

- [ ] **Step 6: 验证并行 subagent**

发送一个需要并行处理的请求，确认 `[并行 X/Y]` 标识正常显示。

- [ ] **Step 7: Final commit（如有修复）**

```bash
git add -A
git commit -m "fix: DDD 重构端到端验证修复"
```
