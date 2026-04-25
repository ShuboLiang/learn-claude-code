# ShellProvider 抽象实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `bash.rs` 中的硬编码 PowerShell 调用解耦为 `ShellProvider` 抽象，Windows 上优先使用 Git Bash，无 Git Bash 时 fallback 到 PowerShell；新增可选的独立 `PowerShellTool`。

**Architecture:** 引入 `ShellProvider` trait 统一 shell 执行接口。`BashShellProvider` 在 Windows 上检测 Git Bash，找不到则内部 fallback 到 PowerShell。`PowerShellShellProvider` 供独立的 `PowerShellTool` 使用。通用执行逻辑（超时、解码、截断）提取到共享函数。

**Tech Stack:** Rust 2024, tokio::process, async-trait, anyhow

---

## 文件结构映射

| 文件 | 操作 | 职责 |
|---|---|---|
| `crates/core/src/tools/shell/mod.rs` | 新建 | ShellProvider trait + 共享执行函数 `run_shell_command` |
| `crates/core/src/tools/shell/bash.rs` | 新建 | BashShellProvider（Git Bash 检测 + PowerShell fallback） |
| `crates/core/src/tools/shell/powershell.rs` | 新建 | PowerShellShellProvider |
| `crates/core/src/tools/bash.rs` | 修改 | 移除硬编码 spawn 逻辑，改为通过 ShellProvider 执行 |
| `crates/core/src/tools/powershell.rs` | 新建 | PowerShellTool（独立工具） |
| `crates/core/src/tools/mod.rs` | 修改 | AgentToolbox 添加 provider 字段，dispatch 增加 powershell 分支 |
| `crates/core/src/tools/schemas.rs` | 修改 | 添加 powershell 工具 schema |
| `crates/core/src/agent.rs` | 修改 | build_system_prompt 动态反映实际 shell 类型 |

---

## Task 1: 创建 shell 模块框架与共享执行函数

**Files:**
- Create: `crates/core/src/tools/shell/mod.rs`
- Modify: `crates/core/src/tools/mod.rs`（添加 `mod shell;`）

- [ ] **Step 1: 新建 `tools/shell/mod.rs`，定义 trait 和共享函数**

```rust
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

    /// Provider 标识名
    fn name(&self) -> &'static str;
}

/// 通用 shell 命令执行逻辑（超时 120s、合并输出、智能解码、截断）
pub async fn run_shell_command(mut cmd: Command, cwd: &Path) -> AgentResult<String> {
    cmd.current_dir(cwd);

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

/// 智能解码命令输出的字节数据，自动在 UTF-8 和 GBK 之间选择
/// （从原 bash.rs 迁移，签名和逻辑保持不变）
pub fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

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

fn looks_like_mojibake(text: &str) -> bool {
    let has_latin_ext = text.chars().any(|ch| matches!(ch, '\u{0100}'..='\u{024F}'));
    let has_cjk = text.chars().any(|ch| matches!(ch, '\u{4E00}'..='\u{9FFF}'));
    has_latin_ext && !has_cjk
}

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
```

- [ ] **Step 2: 在 `tools/mod.rs` 中注册 shell 子模块**

在 `crates/core/src/tools/mod.rs` 顶部添加：

```rust
mod shell;
```

放在 `mod bash;` 的下一行。

- [ ] **Step 3: 编译检查 shell 模块**

```bash
cd crates/core && cargo check
```

Expected: 通过，无错误。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/tools/shell/mod.rs crates/core/src/tools/mod.rs
git commit -m "feat(tools): add ShellProvider trait and shared execution logic"
```

---

## Task 2: 实现 BashShellProvider（含 Windows Git Bash 检测）

**Files:**
- Create: `crates/core/src/tools/shell/bash.rs`

- [ ] **Step 1: 新建 `tools/shell/bash.rs`**

```rust
//! Bash Shell Provider
//!
//! Unix：使用 /bin/sh -lc
//! Windows：优先寻找 Git Bash（bash.exe），找不到则 fallback 到 PowerShell

