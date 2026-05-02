//! 工具级断路器（Circuit Breaker）
//!
//! 两类熔断：
//! 1. 失败熔断：同一工具连续失败 N 次后阻止继续调用
//! 2. 重复熔断：同一工具的相同（归一化后）输入在最近 K 次中出现 ≥ M 次则阻止继续调用
//!
//! 重复熔断针对模型陷入"换关键词反复重试"的死循环（典型场景：tvly search
//! 返回 answer:null 后，模型把同一查询换近义词重复 30+ 次）。

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use serde_json::Value;

const DEFAULT_THRESHOLD: usize = 5;
const RECENT_INPUTS_WINDOW: usize = 10;
const REPEAT_THRESHOLD: usize = 3;

/// 工具级断路器，按工具名独立计数
pub struct ToolCircuitBreaker {
    /// 每个工具的连续失败次数
    consecutive_failures: HashMap<String, usize>,
    /// 每个工具最近 N 次输入的归一化哈希（用于检测重复调用）
    recent_input_hashes: HashMap<String, VecDeque<u64>>,
    /// 失败熔断阈值
    threshold: usize,
}

impl Default for ToolCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolCircuitBreaker {
    pub fn new() -> Self {
        Self {
            consecutive_failures: HashMap::new(),
            recent_input_hashes: HashMap::new(),
            threshold: DEFAULT_THRESHOLD,
        }
    }

    /// 检查指定工具是否已被失败熔断
    pub fn is_open(&self, tool_name: &str) -> bool {
        self.consecutive_failures
            .get(tool_name)
            .is_some_and(|&count| count >= self.threshold)
    }

    /// 记录工具调用成功，重置该工具的连续失败计数
    pub fn record_success(&mut self, tool_name: &str) {
        self.consecutive_failures.remove(tool_name);
    }

    /// 记录工具调用失败，递增连续失败计数
    pub fn record_failure(&mut self, tool_name: &str) {
        *self
            .consecutive_failures
            .entry(tool_name.to_owned())
            .or_insert(0) += 1;
    }

    /// 获取指定工具的当前连续失败次数
    pub fn failure_count(&self, tool_name: &str) -> usize {
        self.consecutive_failures
            .get(tool_name)
            .copied()
            .unwrap_or(0)
    }

    /// 失败熔断后的提示信息
    pub fn blocked_message(tool_name: &str, count: usize) -> String {
        format!(
            "⚠️ 工具 \"{tool_name}\" 已连续失败 {count} 次，已自动停止调用。\n\
             这通常意味着用户环境缺少必要的依赖。\n\
             请停止尝试使用此工具，直接告知用户需要安装什么。"
        )
    }

    /// 在 dispatch 前记录工具输入。如果归一化后的输入在最近窗口内已出现
    /// `REPEAT_THRESHOLD` 次（含本次），返回 `Some(count)`，调用方应跳过实际
    /// dispatch 并把熔断消息塞回 LLM；否则返回 `None`。
    pub fn record_input(&mut self, tool_name: &str, input: &Value) -> Option<usize> {
        let hash = normalize_and_hash(input);
        let queue = self
            .recent_input_hashes
            .entry(tool_name.to_owned())
            .or_default();
        queue.push_back(hash);
        if queue.len() > RECENT_INPUTS_WINDOW {
            queue.pop_front();
        }
        let count = queue.iter().filter(|&&h| h == hash).count();
        if count >= REPEAT_THRESHOLD {
            Some(count)
        } else {
            None
        }
    }

    /// 重复熔断后的提示信息
    pub fn repeat_blocked_message(tool_name: &str, count: usize) -> String {
        format!(
            "⚠️ 工具 \"{tool_name}\" 已用相同（或近似）输入调用 {count} 次，已自动阻止继续重试。\n\
             这通常意味着模型陷入了反复尝试的死循环。\n\
             请：1) 尝试根本不同的工具/参数；或 2) 直接根据已有信息回复用户，不要再调用同一工具。"
        )
    }
}

/// 把工具输入序列化为 JSON 字符串，去除所有 ASCII 空白后做大小写归一化，
/// 然后哈希。这样 `{"command": "TVLY x"}` 和 `{"command":"tvly x"}` 视为等价。
fn normalize_and_hash(input: &Value) -> u64 {
    let raw = input.to_string();
    let normalized: String = raw
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 失败熔断阈值生效() {
        let mut b = ToolCircuitBreaker::new();
        for _ in 0..5 {
            b.record_failure("bash");
        }
        assert!(b.is_open("bash"));
        assert_eq!(b.failure_count("bash"), 5);
    }

    #[test]
    fn 成功重置失败计数() {
        let mut b = ToolCircuitBreaker::new();
        b.record_failure("bash");
        b.record_failure("bash");
        b.record_success("bash");
        assert_eq!(b.failure_count("bash"), 0);
        assert!(!b.is_open("bash"));
    }

    #[test]
    fn 相同输入第三次触发重复熔断() {
        let mut b = ToolCircuitBreaker::new();
        let input = json!({"command": "tvly search 'hello'"});
        assert_eq!(b.record_input("bash", &input), None);
        assert_eq!(b.record_input("bash", &input), None);
        assert_eq!(b.record_input("bash", &input), Some(3));
    }

    #[test]
    fn 归一化忽略空白和大小写() {
        let mut b = ToolCircuitBreaker::new();
        b.record_input("bash", &json!({"command": "TVLY search hello"}));
        b.record_input("bash", &json!({"command": "tvly  search   hello"}));
        // 第三次仍被视为同一输入
        assert_eq!(
            b.record_input("bash", &json!({"command": "tvly search hello"})),
            Some(3)
        );
    }

    #[test]
    fn 不同输入不触发熔断() {
        let mut b = ToolCircuitBreaker::new();
        for i in 0..5 {
            let input = json!({"command": format!("tvly search {i}")});
            assert_eq!(b.record_input("bash", &input), None);
        }
    }

    #[test]
    fn 不同工具计数互不影响() {
        let mut b = ToolCircuitBreaker::new();
        let input = json!({"command": "x"});
        b.record_input("bash", &input);
        b.record_input("bash", &input);
        // 同样的输入但不同工具
        assert_eq!(b.record_input("curl", &input), None);
    }

    #[test]
    fn 窗口外的旧输入不计数() {
        let mut b = ToolCircuitBreaker::new();
        let target = json!({"command": "target"});
        // 先用 target 一次
        b.record_input("bash", &target);
        // 用其它输入填满 10 次窗口（把 target 挤出去）
        for i in 0..10 {
            b.record_input("bash", &json!({"command": format!("noise{i}")}));
        }
        // 此时 target 已被挤出窗口；再连续两次 target 不应触发
        assert_eq!(b.record_input("bash", &target), None);
        assert_eq!(b.record_input("bash", &target), None);
    }
}
