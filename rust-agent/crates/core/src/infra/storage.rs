//! 工具结果持久化模块
//!
//! 当工具输出超过阈值时，将完整内容保存到磁盘（~/.rust-agent/tool-results/），
//! 消息中只保留前 N 字节的预览和文件路径引用，防止上下文膨胀。
//! 模型可以随时通过 read_file 读取完整结果。

use std::fs;
use std::path::PathBuf;

/// 工具结果超过此字符数时触发持久化
const PERSIST_THRESHOLD: usize = 50_000;

/// 预览保留的字符数
const PREVIEW_CHARS: usize = 2_000;

/// 获取工具结果的持久化目录
///
/// 目录位置：`~/.rust-agent/tool-results/`
/// 如果目录不存在会自动创建
fn tool_results_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::home_dir()
        .map(|p| p.join(".rust-agent").join("tool-results"))
        .ok_or_else(|| anyhow::anyhow!("无法确定用户主目录"))?;

    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// 处理工具结果：大的存盘留预览，小的原样返回
///
/// # 参数
/// - `tool_use_id`: 工具调用的唯一标识（用作文件名）
/// - `content`: 工具执行的完整输出
///
/// # 返回值
/// - 如果内容不超过阈值，原样返回
/// - 如果超过阈值，保存到磁盘，返回预览 + 文件路径
pub fn maybe_persist(tool_use_id: &str, content: &str) -> String {
    if content.len() <= PERSIST_THRESHOLD {
        return content.to_owned();
    }

    let dir = match tool_results_dir() {
        Ok(d) => d,
        Err(_) => return content.to_owned(), // 无法创建目录，原样返回
    };

    // 用 tool_use_id 作为文件名（去掉特殊字符）
    let safe_id = tool_use_id.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
    let path = dir.join(format!("{safe_id}.txt"));

    if let Err(e) = fs::write(&path, content) {
        eprintln!("[tool_result_storage] 写入失败: {e}");
        return content.to_owned();
    }

    let size_kb = content.len() / 1024;
    let preview: String = content.chars().take(PREVIEW_CHARS).collect();
    let truncated = content.chars().count() > PREVIEW_CHARS;

    format!(
        "<persisted-output>\n\
         输出过大 ({size_kb} KB)。完整内容已保存到: {path}\n\
         预览（前 {PREVIEW_CHARS} 字符）：\n\
         {preview}{trunc}\n\
         </persisted-output>",
        path = path.display(),
        trunc = if truncated { "\n..." } else { "" },
    )
}

/// 清理超过指定天数的工具结果文件
///
/// # 参数
/// - `max_age_days`: 保留的最大天数
pub fn cleanup_old_results(max_age_days: u64) {
    let Ok(dir) = tool_results_dir() else {
        return;
    };

    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };

    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(max_age_days * 24 * 60 * 60);

    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                if modified < cutoff {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 小内容不触发持久化() {
        let result = maybe_persist("test_id", "短内容");
        assert_eq!(result, "短内容");
        assert!(!result.contains("persisted-output"));
    }

    #[test]
    fn 大内容触发持久化() {
        let long_content = "x".repeat(PERSIST_THRESHOLD + 1);
        let result = maybe_persist("test_large_id", &long_content);
        assert!(result.contains("persisted-output"));
        assert!(result.contains("预览"));
        assert!(result.len() < long_content.len());
    }
}
