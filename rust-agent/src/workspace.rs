use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};

use crate::AgentResult;

/// 将用户提供的路径安全地解析为工作区内的绝对路径
///
/// # 参数
/// - `root`: 工作区根目录，通常是 Agent 启动时的当前目录（由 `std::env::current_dir()` 获取）
/// - `input`: 用户提供的路径字符串，可以是相对路径（如 "src/main.rs"）或绝对路径
///
/// # 返回值
/// 解析后的绝对路径，保证位于工作区 `root` 内部
///
/// # 使用场景
/// 在 `tools.rs` 中被 `read_file`、`write_file`、`edit_file` 三个文件操作工具调用，
/// 确保用户提供的路径不会逃逸到工作区之外（防止路径遍历攻击）
///
/// # 运作原理
/// 1. 先确定基准路径 `base`：如果 `root` 在磁盘上存在就用 `canonicalize()` 获取真实路径；
///    如果不存在但是绝对路径就直接规范化；否则拼上当前工作目录再规范化
/// 2. 如果用户输入是绝对路径，直接规范化；否则拼到 `base` 上再规范化
/// 3. 最终检查结果路径是否以 `base` 开头，不是的话说明路径逃逸了，直接报错
pub fn resolve_workspace_path(root: &Path, input: &str) -> AgentResult<PathBuf> {
    let base = if root.exists() {
        root.canonicalize()
            .with_context(|| format!("Failed to resolve workspace root: {}", root.display()))?
    } else if root.is_absolute() {
        normalize_path(root.to_path_buf())
    } else {
        normalize_path(
            std::env::current_dir()
                .context("Failed to determine current directory")?
                .join(root),
        )
    };
    let joined = if Path::new(input).is_absolute() {
        normalize_path(PathBuf::from(input))
    } else {
        normalize_path(base.join(input))
    };

    if !joined.starts_with(&base) {
        bail!("Path escapes workspace: {input}");
    }

    Ok(joined)
}

/// 规范化路径：消除 `.`（当前目录）和 `..`（上级目录）组件
///
/// # 参数
/// - `path`: 待规范化的路径
///
/// # 返回值
/// 规范化后的路径（不含 `.` 和 `..`）
///
/// # 使用场景
/// 仅被 `resolve_workspace_path` 内部调用，用于在比较路径前将路径标准化
///
/// # 运作原理
/// 遍历路径的每个组件：
/// - 遇到 `.`（当前目录）直接跳过
/// - 遇到 `..`（上级目录）就从已构建的路径中弹出最后一级
/// - 其他正常组件直接追加
fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}
