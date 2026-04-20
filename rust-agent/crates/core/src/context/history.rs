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
