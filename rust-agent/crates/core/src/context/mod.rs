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
        workspace_root: &Path,
    ) -> AgentResult<Vec<ApiMessage>> {
        compact::auto_compact(client, model, workspace_root, &self.conversation).await
    }
}

impl Default for ContextService {
    fn default() -> Self {
        Self::new()
    }
}