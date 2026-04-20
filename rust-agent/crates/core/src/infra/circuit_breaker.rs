//! 工具级断路器（Circuit Breaker）
//!
//! 当某个工具连续失败超过阈值时，自动熔断该工具，
//! 阻止 LLM 继续盲目重试，并提示其告知用户安装环境依赖。

use std::collections::HashMap;

/// 熔断阈值：连续失败 5 次后触发
const DEFAULT_THRESHOLD: usize = 5;

/// 工具级断路器，按工具名独立计数
pub struct ToolCircuitBreaker {
    /// 每个工具的连续失败次数
    consecutive_failures: HashMap<String, usize>,
    /// 熔断阈值
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
            threshold: DEFAULT_THRESHOLD,
        }
    }

    /// 检查指定工具是否已被熔断
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

    /// 生成熔断后的提示信息，注入给 LLM 让其停止尝试并告知用户
    pub fn blocked_message(tool_name: &str, count: usize) -> String {
        format!(
            "⚠️ 工具 \"{tool_name}\" 已连续失败 {count} 次，已自动停止调用。\n\
             这通常意味着用户环境缺少必要的依赖。\n\
             请停止尝试使用此工具，直接告知用户需要安装什么。"
        )
    }
}
