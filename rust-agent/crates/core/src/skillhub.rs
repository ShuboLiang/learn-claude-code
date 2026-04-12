use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use tokio::process::Command;

use crate::AgentResult;

/// SkillHub CLI 安装脚本地址
const INSTALL_URL: &str =
    "https://skillhub-1388575217.cos.ap-guangzhou.myqcloud.com/install/install.sh";

/// 检查 SkillHub CLI 是否已安装，未安装则自动安装（仅 CLI）
///
/// # 返回值
/// `true` 表示 CLI 可用，`false` 表示不可用（如 Windows 平台暂不支持自动安装）
///
/// # 运作原理
/// 1. 执行 `skillhub --version` 检查 CLI 是否可用
/// 2. 如果不可用且在 Unix 平台，通过 curl 下载安装脚本并执行（带 `--cli-only` 参数）
/// 3. Windows 平台暂不支持自动安装，打印提示信息
pub async fn ensure_cli_installed() -> bool {
    if is_cli_available().await {
        return true;
    }

    println!("SkillHub CLI 未安装，正在自动安装...");

    if cfg!(windows) {
        eprintln!(
            "提示：Windows 平台暂不支持自动安装 SkillHub CLI。\n\
             请手动执行：curl -fsSL {INSTALL_URL} | bash -s -- --cli-only"
        );
        return false;
    }

    let install_cmd = format!(
        "curl -fsSL {INSTALL_URL} | bash -s -- --cli-only"
    );

    match run_shell_command(&install_cmd, None).await {
        Ok(output) => {
            println!("{}", output);
            if is_cli_available().await {
                println!("SkillHub CLI 安装成功。");
                true
            } else {
                eprintln!("SkillHub CLI 安装后仍不可用，请检查安装日志。");
                false
            }
        }
        Err(e) => {
            eprintln!("SkillHub CLI 安装失败：{e}");
            false
        }
    }
}

/// 检查 SkillHub CLI 是否可用
///
/// # 运作原理
/// 执行 `skillhub --version`，如果输出以 "skillhub" 开头则判定为已安装
async fn is_cli_available() -> bool {
    match Command::new("skillhub")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            stdout.trim().starts_with("skillhub") || stderr.trim().starts_with("skillhub")
        }
        Err(_) => false,
    }
}

/// 搜索 SkillHub 技能商店中的技能
///
/// # 参数
/// - `query`: 搜索关键词
///
/// # 返回值
/// 搜索结果文本
pub async fn search(query: &str) -> AgentResult<String> {
    run_skillhub_command(&["search", query]).await
}

/// 从 SkillHub 安装指定技能到 workspace 的 skills 目录
///
/// # 参数
/// - `name`: 技能名称
/// - `workspace_root`: workspace 根目录路径
///
/// # 返回值
/// 安装结果文本
pub async fn install(name: &str, _workspace_root: &Path) -> AgentResult<String> {
    // 安装到用户目录 ~/.rust-agent/
    // skillhub CLI 会在 cwd 下创建 skills/<name>/ 子目录
    let target_dir = dirs::home_dir()
        .map(|p| p.join(".rust-agent"))
        .context("无法获取用户主目录")?;
    std::fs::create_dir_all(&target_dir)
        .with_context(|| format!("创建技能目录失败：{}", target_dir.display()))?;

    let output = run_shell_command(
        &format!("skillhub install {name}"),
        Some(&target_dir),
    )
    .await
    .with_context(|| format!("安装技能 '{name}' 失败"))?;
    Ok(format!(
        "技能 '{name}' 安装到 {}。\n{output}",
        target_dir.display()
    ))
}

/// 执行 skillhub 子命令并返回输出
async fn run_skillhub_command(args: &[&str]) -> AgentResult<String> {
    run_shell_command(&format!("skillhub {}", args.join(" ")), None).await
}

/// 通过 shell 执行命令并返回合并的 stdout/stderr 输出
async fn run_shell_command(command: &str, cwd: Option<&Path>) -> AgentResult<String> {
    let mut process = if cfg!(windows) {
        let mut cmd = Command::new("powershell");
        cmd.arg("-NoLogo")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(command);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-lc").arg(command);
        cmd
    };

    if let Some(dir) = cwd {
        process.current_dir(dir);
    }

    let output = process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("执行命令失败：{command}"))?;

    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    let trimmed = combined.trim();
    if trimmed.is_empty() {
        Ok("(无输出)".to_owned())
    } else {
        Ok(trimmed.to_owned())
    }
}
