use std::path::Path;

/// 列出技能目录的文件树结构
///
/// 接收技能目录的真实路径，返回目录树文本，让 Claude 了解技能的完整文件结构并自行判断调用方式。
pub(crate) fn list_skill_tree(skill_dir: &Path) -> String {
    if !skill_dir.exists() {
        return String::new();
    }

    let mut lines = vec![format!("技能目录结构 ({}):", skill_dir.display())];
    for entry in walkdir::WalkDir::new(skill_dir)
        .sort_by(|a, b| a.file_name().cmp(b.file_name()))
        .into_iter()
        .filter_map(Result::ok)
    {
        let depth = entry.depth();
        let indent = "  ".repeat(depth);
        let name = entry.file_name().to_string_lossy();
        if depth == 0 || entry.file_type().is_dir() {
            lines.push(format!("{indent}{name}/"));
        } else {
            lines.push(format!("{indent}{name}"));
        }
    }

    lines.join("\n")
}
