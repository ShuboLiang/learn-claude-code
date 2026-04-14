//! 对话日志记录器
//!
//! 实时写入文件，每条日志写入后立即 flush，确保即使程序崩溃也能保留已记录的内容。

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// 对话日志记录器，在 `~/.rust-agent/logs/` 下创建以时间戳命名的日志文件
pub struct ConversationLogger {
    file: Option<std::fs::File>,
}

impl ConversationLogger {
    /// 创建新的日志记录器
    pub fn create() -> Self {
        let log_dir = match dirs::home_dir() {
            Some(home) => home.join(".rust-agent").join("logs"),
            None => return Self { file: None },
        };

        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            eprintln!("创建日志目录失败: {e}");
            return Self { file: None };
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let datetime = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d_%H-%M-%S").to_string())
            .unwrap_or_else(|| format!("{}", now.as_secs()));

        let filename = log_dir.join(format!("{datetime}.log"));
        let file = std::fs::File::create(&filename)
            .map_err(|e| eprintln!("创建日志文件失败: {e}"))
            .ok();

        Self { file }
    }

    /// 写入一条日志，立即 flush 到磁盘
    pub fn log(&mut self, entry: &str) {
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{entry}\n---");
            let _ = file.flush();
        }
    }
}