use std::path::{Path, PathBuf};
use async_trait::async_trait;
use tokio::process::Command;

use crate::AgentResult;
use super::{ShellProvider, run_shell_command};

pub struct BashShellProvider {
    shell_path: PathBuf,
    #[cfg(windows)]
    is_powershell_fallback: bool,
}

impl BashShellProvider {
    pub fn new() -> Self {
        #[cfg(unix)]
        {
            Self {
                shell_path: PathBuf::from("/bin/sh"),
            }
        }
        #[cfg(windows)]
        {
            if let Some(bash) = Self::find_git_bash() {
                println!("[BashShellProvider] 使用 Git Bash: {}", bash.display());
                Self {
                    shell_path: bash,
                    is_powershell_fallback: false,
                }
            } else {
                println!("[BashShellProvider] 未找到 Git Bash，fallback 到 PowerShell");
                Self {
                    shell_path: PathBuf::from("powershell"),
                    is_powershell_fallback: true,
                }
            }
        }
    }

    #[cfg(windows)]
    fn find_git_bash() -> Option<PathBuf> {
        use std::env;

        // 1. PATH 中搜索
        if let Ok(path) = env::var("PATH") {
            for dir in path.split(';') {
                let candidate = PathBuf::from(dir).join("bash.exe");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // 2. 常见 Git for Windows 安装路径
        let candidates = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
            r"C:\Git\bin\bash.exe",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() {
                return Some(p);
            }
        }

        // 3. 从 GIT_INSTALL_ROOT 推断
        if let Ok(git_root) = env::var("GIT_INSTALL_ROOT") {
            let p = PathBuf::from(&git_root).join("bin").join("bash.exe");
            if p.exists() {
                return Some(p);
            }
        }

        None
    }

    /// 返回当前实际使用的 shell 名称（供系统提示词使用）
    pub fn actual_shell_name(&self) -> &'static str {
        #[cfg(unix)]
        {
            "bash"
        }
        #[cfg(windows)]
        {
            if self.is_powershell_fallback {
                "powershell"
            } else {
                "bash"
            }
        }
    }
}

#[async_trait]
impl ShellProvider for BashShellProvider {
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        let mut cmd = Command::new(&self.shell_path);

        #[cfg(unix)]
        {
            cmd.arg("-lc").arg(command);
        }

        #[cfg(windows)]
        {
            if self.is_powershell_fallback {
                cmd.arg("-NoLogo")
                    .arg("-NonInteractive")
                    .arg("-Command")
                    .arg(wrap_powershell_command(command));
            } else {
                cmd.arg("-lc").arg(command);
            }
        }

        run_shell_command(cmd, cwd).await
    }

    fn name(&self) -> &'static str {
        self.actual_shell_name()
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
```

- [ ] **Step 2: 添加单元测试（Git Bash 检测逻辑）**

在 `tools/shell/bash.rs` 末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    #[cfg(windows)]
    fn find_git_bash_from_path() {
        let tmp = TempDir::new().unwrap();
        let bash_exe = tmp.path().join("bash.exe");
        fs::write(&bash_exe, "").unwrap();

        let original = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{};{}", tmp.path().display(), original),
        );

        let found = BashShellProvider::find_git_bash();
        assert!(found.is_some());
        assert_eq!(found.unwrap(), bash_exe);

        std::env::set_var("PATH", original);
    }

    #[test]
    #[cfg(windows)]
    fn find_git_bash_returns_none_when_not_found() {
        std::env::remove_var("GIT_INSTALL_ROOT");
        // 临时清空 PATH 中的 bash
        let original = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", "");
        let found = BashShellProvider::find_git_bash();
        assert!(found.is_none());
        std::env::set_var("PATH", original);
    }

    #[test]
    fn actual_shell_name_on_unix_is_bash() {
        let provider = BashShellProvider::new();
        assert_eq!(provider.actual_shell_name(), "bash");
    }
}
```

