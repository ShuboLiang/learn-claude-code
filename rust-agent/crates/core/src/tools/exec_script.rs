//! 一次性脚本执行工具
//!
//! 将代码写入系统临时目录运行，执行完毕后自动清理。
//! 解决 AI 滥用 write_file + bash 留下临时脚本垃圾的问题。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use tokio::process::Command;

use crate::AgentResult;
use crate::infra::workspace::resolve_workspace_path;
use crate::tools::shell::run_shell_command;

static EXEC_COUNTER: AtomicU64 = AtomicU64::new(0);

impl super::AgentToolbox {
    /// 执行一次性脚本/代码片段
    ///
    /// 代码写入系统临时目录，执行后自动删除。
    /// 执行失败时保留临时文件便于排查，并告知路径。
    pub(crate) async fn run_exec_script(
        &self,
        language: &str,
        code: &str,
        save_as: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> AgentResult<String> {
        // 1. 准备临时文件
        let ext = match language {
            "python" => "py",
            "node" => "js",
            "bash" => "sh",
            "powershell" => "ps1",
            other => return Err(anyhow::anyhow!("不支持的语言: {other}")),
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let counter = EXEC_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();

        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("rust-agent-exec-{pid}-{timestamp}-{counter}.{ext}"));

        tokio::fs::write(&temp_file, code)
            .await
            .with_context(|| format!("无法写入临时文件: {}", temp_file.display()))?;

        // 2. 构造执行命令
        let mut cmd = build_command(language, &temp_file)?;

        // 3. 执行（使用通用 shell 执行逻辑：超时、解码、截断）
        // 但 run_shell_command 默认 30s 超时，这里允许自定义
        let effective_timeout = timeout_secs.unwrap_or(30);
        let output = if effective_timeout == 30 {
            run_shell_command(cmd, &self.workspace_root).await
        } else {
            // 自定义超时时，手动管理 timeout
            use tokio::time::{timeout, Duration};
            let fut = async {
                let out = cmd.output().await.context("Failed to execute script")?;
                let mut combined = String::new();
                combined.push_str(&super::shell::decode_command_output(&out.stdout));
                combined.push_str(&super::shell::decode_command_output(&out.stderr));
                let trimmed = combined.trim();
                if !out.status.success() {
                    let code = out.status.code().unwrap_or(-1);
                    let msg = if trimmed.is_empty() {
                        format!("脚本执行失败 (exit {code})")
                    } else {
                        format!("脚本执行失败 (exit {code}): {trimmed}")
                    };
                    return Err(anyhow::anyhow!("{msg}"));
                }
                Ok(if trimmed.is_empty() {
                    "(无输出)".to_owned()
                } else {
                    crate::infra::utils::truncate_text(trimmed, 50_000)
                })
            };
            match timeout(Duration::from_secs(effective_timeout), fut).await {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!("脚本执行超时（{effective_timeout}秒）")),
            }
        };

        // 4. 处理 save_as（如果需要保留到工作区）
        if let Some(dest) = save_as {
            let resolved = resolve_workspace_path(&self.workspace_root, dest)?;
            if let Some(parent) = resolved.parent() {
                tokio::fs::create_dir_all(parent).await.with_context(|| {
                    format!("Failed to create directory {}", parent.display())
                })?;
            }
            tokio::fs::copy(&temp_file, &resolved)
                .await
                .with_context(|| {
                    format!(
                        "无法复制临时文件到目标路径: {} -> {}",
                        temp_file.display(),
                        resolved.display()
                    )
                })?;
        }

        // 5. 清理或保留
        match output {
            Ok(result) => {
                // 执行成功：删除临时文件
                let _ = tokio::fs::remove_file(&temp_file).await;
                Ok(result)
            }
            Err(e) => {
                // 执行失败：保留临时文件，便于用户排查
                let err_msg = format!(
                    "{e}\n\n[调试信息] 临时脚本已保留: {}",
                    temp_file.display()
                );
                Err(anyhow::anyhow!("{err_msg}"))
            }
        }
    }
}

/// 根据语言构造执行命令
fn build_command(language: &str, script_path: &Path) -> AgentResult<Command> {
    match language {
        "python" => {
            let mut cmd = Command::new("python");
            cmd.arg(script_path);
            Ok(cmd)
        }
        "node" => {
            let mut cmd = Command::new("node");
            cmd.arg(script_path);
            Ok(cmd)
        }
        "bash" => {
            let mut cmd = Command::new("bash");
            cmd.arg(script_path);
            Ok(cmd)
        }
        "powershell" => {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoProfile")
                .arg("-NonInteractive")
                .arg("-File")
                .arg(script_path);
            Ok(cmd)
        }
        other => Err(anyhow::anyhow!("不支持的语言: {other}")),
    }
}
