//! Shell 执行抽象层
//!
//! 提供 ShellProvider trait 和通用执行逻辑（超时、解码、截断）。

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use tokio::process::Command;
use tokio::time::timeout;

use crate::AgentResult;
use crate::infra::utils::truncate_text;

pub mod bash;
pub mod powershell;

/// Shell 执行提供者接口
#[async_trait]
pub trait ShellProvider: Send + Sync {
    /// 执行命令，返回 stdout + stderr 合并输出
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String>;
}

/// 通用 shell 命令执行逻辑（超时 120s、合并输出、智能解码、截断）
pub async fn run_shell_command(mut cmd: Command, cwd: &Path) -> AgentResult<String> {
    cmd.current_dir(cwd)
        .env("PYTHONIOENCODING", "utf-8");

    let output = timeout(Duration::from_secs(120), cmd.output()).await;
    let output = match output {
        Ok(result) => result.context("Failed to execute shell command")?,
        Err(_) => return Err(anyhow::anyhow!("命令执行超时（120秒）")),
    };

    let mut combined = String::new();
    combined.push_str(&decode_command_output(&output.stdout));
    combined.push_str(&decode_command_output(&output.stderr));
    let trimmed = combined.trim();

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let msg = if trimmed.is_empty() {
            format!("命令执行失败 (exit {code})")
        } else {
            format!("命令执行失败 (exit {code}): {trimmed}")
        };
        return Err(anyhow::anyhow!("{msg}"));
    }

    if trimmed.is_empty() {
        Ok("(无输出)".to_owned())
    } else {
        Ok(truncate_text(trimmed, 50_000))
    }
}

/// 智能解码命令输出的字节数据，自动在 UTF-8、GBK 和 UTF-16 LE 之间选择
pub fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    // 1. 检查是否有 UTF-16 LE BOM (0xFF 0xFE)
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16_data: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        if let Ok(utf16) = String::from_utf16(&u16_data) {
            return utf16;
        }
    }

    // 2. 启发式检测无 BOM 的 UTF-16 LE (Windows PowerShell 常见行为)
    // 如果长度是偶数，且包含大量空字节（每个字符两个字节），则可能是 UTF-16 LE
    if bytes.len() >= 4 && bytes.len() % 2 == 0 {
        let null_count = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        if null_count > bytes.len() / 4 {
            let u16_data: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            if let Ok(utf16) = String::from_utf16(&u16_data) {
                if !looks_like_mojibake(&utf16) {
                    return utf16;
                }
            }
        }
    }

    // 3. 尝试 UTF-8
    if let Ok(utf8) = String::from_utf8(bytes.to_vec()) {
        if !looks_like_mojibake(&utf8) {
            return utf8;
        }
        let (gbk, _, gbk_had_errors) = encoding_rs::GBK.decode(bytes);
        let gbk = gbk.into_owned();
        if !gbk_had_errors && decoding_score(&gbk) > decoding_score(&utf8) {
            return gbk;
        }
        return utf8;
    }

    // 4. 回退：有损 UTF-8 或 GBK
    let utf8 = String::from_utf8_lossy(bytes).into_owned();
    let (gbk, _, gbk_had_errors) = encoding_rs::GBK.decode(bytes);
    let gbk = gbk.into_owned();

    if gbk_had_errors {
        return utf8;
    }

    if decoding_score(&gbk) > decoding_score(&utf8) {
        gbk
    } else {
        utf8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_command_output_handles_utf16le_with_bom() {
        let text = "Hello, 世界";
        let bom = vec![0xFF, 0xFE]; // BOM
        let content: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes().to_vec()).collect();
        let bytes = [bom, content].concat();
        
        assert_eq!(decode_command_output(&bytes), text);
    }

    #[test]
    fn decode_command_output_handles_utf16le_without_bom_heuristic() {
        // PowerShell 经常输出不带 BOM 的 UTF-16 LE (英文字符带 \0)
        let bytes = b"H\0e\0l\0l\0o\0";
        assert_eq!(decode_command_output(bytes), "Hello");
    }
}

fn looks_like_mojibake(text: &str) -> bool {
    let has_latin = text.chars().any(|ch| matches!(ch, '\u{0080}'..='\u{024F}'));
    let has_cjk = text.chars().any(|ch| matches!(ch, '\u{4E00}'..='\u{9FFF}'));
    has_latin && !has_cjk
}

fn decoding_score(text: &str) -> i32 {
    text.chars().fold(0, |score, ch| {
        score
            + match ch {
                '\u{4E00}'..='\u{9FFF}' => 3,
                '\n' | '\r' | '\t' => 1,
                ' '..='~' => 1,
                '\u{0080}'..='\u{024F}' => -2,
                '\u{FFFD}' => -5,
                _ if ch.is_control() => -3,
                _ => 0,
            }
    })
}
