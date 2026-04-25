# 设计文档：ShellProvider 抽象 —— BashTool 与 PowerShellTool 分离

## 背景

当前 `crates/core/src/tools/bash.rs` 在 Windows 上硬编码调用 PowerShell 执行所有 shell 命令：

```rust
let mut process = if cfg!(windows) {
    let mut cmd = Command::new("powershell");
    cmd.arg("-NoLogo").arg("-NonInteractive").arg("-Command")
        .arg(wrap_powershell_command(command));
    cmd
} else {
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(command);
    cmd
};
```

这带来两个问题：
1. **语义错位**：工具名是 `bash`，实际在 Windows 上执行的是 PowerShell，模型输出的 bash 语法在 PowerShell 中经常报错
2. **无选择余地**：Windows 用户无法使用真正的 bash（Git Bash），也不能选择只用 PowerShell

参考 Claude Code 的做法：
- `BashTool` 始终用 bash（即使在 Windows 上也通过 Git Bash 执行）
- `PowerShellTool` 是独立的可选工具，仅在用户启用时出现

## 目标

1. 引入 `ShellProvider` trait，解耦工具与具体 shell 实现
2. `BashTool` 在 Windows 上**优先**寻找 Git Bash，找不到时 graceful fallback 到 PowerShell
3. 新增独立的 `PowerShellTool`，默认禁用，用户显式启用后才注册
4. 保持向后兼容：现有 Windows 用户无 Git Bash 时仍能正常使用（fallback 到 PowerShell）
5. 系统提示词显式告知模型当前实际使用的 shell，避免语法错配

## 设计方案

### 1. ShellProvider trait

```rust
// crates/core/src/tools/shell/mod.rs
use std::path::Path;
use async_trait::async_trait;
use crate::AgentResult;

#[async_trait]
pub trait ShellProvider: Send + Sync {
    /// 执行命令，返回 stdout + stderr 合并输出
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String>;

    /// Provider 标识名（用于日志和系统提示词）
    fn name(&self) -> &'static str;
}

/// 创建默认的 Bash ShellProvider
pub fn create_bash_provider() -> Box<dyn ShellProvider> {
    Box::new(bash::BashShellProvider::new())
}

/// 创建 PowerShell ShellProvider
pub fn create_powershell_provider() -> Box<dyn ShellProvider> {
    Box::new(powershell::PowerShellShellProvider::new())
}
```

### 2. BashShellProvider（含 Windows fallback）

```rust
// crates/core/src/tools/shell/bash.rs
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub struct BashShellProvider {
    shell_path: PathBuf,
    #[cfg(windows)]
    is_powershell_fallback: bool,
}

impl BashShellProvider {
    pub fn new() -> Self {
        #[cfg(unix)]
        {
            Self { shell_path: PathBuf::from("/bin/sh") }
        }
        #[cfg(windows)]
        {
            if let Some(bash) = Self::find_git_bash() {
                println!("[BashShellProvider] 使用 Git Bash: {}", bash.display());
                Self { shell_path: bash, is_powershell_fallback: false }
            } else {
                println!("[BashShellProvider] 未找到 Git Bash，fallback 到 PowerShell");
                Self { shell_path: PathBuf::from("powershell"), is_powershell_fallback: true }
            }
        }
    }

    #[cfg(windows)]
    fn find_git_bash() -> Option<PathBuf> {
        use std::env;
        // 1. PATH 中搜索
        if let Ok(path) = which::which("bash") {
            return Some(path);
        }
        // 2. 常见 Git for Windows 安装路径
        let candidates = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
            r"C:\Git\bin\bash.exe",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() { return Some(p); }
        }
        // 3. 从环境变量 GIT_INSTALL_ROOT 推断
        if let Ok(git_root) = env::var("GIT_INSTALL_ROOT") {
            let p = PathBuf::from(&git_root).join("bin").join("bash.exe");
            if p.exists() { return Some(p); }
        }
        None
    }

    #[cfg(windows)]
    pub fn is_powershell_fallback(&self) -> bool {
        self.is_powershell_fallback
    }
}

#[async_trait]
impl ShellProvider for BashShellProvider {
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        let mut cmd = Command::new(&self.shell_path);

        #[cfg(unix)]
        cmd.arg("-lc").arg(command);

        #[cfg(windows)]
        {
            if self.is_powershell_fallback {
                // Fallback 模式：用 PowerShell 执行，但包装 UTF-8
                cmd.arg("-NoLogo")
                    .arg("-NonInteractive")
                    .arg("-Command")
                    .arg(wrap_powershell_command(command));
            } else {
                // Git Bash 模式
                cmd.arg("-lc").arg(command);
            }
        }

        cmd.current_dir(cwd);
        // 以下逻辑复用现有 bash.rs 的实现：
        // - timeout(Duration::from_secs(120), process.output())
        // - decode_command_output()（UTF-8 / GBK 智能解码）
        // - 合并 stdout + stderr
        // - exit code 非零返回错误
        // - truncate_text(..., 50_000)
        // 此处省略具体实现，改造时直接迁移现有代码。
        todo!() // 占位符，实际实现时移除
    }

    fn name(&self) -> &'static str {
        #[cfg(unix)]
        { "bash" }
        #[cfg(windows)]
        {
            if self.is_powershell_fallback {
                "powershell (fallback for bash tool)"
            } else {
                "bash"
            }
        }
    }
}
```

### 3. PowerShellShellProvider

