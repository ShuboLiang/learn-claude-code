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