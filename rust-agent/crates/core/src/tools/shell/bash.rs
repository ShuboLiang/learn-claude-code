//! Bash Shell Provider
//!
//! 仅用于 Unix：使用 /bin/sh -lc

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::process::Command;

use crate::AgentResult;
use super::{run_shell_command, ShellProvider};

pub struct BashShellProvider {
    shell_path: PathBuf,
    extra_env: HashMap<String, String>,
}

impl BashShellProvider {
    pub fn new(extra_env: HashMap<String, String>) -> Self {
        Self {
            shell_path: PathBuf::from("/bin/sh"),
            extra_env,
        }
    }

    /// 返回当前实际使用的 shell 名称（供系统提示词使用）
    pub fn actual_shell_name(&self) -> &'static str {
        "bash"
    }
}

#[async_trait]
impl ShellProvider for BashShellProvider {
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        let mut cmd = Command::new(&self.shell_path);

        // 注入配置中的额外环境变量
        for (key, value) in &self.extra_env {
            cmd.env(key, value);
        }

        cmd.arg("-lc").arg(command);

        // WSL 中过滤 /mnt/c/ 路径，避免执行 Windows 程序
        let filtered_path = std::env::var("PATH")
            .map(|p| {
                p.split(':')
                    .filter(|part| !part.starts_with("/mnt/c/"))
                    .collect::<Vec<_>>()
                    .join(":")
            })
            .unwrap_or_default();
        if !filtered_path.is_empty() {
            cmd.env("PATH", filtered_path);
        }

        run_shell_command(cmd, cwd).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    #[cfg(unix)]
    fn actual_shell_name_on_unix_is_bash() {
        let provider = BashShellProvider::new(HashMap::new());
        assert_eq!(provider.actual_shell_name(), "bash");
    }
}