```rust
// crates/core/src/tools/shell/powershell.rs
use tokio::process::Command;

pub struct PowerShellShellProvider;

impl PowerShellShellProvider {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl ShellProvider for PowerShellShellProvider {
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        let mut cmd = Command::new("powershell");
        cmd.arg("-NoLogo")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(wrap_powershell_command(command));
        cmd.current_dir(cwd);
        // 复用现有 bash.rs 的执行逻辑（timeout、decode、截断等）
        todo!() // 占位符，实际实现时移除
    }

    fn name(&self) -> &'static str { "powershell" }
}

fn wrap_powershell_command(command: &str) -> String {
    format!(
        "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
         [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
         $OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
         chcp 65001 > $null; \
         {command}"
    )
}
```

### 4. BashTool 改造

```rust
// crates/core/src/tools/bash.rs
use super::shell::{ShellProvider, create_bash_provider};

pub struct BashTool {
    provider: Box<dyn ShellProvider>,
}

impl BashTool {
    pub fn new() -> Self {
        Self { provider: create_bash_provider() }
    }
}

impl super::AgentToolbox {
    pub(crate) async fn run_bash(&self, command: &str) -> AgentResult<String> {
        // 危险命令检测（保持不变）
        // ...
        self.bash_tool.provider.exec(command, &self.workspace_root).await
    }
}
```

### 5. PowerShellTool 新增

```rust
// crates/core/src/tools/powershell.rs（新建）
use super::shell::{ShellProvider, create_powershell_provider};

pub struct PowerShellTool {
    provider: Box<dyn ShellProvider>,
}

impl PowerShellTool {
    pub fn new() -> Self {
        Self { provider: create_powershell_provider() }
    }

    pub fn schema() -> serde_json::Value {
        json!({
            "name": "powershell",
            "description": "在 Windows 上执行 PowerShell 命令。仅在需要 Windows 原生功能（如 WMI、Registry、.NET）时使用。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "要执行的 PowerShell 命令"
                    }
                },
                "required": ["command"]
            }
        })
    }
}
```

### 6. 工具注册

```rust
// crates/core/src/tools/mod.rs
impl AgentToolbox {
    pub fn new(...) -> Self {
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(BashTool::new()),
            // ... 其他工具
        ];

        // PowerShellTool 默认禁用，通过环境变量开启
        if std::env::var("AGENT_ENABLE_POWERSHELL_TOOL").is_ok() {
            tools.push(Box::new(PowerShellTool::new()));
        }

        Self { ... }
    }
}
```

### 7. 系统提示词

```rust
// agent.rs build_system_prompt
#[cfg(windows)]
let (platform, shell_hint) = {
    let is_git_bash = /* 检测 BashShellProvider 是否在用 Git Bash */;
    if is_git_bash {
        ("Windows (Git Bash)", "Bash 工具使用 Git Bash，支持标准 Unix 命令。")
    } else {
        ("Windows (PowerShell fallback)", "Bash 工具当前 fallback 到 PowerShell。请使用 PowerShell 语法：Get-ChildItem 代替 ls，Get-Content 代替 cat。建议安装 Git for Windows 以获得更好的 bash 支持。")
    }
};

#[cfg(unix)]
let platform = "Unix (bash)";
```

## 目录结构

```
crates/core/src/tools/
├── mod.rs              # AgentToolbox 注册逻辑
├── bash.rs             # BashTool（改造：调用 ShellProvider）
├── powershell.rs       # PowerShellTool（新增）
├── shell/
│   ├── mod.rs          # ShellProvider trait + 工厂
│   ├── bash.rs         # BashShellProvider（含 Windows fallback）
│   └── powershell.rs   # PowerShellShellProvider
├── file_ops.rs
├── search.rs
├── skill_ops.rs
├── curl.rs
├── extension.rs
└── schemas.rs
```

## 向后兼容性

| 场景 | 行为变化 |
|---|---|
| Unix | 无变化，BashTool 仍调用 `/bin/sh -lc` |
| Windows + Git Bash 已安装 | BashTool 改用真正的 bash，语义正确，系统提示词说明 "Git Bash" |
| Windows + 无 Git Bash | BashTool fallback 到 PowerShell，行为与改造前完全一致，系统提示词说明 "PowerShell fallback" |
| Windows + 启用 PowerShellTool | 新增 `powershell` 独立工具可用 |

## 风险与缓解

1. **模型在 fallback 模式下仍输出 bash 语法**：系统提示词会明确告知当前实际 shell 类型，让模型自适应。
2. **Git Bash 检测开销**：只在 `BashShellProvider::new()`（启动时）执行一次，无运行时开销。
3. **路径含空格**：`Command::new` 传入 `PathBuf` 会自动处理，无需手动 quote。

## 实施步骤

1. 新建 `tools/shell/` 模块，定义 `ShellProvider` trait
2. 实现 `BashShellProvider`（含 Windows Git Bash 检测 + PowerShell fallback）
   - **依赖**：若使用 `which` crate 检测 `bash.exe`，需在 `crates/core/Cargo.toml` 添加 `which = "7"`；也可手动遍历 PATH 避免新增依赖
3. 实现 `PowerShellShellProvider`
4. 改造 `BashTool`，改用 `BashShellProvider`
   - 在 `BashTool` 上暴露 `actual_shell_name()` 方法，供 `build_system_prompt` 动态生成平台提示
5. 新建 `PowerShellTool`
6. 更新 `AgentToolbox` 注册逻辑
7. 更新系统提示词（通过 `toolbox.bash_shell_name()` 动态检测实际 shell 类型）
8. 测试矩阵：
   - Unix：行为不变
   - Windows + Git Bash：用 bash
   - Windows + 无 Git Bash：fallback 到 PowerShell
   - PowerShellTool 启用/禁用