> **注意：** 若 `tempfile` 不在 dev-dependencies 中，在 `crates/core/Cargo.toml` 的 `[dev-dependencies]` 下添加 `tempfile = "3"`。

- [ ] **Step 3: 编译检查**

```bash
cd crates/core && cargo check
```

Expected: 通过。

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/tools/shell/bash.rs
git commit -m "feat(tools): add BashShellProvider with Windows Git Bash detection"
```

---

## Task 3: 实现 PowerShellShellProvider

**Files:**
- Create: `crates/core/src/tools/shell/powershell.rs`

- [ ] **Step 1: 新建 `tools/shell/powershell.rs`**

```rust
//! PowerShell Shell Provider

use std::path::Path;
use async_trait::async_trait;
use tokio::process::Command;

use crate::AgentResult;
use super::{ShellProvider, run_shell_command};

pub struct PowerShellShellProvider;

impl PowerShellShellProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ShellProvider for PowerShellShellProvider {
    async fn exec(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        let mut cmd = Command::new("powershell");
        cmd.arg("-NoLogo")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(wrap_powershell_command(command));
        run_shell_command(cmd, cwd).await
    }

    fn name(&self) -> &'static str {
        "powershell"
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
```

- [ ] **Step 2: 编译检查**

```bash
cd crates/core && cargo check
```

Expected: 通过。

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/tools/shell/powershell.rs
git commit -m "feat(tools): add PowerShellShellProvider"
```

---

## Task 4: 改造 BashTool 使用 ShellProvider

**Files:**
- Modify: `crates/core/src/tools/bash.rs`
- Modify: `crates/core/src/tools/mod.rs`

- [ ] **Step 1: 修改 `tools/mod.rs`，为 AgentToolbox 添加 bash_provider 字段**

在 `AgentToolbox` 结构体中，添加字段：

```rust
pub struct AgentToolbox {
    pub(crate) workspace_root: PathBuf,
    pub(crate) skills: Arc<RwLock<SkillLoader>>,
    pub(crate) skill_dirs: Vec<PathBuf>,
    pub(crate) todo: Arc<Mutex<TodoManager>>,
    pub(crate) extension: Option<Arc<dyn ToolExtension>>,
    pub(crate) curl_client: CurlClient,
    pub(crate) bash_provider: Box<dyn shell::ShellProvider>,
}
```

在 `AgentToolbox::new()` 中初始化：

```rust
pub fn new(
    workspace_root: PathBuf,
    skills: Arc<RwLock<SkillLoader>>,
    skill_dirs: Vec<PathBuf>,
) -> Self {
    Self {
        workspace_root,
        skills,
        skill_dirs,
        todo: Arc::new(Mutex::new(TodoManager::default())),
        extension: None,
        curl_client: match crate::infra::config::AppConfig::load() {
            Ok(config) => CurlClient::from_config(&config),
            Err(_) => CurlClient::default(),
        },
        bash_provider: Box::new(shell::bash::BashShellProvider::new()),
    }
}
```

- [ ] **Step 2: 修改 `tools/bash.rs`，`run_bash` 改用 provider**

将 `crates/core/src/tools/bash.rs` 中 `run_bash` 的实现替换为：

```rust
impl super::AgentToolbox {
    /// 执行 shell 命令并返回输出
    pub(crate) async fn run_bash(&self, command: &str) -> AgentResult<String> {
        // 危险命令检测（保持不变）
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

        self.bash_provider.exec(command, &self.workspace_root).await
    }
}
```

同时移除原 `bash.rs` 中以下不再需要的函数和 import：
- `wrap_powershell_command`（已移至 shell 模块）
- `decode_command_output`（已移至 `shell::decode_command_output`）
- `looks_like_mojibake`
- `decoding_score`
- `tokio::process::Command` import（如果其他地方不用）
- `tokio::time::{Duration, timeout}` import（如果其他地方不用）
- `crate::infra::utils::truncate_text` import（如果其他地方不用）

保留原 `bash.rs` 中的测试（因为它们测试的是 `decode_command_output`，需要从 `shell` 模块重新导入）。

- [ ] **Step 3: 更新 `tools/mod.rs` 测试中的 import**

将测试模块中的：

```rust
use super::{AgentToolbox, bash::decode_command_output};
```

改为：

```rust
use super::AgentToolbox;
use super::shell::decode_command_output;
```

- [ ] **Step 4: 编译检查**

```bash
cd crates/core && cargo check
```

Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/tools/bash.rs crates/core/src/tools/mod.rs
git commit -m "refactor(tools): BashTool uses ShellProvider abstraction"
```

---

## Task 5: 新增 PowerShellTool

**Files:**
- Create: `crates/core/src/tools/powershell.rs`
- Modify: `crates/core/src/tools/mod.rs`
- Modify: `crates/core/src/tools/schemas.rs`

- [ ] **Step 1: 新建 `tools/powershell.rs`**

```rust
//! PowerShell 工具：独立的 PowerShell 命令执行工具

use std::path::Path;
use serde_json::Value;

use crate::AgentResult;
use super::shell::powershell::PowerShellShellProvider;
use super::shell::ShellProvider;
use super::required_string;

pub struct PowerShellTool {
    provider: PowerShellShellProvider,
}

impl PowerShellTool {
    pub fn new() -> Self {
        Self {
            provider: PowerShellShellProvider::new(),
        }
    }

