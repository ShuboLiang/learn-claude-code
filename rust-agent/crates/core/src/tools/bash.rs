use anyhow::Context;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use crate::AgentResult;

impl super::AgentToolbox {
    /// 执行 shell 命令并返回输出
    ///
    /// 1. 检查危险关键词（如 `rm -rf /`、`sudo` 等），有则拦截
    /// 2. Windows 用 PowerShell，其他用 `sh -lc`
    /// 3. 在 Windows 上额外设置 UTF-8 编码环境
    /// 4. 设置工作目录为工作区根目录，超时限制 120 秒
    /// 5. 合并 stdout 和 stderr，智能解码（UTF-8 / GBK），截断到 50000 字符
    pub(crate) async fn run_bash(&self, command: &str) -> AgentResult<String> {
        let dangerous = ["rm -rf /", "sudo", "shutdown", "reboot", "> /dev/"];
        if dangerous.iter().any(|blocked| command.contains(blocked)) {
            return Ok("错误：危险命令已被拦截".to_owned());
        }

        let mut process = if cfg!(windows) {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoLogo")
                .arg("-NonInteractive")
                .arg("-Command")
                .arg(wrap_powershell_command(command));
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-lc").arg(command);
            cmd
        };

        process.current_dir(&self.workspace_root);
        let output = timeout(Duration::from_secs(120), process.output()).await;
        let output = match output {
            Ok(result) => result.context("Failed to execute shell command")?,
            Err(_) => return Ok("错误：命令执行超时（120秒）".to_owned()),
        };

        let mut combined = String::new();
        combined.push_str(&decode_command_output(&output.stdout));
        combined.push_str(&decode_command_output(&output.stderr));
        let trimmed = combined.trim();
        if trimmed.is_empty() {
            Ok("(无输出)".to_owned())
        } else {
            Ok(trimmed.chars().take(50_000).collect())
        }
    }
}

/// 为 PowerShell 命令包装 UTF-8 编码环境设置
fn wrap_powershell_command(command: &str) -> String {
    format!(
        "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
$OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
chcp 65001 > $null; \
{command}"
    )
}

/// 智能解码命令输出的字节数据，自动在 UTF-8 和 GBK 之间选择最佳解码结果
///
/// 1. 先尝试 UTF-8 严格解码，如果完全成功且不疑似乱码则直接返回
/// 2. 如果 UTF-8 失败或疑似乱码，再尝试 GBK 解码
/// 3. 用 `decoding_score` 给两种结果打分，返回得分更高的
pub(crate) fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    // UTF-8 严格解码：如果完全成功，检查是否看起来像乱码
    if let Ok(utf8) = String::from_utf8(bytes.to_vec()) {
        if !looks_like_mojibake(&utf8) {
            return utf8;
        }
        // UTF-8 虽然合法但看起来像乱码，尝试 GBK 作为候选
        let (gbk, _, gbk_had_errors) = encoding_rs::GBK.decode(bytes);
        let gbk = gbk.into_owned();
        if !gbk_had_errors && decoding_score(&gbk) > decoding_score(&utf8) {
            return gbk;
        }
        return utf8;
    }

    // UTF-8 失败，用 lossy 解码作为候选
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

/// 判断文本是否看起来像 GBK 被误读为 UTF-8 后的乱码
fn looks_like_mojibake(text: &str) -> bool {
    let has_latin_ext = text.chars().any(|ch| matches!(ch, '\u{0100}'..='\u{024F}'));
    let has_cjk = text.chars().any(|ch| matches!(ch, '\u{4E00}'..='\u{9FFF}'));
    has_latin_ext && !has_cjk
}

/// 给解码后的文本打分，用于判断哪种编码的解码结果更合理
fn decoding_score(text: &str) -> i32 {
    text.chars().fold(0, |score, ch| {
        score
            + match ch {
                '\u{4E00}'..='\u{9FFF}' => 3,
                '\n' | '\r' | '\t' => 1,
                ' '..='~' => 1,
                '\u{0100}'..='\u{024F}' => -2,
                '\u{FFFD}' => -5,
                _ if ch.is_control() => -3,
                _ => 0,
            }
    })
}
