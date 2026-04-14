//! API 用量统计与配额提醒
//!
//! 按 (base_url, api_key) 组合统计调用次数，配额规则从 profile 配置中获取。
//! 当用量接近或超过配额时，在终端打印警告。未配置配额则不限制。

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::AgentResult;

/// 统计数据文件名
const USAGE_FILE_NAME: &str = "usage_stats.json";

/// 配额提醒阈值（达到 80% 时开始提醒）
const ALERT_THRESHOLD: f64 = 0.8;

/// 单个账户的用量记录
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UsageRecord {
    /// 账户标识（base_url + api_key 的哈希前 12 位）
    pub id: String,
    /// API 基础 URL
    pub base_url: String,
    /// API 密钥（脱敏，只保留前 8 位）
    pub api_key_masked: String,
    /// 每次调用的 Unix 时间戳（秒），用于按时间窗口统计
    call_timestamps: Vec<u64>,
}

impl UsageRecord {
    /// 总调用次数
    pub fn call_count(&self) -> u64 {
        self.call_timestamps.len() as u64
    }

    /// 在指定时间窗口（秒）内的调用次数
    pub fn count_in_window(&self, window_secs: u64) -> u64 {
        let now = now_secs();
        let cutoff = now.saturating_sub(window_secs);
        self.call_timestamps
            .iter()
            .filter(|&&ts| ts > cutoff)
            .count() as u64
    }

    /// 清理过期的时间戳
    fn cleanup_old_timestamps(&mut self, max_window_secs: u64) {
        if max_window_secs == 0 {
            return;
        }
        let cutoff = now_secs().saturating_sub(max_window_secs * 2);
        self.call_timestamps.retain(|&ts| ts > cutoff);
    }
}

/// 配额规则
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuotaRule {
    /// 时间窗口描述（如 "5h"、"7d"、"30d"）
    pub window: String,
    /// 时间窗口对应的秒数
    pub window_secs: u64,
    /// 该窗口内的最大调用次数
    pub max_calls: u64,
}

impl QuotaRule {
    /// 从配置中的 QuotaConfig 创建
    pub fn from_config(window: &str, max_calls: u64) -> Self {
        Self {
            window: window.to_owned(),
            window_secs: parse_duration(window),
            max_calls,
        }
    }

    /// 时间窗口的描述
    pub fn description(&self) -> String {
        format!("每 {} {} 次", self.window, self.max_calls)
    }
}

/// 持久化的用量数据（不包含配额规则，配额从配置文件读取）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct UsageData {
    /// 按账户 ID 索引的用量记录
    records: HashMap<String, UsageRecord>,
    /// 已经提醒过的配额（避免重复提醒）
    #[serde(default)]
    alerted: HashMap<String, Vec<usize>>,
}

/// 用量统计管理器
pub struct UsageTracker {
    /// 持久化的用量数据
    data: UsageData,
    /// 当前 profile 的配额规则（从配置文件传入，为空则不限制）
    quotas: Vec<QuotaRule>,
}

impl UsageTracker {
    /// 获取统计文件的路径
    fn file_path() -> AgentResult<PathBuf> {
        let dir = dirs::home_dir()
            .context("无法获取用户主目录")?
            .join(".rust-agent");
        fs::create_dir_all(&dir)?;
        Ok(dir.join(USAGE_FILE_NAME))
    }

