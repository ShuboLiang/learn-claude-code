//! PowerShell 工具：独立的 PowerShell 命令执行工具

use std::path::Path;

use crate::AgentResult;
use super::shell::powershell::PowerShellShellProvider;
use super::shell::ShellProvider;

#[derive(Clone)]
pub struct PowerShellTool {
    provider: PowerShellShellProvider,
}

impl PowerShellTool {
    pub fn new(extra_env: std::collections::HashMap<String, String>) -> Self {
        Self {
            provider: PowerShellShellProvider::new(extra_env),
        }
    }

    pub async fn run(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        self.provider.exec(command, cwd).await
    }
}
