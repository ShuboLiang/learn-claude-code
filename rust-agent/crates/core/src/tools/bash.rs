use crate::AgentResult;

impl super::AgentToolbox {
    /// 执行 shell 命令并返回输出
    ///
    /// 1. 检查危险关键词（如 `rm -rf /`、`sudo` 等），有则拦截
    /// 2. 通过 ShellProvider 执行，由 Provider 决定具体 shell（Unix 为 Bash，Windows 为 PowerShell）
    pub(crate) async fn run_bash(&self, command: &str) -> AgentResult<String> {
        // 危险命令检测：只匹配命令开头，避免误杀 URL 或参数中包含关键词的正常命令
        let trimmed = command.trim();
        let dangerous_prefixes = [
            "rm -rf /",
            "rm -rf ~",
            "rm -rf .",
            "sudo ",
            "shutdown ",
            "shutdown\n",
            "reboot ",
            "reboot\n",
            "> /dev/",
        ];
        if dangerous_prefixes
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            return Err(anyhow::anyhow!("危险命令已被拦截"));
        }

        self.default_shell.exec(command, &self.workspace_root).await
    }
}