    /// 从磁盘加载统计数据，并设置配额规则
    pub fn load(quotas: Vec<QuotaRule>) -> AgentResult<Self> {
        let path = Self::file_path()?;
        let data = if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("读取用量统计文件失败: {}", path.display()))?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            UsageData::default()
        };
        Ok(Self { data, quotas })
    }

    /// 从磁盘加载统计数据并显示用量状态（不需要配额参数）
    pub fn display() {
        let path = match Self::file_path() {
            Ok(p) => p,
            Err(_) => return,
        };
        if !path.exists() {
            return;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let data: UsageData = serde_json::from_str(&content).unwrap_or_default();
        if data.records.is_empty() {
            return;
        }

        println!("┌─ 用量统计");
        for record in data.records.values() {
            println!("│  [{}] {} | 总计 {} 次", record.id, record.api_key_masked, record.call_count());
        }
        println!("└─");
    }

    /// 从磁盘加载数据并显示带配额的详细用量状态
    pub fn display_with_quotas(quotas: &[QuotaRule]) {
        if quotas.is_empty() {
            Self::display();
            return;
        }

        let data = match Self::file_path().and_then(|path| {
            if !path.exists() { return Ok(UsageData::default()); }
            let content = fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&content).unwrap_or_default())
        }) {
            Ok(d) => d,
            Err(_) => return,
        };

        if data.records.is_empty() {
            return;
        }

        println!("┌─ 用量统计");
        for record in data.records.values() {
            println!("│  [{}] {} | 总计 {} 次", record.id, record.api_key_masked, record.call_count());
            for quota in quotas {
                let count = record.count_in_window(quota.window_secs);
                let ratio = count as f64 / quota.max_calls as f64;
                let bar_len = 20;
                let filled = ((ratio * bar_len as f64) as usize).min(bar_len);
                let bar: String = "█".repeat(filled) + &"░".repeat(bar_len - filled);
                println!("│    {}  {}/{} ({}%)", quota.window, bar, count, quota.max_calls);
            }
        }
        println!("└─");
    }

    /// 保存统计数据到磁盘
    fn save(&self) -> AgentResult<()> {
        let path = Self::file_path()?;
        let content = serde_json::to_string_pretty(&self.data)?;
        let mut file = fs::File::create(&path)
            .with_context(|| format!("创建用量统计文件失败: {}", path.display()))?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    /// 记录一次 API 调用，并检查配额
    pub fn record_call(&mut self, base_url: &str, api_key: &str) -> AgentResult<()> {
        let id = make_id(base_url, api_key);
        let masked_key = mask_key(api_key);

        // 添加调用记录
        {
            let record = self.data.records.entry(id.clone()).or_insert_with(|| UsageRecord {
                id: id.clone(),
                base_url: base_url.to_owned(),
                api_key_masked: masked_key,
                call_timestamps: Vec::new(),
            });
            record.call_timestamps.push(now_secs());

            // 清理旧时间戳（取最长配额窗口）
            let max_window = self.quotas.iter().map(|q| q.window_secs).max().unwrap_or(0);
            record.cleanup_old_timestamps(max_window);
        }

        // 无配额规则则跳过提醒
        if self.quotas.is_empty() {
            return self.save();
        }

        // 检查配额并提醒
        let window_counts: Vec<(String, u64, u64)> = {
            let record = match self.data.records.get(&id) {
                Some(r) => r,
                None => return self.save(),
            };
            self.quotas
                .iter()
                .enumerate()
                .map(|(idx, q)| {
                    (q.description(), record.count_in_window(q.window_secs), idx as u64)
                })
                .collect()
        };

        let alerts = self.data.alerted.entry(id.clone()).or_default();
        for (desc, count, idx) in &window_counts {
            let quota = &self.quotas[*idx as usize];
            let ratio = *count as f64 / quota.max_calls as f64;
            let idx_usize = *idx as usize;

            if ratio >= 1.0 && !alerts.contains(&idx_usize) {
                eprintln!(
                    "\n[用量警告] {} - 已超过配额！{}/{} ({} 内)\n",
                    desc, count, quota.max_calls, quota.window,
                );
                alerts.push(idx_usize);
            } else if ratio >= ALERT_THRESHOLD && !alerts.contains(&idx_usize) {
                eprintln!(
                    "\n[用量提醒] {} - 已使用 {}/{} ({}%, {} 内)\n",
                    desc, count, quota.max_calls, (ratio * 100.0) as u64, quota.window,
                );
                alerts.push(idx_usize);
            } else if ratio < ALERT_THRESHOLD {
                alerts.retain(|&i| i != idx_usize);
            }
        }

        self.save()
    }

    /// 获取所有统计记录
    pub fn records(&self) -> &HashMap<String, UsageRecord> {
        &self.data.records
    }

    /// 获取当前配额规则
    pub fn quotas(&self) -> &[QuotaRule] {
        &self.quotas
    }

}

