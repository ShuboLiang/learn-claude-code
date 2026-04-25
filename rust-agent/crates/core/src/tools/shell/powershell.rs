//! PowerShell Shell Provider

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::process::Command;

use super::{ShellProvider, run_shell_command};
use crate::AgentResult;

#[derive(Clone)]
pub struct PowerShellShellProvider {
    shell_path: PathBuf,
    extra_env: HashMap<String, String>,
}

impl PowerShellShellProvider {
    pub fn new(extra_env: HashMap<String, String>) -> Self {
        Self {
            shell_path: Self::find_pwsh().unwrap_or_else(|| PathBuf::from("powershell")),
            extra_env,
        }
    }

    fn find_pwsh() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            use std::env;
            if let Ok(path) = env::var("PATH") {
                for dir in path.split(';') {
                    let candidate = PathBuf::from(dir).join("pwsh.exe");
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }
}

#[async_trait]
impl ShellProvider for PowerShellShellProvider {
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        let mut cmd = Command::new(&self.shell_path);

        for (key, value) in &self.extra_env {
            cmd.env(key, value);
        }

        cmd.arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(wrap_powershell_command(command));
        run_shell_command(cmd, cwd).await
    }
}

/// 为 PowerShell 命令包装 UTF-8 编码环境设置
fn wrap_powershell_command(command: &str) -> String {
    format!(
        "$OutputEncoding = [Console]::InputEncoding = [Console]::OutputEncoding = (New-Object System.Text.UTF8Encoding $false); \
         chcp 65001 | Out-Null; \
         {command}"
    )
}