    pub async fn run(&self, command: &str, cwd: &Path) -> AgentResult<String> {
        self.provider.exec(command, cwd).await
    }
}
```

- [ ] **Step 2: 修改 `tools/mod.rs`，添加 PowerShellTool 支持**

在 `tools/mod.rs` 顶部添加：

```rust
mod powershell;
```

在 `AgentToolbox` 结构体中添加：

```rust
pub struct AgentToolbox {
    // ... 现有字段 ...
    pub(crate) bash_provider: Box<dyn shell::ShellProvider>,
    pub(crate) powershell_tool: Option<PowerShellTool>,
}
```

在 `AgentToolbox::new()` 中初始化：

```rust
pub fn new(
    workspace_root: PathBuf,
    skills: Arc<RwLock<SkillLoader>>,
    skill_dirs: Vec<PathBuf>,
) -> Self {
    let powershell_tool = if std::env::var("AGENT_ENABLE_POWERSHELL_TOOL").is_ok() {
        Some(powershell::PowerShellTool::new())
    } else {
        None
    };

    Self {
        workspace_root,
        skills,
        skill_dirs,
        todo: Arc::new(Mutex::new(TodoManager::default())),
        extension: None,
        curl_client: match crate::infra::config::AppConfig::load() {
            Ok(config) => CurlClient::from_config(&config),
            Err(_) => CurlClient::default(),
        },
        bash_provider: Box::new(shell::bash::BashShellProvider::new()),
        powershell_tool,
    }
}
```

在 `dispatch()` 的 `match name` 中添加 `"powershell"` 分支：

```rust
"powershell" => {
    if let Some(tool) = &self.powershell_tool {
        tool.run(required_string(input, "command")?, &self.workspace_root).await?
    } else {
        bail!("PowerShell 工具未启用。设置 AGENT_ENABLE_POWERSHELL_TOOL=1 后重试。")
    }
}
```

- [ ] **Step 3: 修改 `tools/schemas.rs`，添加 powershell schema**

在 `tool_schemas()` 函数的 `let mut tools = vec![...]` 数组中，在 `bash` 条目之后插入：

```rust
json!({
    "name": "powershell",
    "description": "在 Windows 上执行 PowerShell 命令。仅在需要 Windows 原生功能（如 WMI、Registry、.NET）时，且已启用 AGENT_ENABLE_POWERSHELL_TOOL 时使用。",
    "input_schema": {
        "type": "object",
        "properties": {
            "command": { "type": "string", "description": "要执行的 PowerShell 命令" }
        },
        "required": ["command"]
    }
}),
```

- [ ] **Step 4: 编译检查**

```bash
cd crates/core && cargo check
```

Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/tools/powershell.rs crates/core/src/tools/mod.rs crates/core/src/tools/schemas.rs
git commit -m "feat(tools): add optional PowerShellTool"
```

---

## Task 6: 更新系统提示词

**Files:**
- Modify: `crates/core/src/agent.rs`

- [ ] **Step 1: 修改 `build_system_prompt`，动态检测实际 shell 类型**

找到 `build_system_prompt` 函数中的平台提示部分，替换为：

```rust
let platform = if cfg!(windows) {
    "Windows"
} else {
    "Unix (bash)"
};

let shell_hint = if cfg!(windows) {
    // BashShellProvider 在 Windows 上可能是 Git Bash 或 PowerShell fallback
    // 由于 build_system_prompt 不持有 AgentToolbox，我们用静态提示
    "Bash 工具在 Windows 上优先使用 Git Bash；如未安装则 fallback 到 PowerShell。"
} else {
    "使用标准 bash 语法。"
};
```

然后在 format 字符串中，将原来硬编码的 PowerShell 语法指导替换为：

```rust
format!(
    "{identity_line}\n工作目录：{}。\n平台：{platform}\n{shell_hint}\n优先使用工具解决问题，避免冗长解释。\n\n\
    任务执行流程 — 每个任务必须按以下顺序执行：\n\
    ...",
    workspace_root.display(),
    // ...
)
```

> **说明：** 更精确的动态提示（如"当前实际使用 PowerShell fallback"）需要把 `AgentToolbox` 或 `bash_provider` 传入 `build_system_prompt`。由于该函数目前只接收基础参数，可以在后续迭代中增强。当前先移除硬编码的 PowerShell 语法指导，改为更中性的平台描述。

- [ ] **Step 2: 编译检查**

```bash
cd crates/core && cargo check
```

Expected: 通过。

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/agent.rs
git commit -m "refactor(agent): update system prompt for ShellProvider abstraction"
```

---

## Task 7: 集成测试与验证

- [ ] **Step 1: 运行 core crate 的单元测试**

```bash
cd crates/core && cargo test
```

Expected: 所有测试通过。

- [ ] **Step 2: 验证编译整个 workspace**

```bash
cargo check --workspace
```

Expected: 通过，无错误。

- [ ] **Step 3: 手动验证场景（开发者自测）**

| 场景 | 验证方式 |
|---|---|
| Unix | `cargo run` 后执行 bash 命令，确认行为不变 |
| Windows + Git Bash | 确认 `[BashShellProvider] 使用 Git Bash` 日志出现 |
| Windows + 无 Git Bash | 确认 `[BashShellProvider] fallback 到 PowerShell` 日志出现，bash 命令仍能执行 |
| PowerShellTool 启用 | `set AGENT_ENABLE_POWERSHELL_TOOL=1` 后运行，确认 `powershell` 工具出现在 schema 列表中 |

- [ ] **Step 4: Commit**

```bash
git commit --allow-empty -m "test: verify ShellProvider integration"
```

---

## 实施检查清单

- [ ] `ShellProvider` trait 和 `run_shell_command` 共享函数
- [ ] `BashShellProvider`（Git Bash 检测 + PowerShell fallback）
- [ ] `PowerShellShellProvider`
- [ ] `BashTool` 改用 `ShellProvider`
- [ ] `PowerShellTool` 独立工具（默认禁用）
- [ ] `AgentToolbox` 注册逻辑更新
- [ ] `schemas.rs` 添加 powershell schema
- [ ] 系统提示词更新
- [ ] `cargo test` 通过
- [ ] `cargo check --workspace` 通过