/// 获取当前 Unix 时间戳（秒）
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 根据 base_url 和 api_key 生成账户 ID（哈希前 12 位）
fn make_id(base_url: &str, api_key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    base_url.hash(&mut hasher);
    api_key.hash(&mut hasher);
    format!("{:012x}", hasher.finish())
}

/// 脱敏 API 密钥，只保留前 8 位
fn mask_key(api_key: &str) -> String {
    if api_key.len() <= 8 {
        "***".to_owned()
    } else {
        format!("{}***", &api_key[..8])
    }
}

/// 解析时间字符串为秒数
fn parse_duration(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let (num_str, unit) = if let Some(pos) = s.find(|c: char| !c.is_ascii_digit()) {
        (&s[..pos], &s[pos..])
    } else {
        (s, "")
    };
    let num: u64 = num_str.parse().unwrap_or(1);
    match unit {
        "s" | "S" => num,
        "m" => num * 60,
        "h" | "H" => num * 3600,
        "d" | "D" => num * 86400,
        "w" | "W" => num * 86400 * 7,
        _ => num,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 相同url和密钥生成相同id() {
        let id1 = make_id("https://api.openai.com", "sk-1234567890");
        let id2 = make_id("https://api.openai.com", "sk-1234567890");
        assert_eq!(id1, id2);
    }

    #[test]
    fn 不同密钥生成不同id() {
        let id1 = make_id("https://api.openai.com", "sk-111");
        let id2 = make_id("https://api.openai.com", "sk-222");
        assert_ne!(id1, id2);
    }

    #[test]
    fn 密钥脱敏() {
        assert_eq!(mask_key("sk-1234567890"), "sk-12345***");
        assert_eq!(mask_key("short"), "***");
    }

    #[test]
    fn 解析时间字符串() {
        assert_eq!(parse_duration("5s"), 5);
        assert_eq!(parse_duration("5m"), 300);
        assert_eq!(parse_duration("5h"), 18000);
        assert_eq!(parse_duration("7d"), 604800);
        assert_eq!(parse_duration("1w"), 604800);
    }

    #[test]
    fn 时间窗口内统计() {
        let record = UsageRecord {
            id: "test".to_owned(),
            base_url: "https://api.test.com".to_owned(),
            api_key_masked: "sk-test***".to_owned(),
            call_timestamps: vec![now_secs(), now_secs() - 100, now_secs() - 1000],
        };
        assert_eq!(record.count_in_window(500), 2);
        assert_eq!(record.count_in_window(100000), 3);
    }

    #[test]
    fn 配额规则创建() {
        let rule = QuotaRule::from_config("5h", 1200);
        assert_eq!(rule.window_secs, 18000);
        assert_eq!(rule.max_calls, 1200);
    }

    #[test]
    fn 记录调用次数() {
        let mut tracker = UsageTracker {
            data: UsageData::default(),
            quotas: vec![],
        };
        tracker.record_call("https://api.openai.com", "sk-test123456").unwrap();
        tracker.record_call("https://api.openai.com", "sk-test123456").unwrap();
        tracker.record_call("https://api.anthropic.com", "sk-ant-other").unwrap();

        assert_eq!(tracker.records().len(), 2);
        let openai_id = make_id("https://api.openai.com", "sk-test123456");
        assert_eq!(tracker.records()[&openai_id].call_count(), 2);
    }

    #[test]
    fn 无配额时不提醒() {
        let mut tracker = UsageTracker {
            data: UsageData::default(),
            quotas: vec![],
        };
        // 不应 panic，只是不提醒
        for _ in 0..10 {
            tracker.record_call("https://api.test.com", "sk-test").unwrap();
        }
        assert_eq!(tracker.records().len(), 1);
    }
}
